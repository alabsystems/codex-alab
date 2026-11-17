// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real original Child/TempPath ownership only. No Native/F1/Go/Entry or App
//! permission is fabricated by these controls.
use super::*;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

async fn started(actual: &Operation) -> io::Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            if actual
                .0
                .progress
                .lock()
                .map_err(|_| unavailable("test actual state poisoned"))?
                .started
            {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(io::Error::other)?
}

#[tokio::test(flavor = "current_thread")]
async fn original_child_and_document_survive_cancelled_wait_and_pre_h1_freeze() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
    let path = document.to_path_buf();
    std::fs::write(&path, "seed")?;
    let marker = root.path().join("starts");
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "printf start >> \"$2\"; IFS= read -r release; printf 'actual edit\\n' > \"$1\"",
            "editor",
        ])
        .arg(&path)
        .arg(&marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let operation = Operation::prepare(command, document)?;
    // The real processed Launch event can schedule the original worker before
    // handing control back to an update select. A second observer must reuse it.
    operation.start_original()?;
    operation.start_original()?;
    let observer = tokio::spawn({
        let same = operation.clone();
        async move { same.wait().await }
    });
    started(&operation).await?;
    observer.abort();
    assert!(observer.await.is_err_and(|error| error.is_cancelled()));
    let fence = operation.freeze_before_h1().await?;
    assert!(operation.is_for_fence(&fence));
    assert!(fence.finished_pending()?.is_none());
    let mut input = operation
        .0
        .progress
        .lock()
        .map_err(|_| unavailable("test actual state poisoned"))?
        .child
        .as_mut()
        .and_then(|child| child.stdin.take())
        .ok_or_else(|| unavailable("same actual child input absent"))?;
    input.write_all(b"release\n").await?;
    drop(input);
    // The frozen owner cannot reap/read while the original child exits. Its
    // actual Child remains in the same record, no PID/list exclusion is given.
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(
        operation
            .0
            .progress
            .lock()
            .map_err(|_| unavailable("test actual state poisoned"))?
            .finished
            .is_none()
    );
    drop(fence); // Failed-exec/local rollback resumes SAME original waiter.
    let finished = tokio::time::timeout(std::time::Duration::from_secs(60), operation.wait())
        .await
        .map_err(io::Error::other)??;
    assert!(finished.is_for(&operation));
    assert_eq!(
        finished
            .outcome()
            .map_err(|error| io::Error::other(error.to_string()))?,
        "actual edit\n"
    );
    assert_eq!(std::fs::read_to_string(marker)?, "start");
    // Completed root/read retains its exact immutable result across a later cut.
    let completed = operation.freeze_before_h1().await?;
    let pending = completed
        .finished_pending()?
        .ok_or_else(|| unavailable("actual completed cut lost its original pending result"))?;
    assert!(pending.is_for(&operation));
    assert!(Arc::ptr_eq(&pending.original, &finished.original));
    assert!(Arc::ptr_eq(&pending.value, &finished.value));
    std::fs::write(&path, "must not reread")?;
    assert_eq!(
        finished
            .outcome()
            .map_err(|error| io::Error::other(error.to_string()))?,
        "actual edit\n"
    );
    assert_eq!(
        pending
            .outcome()
            .map_err(|error| io::Error::other(error.to_string()))?,
        "actual edit\n"
    );
    assert!(
        finished
            .apply_and_retire(|| panic!("frozen callback was applied"))
            .is_err()
    );
    drop(completed);
    finished.apply_and_retire(|| Ok(()))?;
    assert!(operation.freeze_before_h1().await.is_err());
    assert!(
        finished
            .apply_and_retire(|| panic!("original callback repeated"))
            .is_err()
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn failed_original_spawn_has_original_error_and_no_native_reservation() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
    let missing = root.path().join("missing-editor");
    let expected = std::process::Command::new(&missing)
        .spawn()
        .err()
        .ok_or_else(|| unavailable("missing original editor unexpectedly spawned"))?;
    let operation = Operation::prepare(Command::new(&missing), document)?;
    let finished = tokio::time::timeout(std::time::Duration::from_secs(60), operation.wait())
        .await
        .map_err(io::Error::other)??;
    match finished.outcome() {
        Err(Failure::Start(actual)) => {
            assert_eq!(actual.raw_os_error(), expected.raw_os_error());
            assert_eq!(actual.kind(), expected.kind());
            assert_eq!(actual.to_string(), expected.to_string());
        }
        _ => return Err(unavailable("original failed spawn lost exact outcome")),
    }
    assert!(
        operation
            .0
            .progress
            .lock()
            .map_err(|_| unavailable("test actual state poisoned"))?
            .child
            .is_none()
    );
    let cut = operation.freeze_before_h1().await?;
    let pending = cut
        .finished_pending()?
        .ok_or_else(|| unavailable("original failed spawn lost genuine pending outcome"))?;
    assert!(Arc::ptr_eq(&pending.value, &finished.value));
    assert!(matches!(pending.outcome(), Err(Failure::Start(_))));
    drop(cut);
    finished.apply_and_retire(|| Ok(()))?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn local_prestart_cancel_never_runs_original_command_and_read_failure_is_cached()
-> io::Result<()> {
    let root = tempfile::tempdir()?;
    let marker = root.path().join("must-not-start");
    let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "touch \"$1\"", "editor"]).arg(&marker);
    let cancelled = Operation::prepare(command, document)?;
    let receipt = cancelled.cancel_before_start()?;
    assert!(receipt.is_for(&cancelled));
    assert!(cancelled.before_start().is_err());
    assert!(cancelled.wait().await.is_err());
    assert!(!marker.exists());
    let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
    let path = document.to_path_buf();
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "rm \"$1\"", "editor"]).arg(&path);
    let operation = Operation::prepare(command, document)?;
    assert!(!receipt.is_for(&operation));
    let finished = tokio::time::timeout(std::time::Duration::from_secs(60), operation.wait())
        .await
        .map_err(io::Error::other)??;
    let expected = std::fs::read_to_string(&path)
        .err()
        .ok_or_else(|| unavailable("original missing document unexpectedly read"))?;
    match finished.outcome() {
        Err(Failure::Document(actual)) => {
            assert_eq!(actual.raw_os_error(), expected.raw_os_error());
            assert_eq!(actual.to_string(), expected.to_string());
        }
        _ => return Err(unavailable("original document read failure lost")),
    }
    std::fs::write(&path, "replacement after original failure")?;
    let repeated = operation.wait().await?;
    assert!(matches!(repeated.outcome(), Err(Failure::Document(_))));
    finished.apply_and_retire(|| Ok(()))?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn prechild_guard_excludes_real_start_observer_until_release() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let marker = root.path().join("original-started");
    let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "printf started > \"$2\"; printf 'original guarded edit' > \"$1\"",
            "editor",
        ])
        .arg(document.as_os_str())
        .arg(&marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let operation = Operation::prepare(command, document)?;
    let before = operation.before_start()?;
    assert!(before.is_for(&operation));
    let (entered, ready) = std::sync::mpsc::sync_channel(1);
    let (did_return, returned) = std::sync::mpsc::sync_channel(1);
    let observer = tokio::task::spawn_blocking({
        let same = operation.clone();
        move || {
            entered.send(()).map_err(io::Error::other)?;
            let result = same.start();
            did_return.send(()).map_err(io::Error::other)?;
            result
        }
    });
    ready
        .recv_timeout(std::time::Duration::from_secs(60))
        .map_err(io::Error::other)?;
    // Observe the blocking caller's return independently of whether the
    // current-thread runtime can poll the newly scheduled worker. This is a
    // negative observation window, not an editor-start latency assertion.
    let return_while_held = returned.recv_timeout(std::time::Duration::from_millis(50));
    let marker_while_held = marker.exists();
    let started_while_held = operation
        .0
        .progress
        .lock()
        .map_err(|_| unavailable("actual guard test state poisoned"))?
        .started;
    // Release the SAME start gate to the actual observer and original command.
    // This SDK control does not claim publication of a complete App Job.
    drop(before);
    tokio::time::timeout(std::time::Duration::from_secs(60), observer)
        .await
        .map_err(io::Error::other)?
        .map_err(io::Error::other)??;
    let finished = tokio::time::timeout(std::time::Duration::from_secs(60), operation.wait())
        .await
        .map_err(io::Error::other)??;
    let actual_marker = std::fs::read_to_string(marker)?;
    let actual_edit = matches!(finished.outcome(), Ok("original guarded edit"));
    let refused_late_guard = operation.before_start().is_err();
    finished.apply_and_retire(|| Ok(()))?;
    // Clean up the same actual observer, child, read and registry before a
    // negative assertion can fail, including under a broken start gate.
    assert_eq!(
        return_while_held,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    );
    assert!(!marker_while_held);
    assert!(!started_while_held);
    assert_eq!(actual_marker, "started");
    assert!(actual_edit);
    assert!(refused_late_guard);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn original_fifo_document_read_keeps_update_observation_alive_and_pre_h1_waits()
-> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::fs::OpenOptionsExt;
    let root = tempfile::tempdir()?;
    let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
    let path = document.to_path_buf();
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "rm \"$1\" && mkfifo \"$1\"", "editor"])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let operation = Operation::prepare(command, document)?;
    let (opened, opened_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    // The actual FIFO writer has an independent bounded fail-open solely so a
    // regression that blocks the single async thread cannot hang this control.
    let writer = std::thread::spawn(move || -> io::Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut output = loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
            {
                Ok(output) if output.metadata()?.file_type().is_fifo() => break output,
                Ok(_) => {}
                Err(error)
                    if matches!(error.raw_os_error(), Some(libc::ENXIO) | Some(libc::ENOENT)) => {}
                Err(error) => return Err(error),
            }
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let _ = opened.send(());
        let _ = release_rx.recv_timeout(std::time::Duration::from_secs(5));
        output.write_all(b"original FIFO outcome\n")
    });
    let observer = tokio::spawn({
        let operation = operation.clone();
        async move { operation.wait().await }
    });
    let body = async {
        tokio::time::timeout(std::time::Duration::from_secs(60), opened_rx)
            .await
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?;
        // A real async update notification remains observable while the exact
        // original read is blocked, without completing/replacing that read.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        if operation
            .0
            .progress
            .lock()
            .map_err(|_| unavailable("test actual state poisoned"))?
            .finished
            .is_some()
        {
            return Err(unavailable(
                "original blocking read prevented real async update observation",
            ));
        }
        // The distinct C0 route pins this SAME operation and actual opened
        // read without waiting on the progression permit or starting a new IO.
        let capture = operation.capture_c0()?;
        let mut changes = capture.changes();
        let observed = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                let observed = capture.observe()?;
                if observed.phase() == C0Phase::RootReapedReadPending
                    && observed.original_read().is_some_and(|read| {
                        read.phase()
                            .is_ok_and(|phase| phase == OriginalReadPhase::Reading)
                    })
                {
                    break Ok::<_, io::Error>(observed);
                }
                changes.changed().await.map_err(io::Error::other)?;
            }
        })
        .await
        .map_err(io::Error::other)??;
        assert!(observed.is_for(&capture));
        let descriptor = observed
            .original_read()
            .ok_or_else(|| unavailable("actual original read owner absent"))?
            .description()?
            .ok_or_else(|| unavailable("actual original read description absent"))?;
        assert!(
            std::fs::File::from(descriptor)
                .metadata()?
                .file_type()
                .is_fifo()
        );
        let fence = tokio::spawn({
            let operation = operation.clone();
            async move { operation.freeze_before_h1().await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let crossed = fence.is_finished();
        fence.abort();
        let cancelled = fence.await.is_err_and(|error| error.is_cancelled());
        if crossed || !cancelled {
            return Err(unavailable(
                "pre-H1 cut crossed the actual original document read",
            ));
        }
        Ok::<_, io::Error>(())
    }
    .await;
    let _ = release.send(());
    let writer_result = tokio::task::spawn_blocking(move || writer.join())
        .await
        .map_err(io::Error::other)?
        .map_err(|_| unavailable("test original FIFO writer panicked"))?;
    let finished = tokio::time::timeout(std::time::Duration::from_secs(60), observer)
        .await
        .map_err(io::Error::other)?
        .map_err(io::Error::other)??;
    let same_outcome = finished
        .outcome()
        .map_err(|error| io::Error::other(error.to_string()))?
        .to_string();
    finished.apply_and_retire(|| Ok(()))?;
    body?;
    writer_result?;
    assert_eq!(same_outcome, "original FIFO outcome\n");
    Ok(())
}
