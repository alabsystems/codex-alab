// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::MAX_THREAD_LOGS;
use super::MEMORY_PREFIX_BYTES;
use super::ShellLog;
use super::ThreadLogs;
use std::fs;
use std::io;
use std::sync::Arc;

#[cfg(unix)]
impl ShellLog {
    // Capture-level controls exercise the same prepare, verified publish and receipt
    // cleanup as the watcher. Journal and process replacement are exercised separately.
    fn stage_diagnostic(
        &self,
    ) -> io::Result<Option<(String, bool, Option<super::ShellOutputTail>)>> {
        let Some(prepared) = self.prepare_diagnostic()? else {
            return Ok(None);
        };
        if !prepared.publish()? {
            return Ok(None);
        }
        prepared.cleanup(true)?;
        Ok(Some((prepared.name, prepared.partial, prepared.tail)))
    }
}

#[test]
fn failed_directory_setup_can_retry() -> io::Result<()> {
    let logs = ThreadLogs::default();
    let scratch = tempfile::tempdir()?;
    let missing_parent = scratch.path().join("initially-missing");
    let create = || tempfile::tempdir_in(&missing_parent);
    assert!(logs.directory_with(None, create).is_err());

    fs::create_dir(&missing_parent)?;
    let directory = logs.directory_with(None, create)?;
    assert!(directory.directory.path().starts_with(&missing_parent));
    assert!(Arc::ptr_eq(&directory, &logs.directory_with(None, create)?));
    Ok(())
}

#[test]
fn small_running_commands_leave_slots_for_clipped_output() -> io::Result<()> {
    let logs = ThreadLogs::default();
    let directory = logs.directory_with(None, tempfile::tempdir)?;
    let mut short_commands = Vec::with_capacity(MAX_THREAD_LOGS);
    for _ in 0..MAX_THREAD_LOGS {
        let command = ShellLog::new(Some(Arc::clone(&directory)));
        command.append(b"ready\n");
        assert!(command.notice(false, true).is_none());
        short_commands.push(command);
    }
    assert_eq!(fs::read_dir(directory.directory.path())?.count(), 0);

    let running = ShellLog::new(Some(Arc::clone(&directory)));
    running.append(b"early\xff");
    let notice = running.notice(true, true).expect("clipped running output");
    assert_eq!(notice.status, Some("in_progress"));
    let path = notice.path.expect("running command has a log");
    assert_eq!(fs::read(&path)?, b"early\xff");
    running.append(b"\x00last");
    running.close(true);
    let notice = running
        .notice(false, false)
        .expect("previously disclosed log");
    assert_eq!(notice.path.as_ref(), Some(&path));
    assert_eq!(notice.status, None);
    drop(running);
    assert_eq!(fs::read(&path)?, b"early\xff\x00last");

    let interrupted = ShellLog::new(Some(Arc::clone(&directory)));
    interrupted.append(b"\x80partial");
    interrupted.close(false);
    assert_eq!(fs::read_dir(directory.directory.path())?.count(), 1);
    let notice = interrupted
        .notice(true, false)
        .expect("clipped after close");
    assert_eq!(notice.status, Some("partial"));
    let partial_path = notice
        .path
        .expect("interrupted command retained its prefix");
    assert_ne!(partial_path, path);
    drop(interrupted);
    assert_eq!(fs::read(&partial_path)?, b"\x80partial");
    drop(short_commands);
    assert_eq!(fs::read_dir(directory.directory.path())?.count(), 2);
    Ok(())
}

#[test]
fn crossing_the_prefix_limit_writes_every_byte_once() -> io::Result<()> {
    let logs = ThreadLogs::default();
    let directory = logs.directory_with(None, tempfile::tempdir)?;
    let output = ShellLog::new(Some(Arc::clone(&directory)));
    let mut expected: Vec<u8> = (0..=255).cycle().take(MEMORY_PREFIX_BYTES).collect();
    let boundary = MEMORY_PREFIX_BYTES - 1;
    output.append(&expected[..boundary]);
    output.append(&expected[boundary..]);
    assert_eq!(fs::read_dir(directory.directory.path())?.count(), 0);

    output.append(b"\xff");
    output.append(b"\x00\x80tail");
    expected.extend_from_slice(b"\xff\x00\x80tail");
    assert_eq!(fs::read_dir(directory.directory.path())?.count(), 1);
    output.close(true);
    let notice = output
        .notice(true, false)
        .expect("clipped completed output");
    assert_eq!(notice.status, None);
    let path = notice.path.expect("promoted output has a log");
    drop(output);
    assert_eq!(fs::read(&path)?, expected);
    Ok(())
}

#[test]
fn running_commands_recover_their_retained_bytes_after_slots_reopen() -> io::Result<()> {
    let directory = ThreadLogs::default().directory_with(None, tempfile::tempdir)?;
    let mut completed = Vec::with_capacity(MAX_THREAD_LOGS);
    for _ in 0..MAX_THREAD_LOGS {
        let command = ShellLog::new(Some(Arc::clone(&directory)));
        command.append(b"completed");
        command.close(true);
        let notice = command.notice(true, false).expect("clipped completed log");
        completed.push(notice.path.expect("completed command reserved a file"));
    }

    let small = ShellLog::new(Some(Arc::clone(&directory)));
    small.append(b"before\xff");
    let large = ShellLog::new(Some(Arc::clone(&directory)));
    let mut first_chunk: Vec<u8> = (0..=255).cycle().take(MEMORY_PREFIX_BYTES).collect();
    first_chunk.extend_from_slice(b"first omitted bytes");
    large.append(&first_chunk);
    for command in [&small, &large] {
        let denied = command.notice(true, true).expect("clipped running output");
        assert_eq!(denied.path, None);
        assert_eq!(denied.status, Some("unavailable"));
    }

    small.append(b"\x00during");
    fs::remove_file(&completed[0])?;
    let notice = small.notice(false, true).expect("unclipped running poll");
    assert_eq!(notice.status, Some("in_progress"));
    let small_path = notice.path.expect("recovered complete prefix");
    assert_eq!(fs::read(&small_path)?, b"before\xff\x00during");
    small.append(b"after");
    small.close(true);
    let final_notice = small.notice(false, false).expect("completed small output");
    assert_eq!(final_notice.status, None);
    assert_eq!(final_notice.path.as_ref(), Some(&small_path));
    assert_eq!(fs::read(&small_path)?, b"before\xff\x00duringafter");

    fs::remove_file(&completed[1])?;
    let notice = large.notice(false, true).expect("unclipped running poll");
    assert_eq!(notice.status, Some("partial"));
    let large_path = notice.path.expect("recovered truncated prefix");
    large.append(b"late bytes after the gap");
    large.close(true);
    let final_notice = large.notice(false, false).expect("completed large output");
    assert_eq!(final_notice.status, Some("partial"));
    assert_eq!(final_notice.path.as_ref(), Some(&large_path));
    assert_eq!(fs::read(&large_path)?, &first_chunk[..MEMORY_PREFIX_BYTES]);
    assert_eq!(
        fs::read_dir(directory.directory.path())?.count(),
        MAX_THREAD_LOGS
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_unlinked_capture_stays_charged_until_closed() -> io::Result<()> {
    let logs = ThreadLogs::default();
    let directory = logs.directory_with(None, tempfile::tempdir)?;
    let active = directory.create_log()?.expect("active output");

    let mut published = Vec::new();
    for _ in 1..MAX_THREAD_LOGS {
        let file = directory.create_log()?.expect("published output");
        published.push(file.path().to_owned());
        directory.finish(file, true);
    }

    fs::remove_file(active.path())?;
    assert!(directory.create_log()?.is_none());

    let unknown = directory.directory.path().join("renamed-output");
    fs::rename(&published[0], &unknown)?;
    assert!(directory.create_log()?.is_none());
    directory.finish(active, true);
    assert!(directory.create_log()?.is_some());
    assert!(unknown.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn unpublished_replacement_survives_and_both_names_count() -> io::Result<()> {
    use std::io::Write;

    let logs = ThreadLogs::default();
    let directory = logs.directory_with(None, tempfile::tempdir)?;
    let mut original = directory.create_log()?.expect("unpublished output");
    original.write_all(b"original")?;
    let path = original.path().to_owned();
    let moved = directory.directory.path().join("moved-log");
    fs::rename(&path, &moved)?;
    fs::write(&path, b"replacement")?;
    directory.finish(original, false);

    assert_eq!(fs::read(&path)?, b"replacement");
    assert_eq!(fs::read(&moved)?, b"original");
    for _ in 2..MAX_THREAD_LOGS {
        let file = directory.create_log()?.expect("vacant slot");
        directory.finish(file, true);
    }
    assert!(directory.create_log()?.is_none());
    assert_eq!(
        fs::read_dir(directory.directory.path())?.count(),
        MAX_THREAD_LOGS
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn unpublished_name_is_pinned_until_cleanup() -> io::Result<()> {
    let logs = ThreadLogs::default();
    let directory = logs.directory_with(None, tempfile::tempdir)?;
    let output = directory.create_log()?.expect("unpublished output");
    let path = output.path().to_owned();
    assert!(fs::File::open(&path).is_ok());
    assert!(fs::rename(&path, path.with_extension("moved")).is_err());
    assert!(fs::remove_file(&path).is_err());
    directory.finish(output, false);
    assert!(!path.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn diagnostic_staging_requires_a_local_running_result_and_later_raw_bytes() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let logs = ThreadLogs::default();
    let directory = logs.directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let early_only = ShellLog::new(Some(Arc::clone(&directory)));
    early_only.append(b"ready\xff");
    early_only.initial_response(6);
    early_only.close(true);
    assert!(early_only.stage_diagnostic()?.is_none());

    let never_running = ShellLog::new(Some(Arc::clone(&directory)));
    never_running.append(b"compiler failed");
    never_running.close(true);
    assert!(never_running.stage_diagnostic()?.is_none());
    let temporary = ShellLog::new(Some(
        ThreadLogs::default().directory_with(None, tempfile::tempdir)?,
    ));
    temporary.initial_response(0);
    temporary.append(b"temporary failure");
    temporary.close(true);
    assert!(temporary.stage_diagnostic()?.is_none());
    assert!(crate::alab_shell_log_store::existing_directory(home.path(), thread)?.is_none());

    let log = ShellLog::new(Some(directory));
    log.append(b"ready\xff");
    log.initial_response(6);
    log.append(b"\x00compile failure\n");
    assert!(log.stage_diagnostic()?.is_none());
    log.close(true);
    let (name, partial, _) = log.stage_diagnostic()?.expect("late diagnostic");
    assert!(!partial);
    let store = crate::alab_shell_log_store::existing_directory(home.path(), thread)?.unwrap();
    let path = store.path().join(name);
    assert_eq!(fs::read(&path)?, b"ready\xff\x00compile failure\n");
    log.finish_diagnostic(true);
    assert!(log.stage_diagnostic()?.is_none());
    drop(log);
    assert!(path.exists());
    store.retire()?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn already_disclosed_capped_output_settles_as_partial_without_more_bytes() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let logs = ThreadLogs::default();
    let directory = logs.directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let bytes = vec![0xff; super::MAX_LOG_BYTES + 1];
    let log = ShellLog::new(Some(Arc::clone(&directory)));
    log.append(&bytes);
    log.initial_response(bytes.len());
    let notice = log
        .notice(true, true)
        .expect("running capture exceeded the cap");
    assert_eq!(notice.status, Some("partial"));
    let path = notice.path.expect("disclosed file");
    log.close(true);
    let (name, partial, _) = log
        .stage_diagnostic()?
        .expect("terminal state of the disclosed prefix");
    assert!(partial);
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some(name.as_str())
    );
    assert_eq!(fs::metadata(&path)?.len(), super::MAX_LOG_BYTES as u64);
    log.finish_diagnostic(true);
    drop(log);

    let undisclosed = ShellLog::new(Some(directory));
    undisclosed.append(&bytes);
    undisclosed.initial_response(bytes.len());
    undisclosed.close(true);
    assert!(undisclosed.stage_diagnostic()?.is_none());
    let store = crate::alab_shell_log_store::existing_directory(home.path(), thread)?.unwrap();
    assert_eq!(store.count(MAX_THREAD_LOGS)?, 1);
    store.retire()?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn final_poll_accounts_for_each_collection_and_only_retains_unseen_output() -> io::Result<()> {
    use super::super::process::retired_readers::ReaderPool;

    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let pool = Arc::new(ReaderPool::default());
    for late in [b"".as_slice(), b"\0late\xff"] {
        let log = ShellLog::new(Some(Arc::clone(&directory)));
        log.append(b"initial");
        log.initial_response(7);
        log.append(b"poll");
        assert!(!log.collect_poll(4, false, None));
        log.append(b"final");
        assert!(log.retain_reader(Arc::downgrade(&pool)));
        assert!(log.collect_poll(5, true, None));
        log.append(late);
        log.close(true);
        let staged = log.stage_diagnostic()?;
        if late.is_empty() {
            assert!(staged.is_none());
        } else {
            let (name, partial, _) = staged.expect("later original bytes");
            assert!(!partial);
            let store =
                crate::alab_shell_log_store::existing_directory(home.path(), thread)?.unwrap();
            assert_eq!(
                fs::read(store.path().join(name))?,
                b"initialpollfinal\0late\xff"
            );
            log.finish_diagnostic(true);
        }
    }
    crate::alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_canceled_final_poll_releases_only_a_drained_reader_without_pending_evidence() -> io::Result<()>
{
    use super::super::process::retired_readers::ReaderPool;
    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let pool = Arc::new(ReaderPool::default());
    for late in [b"".as_slice(), b"compiler failure"] {
        let log = ShellLog::new(Some(Arc::clone(&directory)));
        log.append(b"seen");
        log.initial_response(4);
        assert!(log.retain_reader(Arc::downgrade(&pool)));
        log.append(late);
        log.close(true);
        let still_owned = || log.capture.lock().unwrap().reader_pool.is_some();
        log.release_drained_reader(false);
        assert_eq!(still_owned(), !late.is_empty());
        if !late.is_empty() {
            assert!(log.stage_diagnostic()?.is_some());
            log.finish_diagnostic(true);
            log.release_drained_reader(false);
            assert!(!still_owned());
        }
    }
    crate::alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn first_terminal_collection_uses_original_range_and_frozen_file_coverage() -> io::Result<()> {
    use super::super::process::retired_readers::ReaderPool;
    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let pool = Arc::new(ReaderPool::default());
    let current = Arc::clone(&pool);
    let next = Arc::new(ReaderPool::default());

    for (returned_bytes, disclose_path, needs_reader) in
        [(6, false, false), (2, false, true), (2, true, false)]
    {
        let mut log = ShellLog::new(Some(Arc::clone(&directory)));
        log.bind_launch_generation(&pool);
        assert!(log.launched_in(&current));
        assert!(!log.launched_in(&next));
        log.append(b"abcdef");
        log.close(true);
        let disclosed = log
            .notice(disclose_path, false)
            .and_then(|notice| notice.disclosed);
        log.collect_first_exit(returned_bytes, disclosed, 19);
        assert_eq!(log.retain_reader(Arc::downgrade(&pool)), needs_reader);
        log.remember_first_admission();
        assert_eq!(log.first_exit_uncollected(), needs_reader);
        assert_eq!(log.diagnostic_ready(), needs_reader);
    }
    crate::alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn stale_first_exit_seals_and_registers_only_the_original_output_that_was_not_returned()
-> io::Result<()> {
    use super::super::process::retired_readers::ReaderPool;
    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    for late in [b"".as_slice(), b"\xffstderr"] {
        let mut log = ShellLog::new(Some(Arc::clone(&directory)));
        let current = Arc::new(ReaderPool::default());
        let next = Arc::new(ReaderPool::default());
        log.bind_launch_generation(&current);
        log.append(b"seen");
        log.collect_first_exit(4, None, 19);
        current.close_all();
        assert!(!log.launched_in(&next));
        log.append(late);
        log.close(false);
        log.remember_sealed_first_exit();
        assert_eq!(log.first_exit_uncollected(), !late.is_empty());
        assert!(log.capture.lock().unwrap().reader_pool.is_none());
        if !late.is_empty() {
            let (name, partial, _) = log.stage_diagnostic()?.expect("sealed genuine prefix");
            assert!(partial);
            let store =
                crate::alab_shell_log_store::existing_directory(home.path(), thread)?.unwrap();
            assert_eq!(fs::read(store.path().join(name))?, b"seen\xffstderr");
            log.finish_diagnostic(true);
        }
    }
    crate::alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn first_terminal_reader_shares_writable_capacity_and_keeps_its_original_pipe()
-> anyhow::Result<()> {
    use super::super::MAX_UNIFIED_EXEC_PROCESSES;
    use super::super::UnifiedExecProcess;
    use super::super::process::retired_readers::ReaderPool;
    use std::io::Write;
    use std::time::Duration;

    let home = tempfile::tempdir()?;
    let gate = home.path().join("late-input");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&gate)
            .status()?
            .success()
    );
    let thread = codex_protocol::ThreadId::new();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let wait = Duration::from_secs(60);
    for active in [MAX_UNIFIED_EXEC_PROCESSES - 1, MAX_UNIFIED_EXEC_PROCESSES] {
        let mut log = ShellLog::new(Some(Arc::clone(&directory)));
        let pool = Arc::new(ReaderPool::default());
        log.bind_launch_generation(&pool);
        let started = codex_utils_pty::spawn_pipe_process(
            "/bin/sh",
            &[
                "-c".into(),
                r#"exec 3<> "$1"; (IFS= read -r gate <&3; printf late) & exit 17"#.into(),
                "sh".into(),
                gate.to_string_lossy().into_owned(),
            ],
            home.path(),
            &Default::default(),
            &None,
            &[],
        )
        .await?;
        let process = Arc::new(
            UnifiedExecProcess::from_spawned(
                started,
                codex_sandboxing::SandboxType::None,
                Some(log.clone()),
            )
            .await?,
        );
        tokio::time::timeout(wait, process.cancellation_token().cancelled()).await?;
        let exit = process.exit_code().expect("original native exit");
        assert_eq!(exit, 17);
        log.collect_first_exit(0, None, exit);
        let evicted = pool.keep(&process, Some(&log), Some(exit), active);
        log.remember_first_admission();
        drop(process);
        if active == MAX_UNIFIED_EXEC_PROCESSES {
            tokio::time::timeout(wait, evicted.expect("saturated store freezes new reader"))
                .await??;
            assert!(!log.first_exit_uncollected());
            assert!(!log.diagnostic_ready());
        } else {
            assert!(evicted.is_none());
            assert!(log.first_exit_uncollected());
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&gate)?
                .write_all(b"release\n")?;
            tokio::time::timeout(wait, log.wait_for_local_drain()).await?;
            let (name, partial, _) = log.stage_diagnostic()?.expect("original pipe drained");
            assert!(!partial);
            let store =
                crate::alab_shell_log_store::existing_directory(home.path(), thread)?.unwrap();
            assert_eq!(fs::read(store.path().join(name))?, b"late");
            log.finish_diagnostic(true);
            log.release_drained_reader(false);
            assert!(
                log.first_exit_uncollected(),
                "receipt survives native release"
            );
        }
        tokio::time::timeout(wait, pool.close_all().wait()).await?;
        assert!(pool.is_closed());
    }
    crate::alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn canceled_first_result_closes_the_original_server_reader_before_registration()
-> anyhow::Result<()> {
    use super::super::UnifiedExecProcess;
    use super::super::process::OutputDrainPolicy;
    use super::super::process::retired_readers::ReaderPool;
    use codex_exec_server::Environment;
    use codex_exec_server::ExecParams;
    use codex_exec_server::ExecProcessEvent;
    use codex_exec_server::LocalOutput;
    use codex_utils_path_uri::PathUri;
    use std::io::Write;
    use std::time::Duration;

    for (during_startup, exit) in [(true, 29), (true, 0), (false, 29)] {
        let home = tempfile::tempdir()?;
        let path = home.path().join("keep-original-stderr");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&path)
                .status()?
                .success()
        );
        // Hold the helper until we have witnessed the actual reader's retirement.
        let mut release = fs::OpenOptions::new().read(true).write(true).open(&path)?;
        let thread = codex_protocol::ThreadId::new();
        let directory = ThreadLogs::default()
            .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
        let mut log = ShellLog::new(Some(directory));
        let readers = Arc::new(ReaderPool::default());
        log.bind_launch_generation(&readers);
        let observed = log.clone();
        let environment = Environment::default_for_tests();
        let started = environment
            .get_exec_backend()
            .start_with_local_capture_observer(
                ExecParams {
                    process_id: "first-result-cancel".to_string().into(),
                    argv: vec![
                        "/bin/sh".into(),
                        "-c".into(),
                        r#"exec 3< "$1"; (IFS= read -r gate <&3; printf late >&2) & exit "$2""#
                            .into(),
                        "sh".into(),
                        path.to_string_lossy().into_owned(),
                        exit.to_string(),
                    ],
                    cwd: PathUri::from_host_native_path(home.path())?,
                    metadata: None,
                    shell_snapshot: None,
                    env_policy: None,
                    env: Default::default(),
                    tty: false,
                    pipe_stdin: false,
                    arg0: None,
                    sandbox: None,
                    enforce_managed_network: false,
                    managed_network: None,
                    network_proxy: None,
                },
                Arc::new(move |event| match event {
                    LocalOutput::Bytes(bytes) => observed.append(bytes),
                    LocalOutput::Closed { native_eof } => observed.close_raw(native_eof),
                }),
            )
            .await?;
        let mut events = started.process.subscribe_events();
        let mut startup = Box::pin(UnifiedExecProcess::from_exec_server_started(
            started,
            OutputDrainPolicy::BoundedAfterExit,
            Some(log.clone()),
        ));
        let wait = Duration::from_secs(60);
        assert!(futures::poll!(startup.as_mut()).is_pending());
        tokio::time::timeout(wait, async {
            loop {
                if let ExecProcessEvent::Exited { exit_code, .. } = events.recv().await? {
                    assert_eq!(exit_code, exit);
                    return Ok::<_, anyhow::Error>(());
                }
            }
        })
        .await??;
        assert!(!log.raw_closed());
        if during_startup {
            let shutdown = readers.close_all();
            drop(startup);
            tokio::time::timeout(wait, shutdown.wait()).await?;
        } else {
            let process = Arc::new(startup.await?);
            let (entered, waiting) = tokio::sync::oneshot::channel();
            let handoff = tokio::spawn(async move {
                process.protect_first_handoff();
                let _ = entered.send(());
                // Suspend at admission so cancellation drops the original process here.
                std::future::pending::<()>().await;
                drop(process);
            });
            tokio::time::timeout(wait, waiting).await??;
            let shutdown = readers.close_all();
            handoff.abort();
            assert!(handoff.await.unwrap_err().is_cancelled());
            tokio::time::timeout(wait, shutdown.wait()).await?;
        }
        assert_eq!(log.raw_closed(), exit != 0);
        if exit == 0 {
            release.write_all(b"finish\n")?;
        }
        tokio::time::timeout(wait, async {
            while !matches!(events.recv().await?, ExecProcessEvent::Closed { .. }) {}
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        if exit != 0 {
            release.write_all(b"finish\n")?;
        } else {
            let capture = log.capture.lock().unwrap();
            assert!(capture.raw_eof);
            assert_eq!(capture.prefix.as_slice(), b"late");
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_terminal_disclosure_covers_its_original_bytes_even_if_replay_dropped_them() -> io::Result<()> {
    use super::super::process::retired_readers::ReaderPool;

    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let pool = Arc::new(ReaderPool::default());
    for complete in [false, true] {
        let log = ShellLog::new(Some(Arc::clone(&directory)));
        log.initial_response(0);
        log.append(b"server raw original");
        assert!(log.retain_reader(Arc::downgrade(&pool)));
        log.close(complete);
        let notice = log.notice(true, false).unwrap();
        assert_eq!(notice.disclosed.map(|disclosed| disclosed.bytes), Some(19));
        assert_eq!(notice.status, (!complete).then_some("partial"));
        assert!(!log.collect_poll(3, true, notice.disclosed));
        assert!(log.stage_diagnostic()?.is_none());
    }

    // Both orderings preserve the final tool's exact partial receipt, even
    // when the original reader closes in the interval before the poll records it.
    for closes_before_record in [false, true] {
        let log = ShellLog::new(Some(Arc::clone(&directory)));
        log.append(b"prefix");
        log.initial_response(6);
        assert!(log.retain_reader(Arc::downgrade(&pool)));
        let mut result_log = log.clone();
        let notice = result_log.notice(true, false).unwrap();
        assert_eq!(notice.status, Some("partial"));
        let disclosed = notice.disclosed;
        result_log.freeze_result_notice(Some(notice));
        if closes_before_record {
            log.close(true);
        }
        assert!(log.collect_poll(0, true, disclosed));
        log.close(true);
        let delivered = result_log.notice(true, false).unwrap();
        assert_eq!(delivered.status, Some("partial"));
        let (name, partial, _) = log
            .stage_diagnostic()?
            .expect("same-length capture proved complete");
        assert!(!partial);
        let path = delivered.path.unwrap();
        assert_eq!(path.file_name().unwrap().to_str(), Some(name.as_str()));
        assert_eq!(fs::read(path)?, b"prefix");
        log.finish_diagnostic(true);
        assert!(log.stage_diagnostic()?.is_none());
    }

    let capped = ShellLog::new(Some(Arc::clone(&directory)));
    capped.initial_response(0);
    capped.append(&vec![0x80; super::MAX_LOG_BYTES + 1]);
    assert!(capped.retain_reader(Arc::downgrade(&pool)));
    let notice = capped.notice(true, false).unwrap();
    assert_eq!(
        notice.disclosed.map(|disclosed| disclosed.bytes),
        Some(super::MAX_LOG_BYTES)
    );
    assert_eq!(notice.status, Some("partial"));
    assert!(capped.collect_poll(16, true, notice.disclosed));
    capped.append(b"discarded after cap");
    capped.close(true);
    assert!(capped.stage_diagnostic()?.is_none());

    let growing = ShellLog::new(Some(directory));
    growing.initial_response(0);
    growing.append(b"prefix");
    assert!(growing.retain_reader(Arc::downgrade(&pool)));
    let mut result_log = growing.clone();
    let notice = result_log.notice(true, false).unwrap();
    assert!(growing.collect_poll(3, true, notice.disclosed));
    result_log.freeze_result_notice(Some(notice));
    growing.append(b" + tail");
    growing.close(true);
    let delivered = result_log.notice(true, false).unwrap();
    assert_eq!(delivered.status, Some("partial"));
    assert_eq!(
        delivered.disclosed.map(|disclosed| disclosed.bytes),
        Some(6)
    );
    let (name, partial, _) = growing
        .stage_diagnostic()?
        .expect("file gained actual bytes");
    assert!(!partial);
    let path = delivered.path.unwrap();
    assert_eq!(path.file_name().unwrap().to_str(), Some(name.as_str()));
    assert_eq!(fs::read(path)?, b"prefix + tail");
    growing.finish_diagnostic(true);
    crate::alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn oversized_failure_keeps_its_contiguous_prefix_and_the_exact_nonoverlapping_eof_tail()
-> io::Result<()> {
    use super::super::process::retired_readers::ReaderPool;
    use super::super::shell_observation::observe_local_exit;
    use codex_alab_core::MAX_SHELL_OUTPUT_TAIL_BYTES;

    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let log = ShellLog::new(Some(directory));
    log.initial_response(0);
    let prefix = vec![b'x'; super::MAX_LOG_BYTES];
    log.append(&prefix[..prefix.len() - 2]);
    log.append(b"xxinitial suffix");
    let pool = Arc::new(ReaderPool::default());
    assert!(log.retain_reader(Arc::downgrade(&pool)));
    let mut poll = log.clone();
    poll.result_can_disclose_tail = true;
    let before_eof = poll.notice(true, false).unwrap();
    assert!(before_eof.tail.is_none());
    assert!(log.collect_poll(prefix.len() + 14, true, before_eof.disclosed));

    let mut beyond = b"initial suffix".to_vec();
    let wide = vec![0xa1; MAX_SHELL_OUTPUT_TAIL_BYTES + 21];
    beyond.extend_from_slice(&wide);
    log.append(&wide);
    for byte in (0u8..=255).cycle().take(MAX_SHELL_OUTPUT_TAIL_BYTES + 13) {
        log.append(&[byte]);
        beyond.push(byte);
    }
    let expected = &beyond[beyond.len() - MAX_SHELL_OUTPUT_TAIL_BYTES..];
    observe_local_exit(Some(&log), Some(17)).await;
    log.close_capture(true, true, None);
    let (name, partial, tail) = log.stage_diagnostic()?.expect("watcher-refined coverage");
    let tail = tail.expect("verified nonzero EOF suffix");
    assert!(partial);
    assert_eq!(tail.end_byte(), (prefix.len() + beyond.len()) as u64);
    assert_eq!(tail.start_byte(), tail.end_byte() - expected.len() as u64);
    assert_eq!(tail.bytes(), expected);
    let store = crate::alab_shell_log_store::existing_directory(home.path(), thread)?.unwrap();
    let path = store.path().join(name);
    assert_eq!(fs::read(&path)?, prefix);
    let completed = poll.notice(false, false).unwrap();
    assert_eq!(completed.tail.as_ref(), Some(&tail));
    assert!(!log.collect_poll(0, true, completed.disclosed));
    assert!(log.stage_diagnostic()?.is_none());

    fs::OpenOptions::new().write(true).open(&path)?.set_len(1)?;
    assert!(poll.notice(true, false).unwrap().tail.is_none());
    drop(log);
    store.retire()?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn oversized_unproven_streams_and_unsuccessful_waits_disclose_no_tail() -> io::Result<()> {
    use super::super::shell_observation::observe_local_exit;

    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let prefix = vec![b'x'; super::MAX_LOG_BYTES];
    for (code, native_eof, eligible) in [
        (None, true, false),
        (Some(-1), true, false),
        (Some(0), true, false),
        (Some(3), false, false),
        (Some(3), true, true),
    ] {
        let log = ShellLog::new(Some(Arc::clone(&directory)));
        log.initial_response(0);
        log.append(&prefix);
        log.append(b"only suffix");
        observe_local_exit(Some(&log), code).await;
        log.close_capture(native_eof, native_eof, None);
        let (_, _, tail) = log.stage_diagnostic()?.expect("retained prefix");
        assert_eq!(tail.is_some(), eligible);
        if let Some(tail) = tail {
            assert_eq!(tail.start_byte(), prefix.len() as u64);
            assert_eq!(tail.bytes(), b"only suffix");
        }
        log.finish_diagnostic(false);
    }
    let temporary = ThreadLogs::default().directory_with(None, tempfile::tempdir)?;
    let temp = ShellLog::new(Some(temporary));
    temp.append(&prefix);
    temp.append(b"unowned");
    temp.close_capture(true, true, None);
    assert!(temp.capture.lock().unwrap().suffix.is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_partial_diagnostic_rolls_back_only_its_undisclosed_file() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let logs = ThreadLogs::default();
    let directory = logs.directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    for disclosed in [false, true] {
        let log = ShellLog::new(Some(Arc::clone(&directory)));
        log.initial_response(0);
        log.append(b"diagnostic prefix");
        log.close(false);
        let existing = disclosed
            .then(|| log.notice(true, false).and_then(|notice| notice.path))
            .flatten();
        let prepared = log.prepare_diagnostic()?.expect("captured prefix");
        assert!(prepared.partial);
        assert!(prepared.publish()?);
        let store = crate::alab_shell_log_store::existing_directory(home.path(), thread)?.unwrap();
        let path = store.path().join(&prepared.name);
        if disclosed {
            assert_eq!(existing.as_ref(), Some(&path));
        }
        prepared.cleanup(false)?;
        drop(prepared);
        log.finish_diagnostic(false);
        drop(log);
        assert_eq!(path.exists(), disclosed);
        if disclosed {
            store.retire()?;
        } else {
            assert!(
                crate::alab_shell_log_store::existing_directory(home.path(), thread)?.is_none()
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn retained_output_starts_at_disclosure_and_survives_manager_unload() -> io::Result<()> {
    use crate::alab_shell_log_store;
    use codex_protocol::ThreadId;

    let home = tempfile::tempdir()?;
    let thread = ThreadId::default();
    let logs = ThreadLogs::default();
    let directory =
        logs.directory_with(Some((home.path().to_owned(), thread)), tempfile::tempdir)?;
    let mut bytes = vec![0xff; MEMORY_PREFIX_BYTES + 19];
    let discarded = ShellLog::new(Some(Arc::clone(&directory)));
    discarded.append(&bytes);
    assert!(discarded.notice(false, true).is_none());
    drop(discarded);
    assert!(alab_shell_log_store::existing_directory(home.path(), thread)?.is_none());

    let command = ShellLog::new(Some(Arc::clone(&directory)));
    command.append(&bytes);
    let running = command.notice(true, true).expect("clipped output");
    assert_eq!(running.status, Some("in_progress"));
    let path = running.path.expect("retained path");
    assert_ne!(path.parent(), Some(directory.directory.path()));
    assert_eq!(fs::read(&path)?, bytes);
    command.append(b"\x00last");
    bytes.extend_from_slice(b"\x00last");
    command.close(false);
    let terminal = command.notice(false, false).expect("disclosed output");
    assert_eq!(terminal.path.as_ref(), Some(&path));
    assert_eq!(terminal.status, Some("partial"));
    drop(command);
    drop(directory);
    drop(logs);
    assert_eq!(fs::read(&path)?, bytes);
    alab_shell_log_store::delete(home.path(), thread)?;
    assert!(!path.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn resumed_manager_charges_retained_logs_and_retries_the_same_command() -> io::Result<()> {
    use crate::alab_shell_log_store;
    use codex_protocol::ThreadId;

    let home = tempfile::tempdir()?;
    let thread = ThreadId::default();
    let owner = Some((home.path().to_owned(), thread));
    let mut paths = Vec::new();
    {
        let directory = ThreadLogs::default().directory_with(owner.clone(), tempfile::tempdir)?;
        for _ in 0..MAX_THREAD_LOGS {
            let log = ShellLog::new(Some(Arc::clone(&directory)));
            log.append(b"completed");
            log.close(true);
            paths.push(log.notice(true, false).unwrap().path.unwrap());
        }
    }
    let directory = ThreadLogs::default().directory_with(owner, tempfile::tempdir)?;
    let log = ShellLog::new(Some(directory));
    log.append(b"retained prefix");
    assert_eq!(log.notice(true, true).unwrap().status, Some("unavailable"));
    fs::remove_file(&paths[0])?;
    let path = log
        .notice(false, true)
        .unwrap()
        .path
        .expect("freed retained slot");
    assert_eq!(fs::read(path)?, b"retained prefix");
    let retained = alab_shell_log_store::directory(home.path(), thread)?;
    assert_eq!(retained.count(MAX_THREAD_LOGS + 1)?, MAX_THREAD_LOGS);
    drop(log);
    alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn replaced_retained_name_never_reports_another_files_bytes() -> io::Result<()> {
    use crate::alab_shell_log_store;
    use codex_protocol::ThreadId;

    let home = tempfile::tempdir()?;
    let thread = ThreadId::default();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().to_owned(), thread)), tempfile::tempdir)?;
    let log = ShellLog::new(Some(directory));
    log.append(b"original");
    let path = log.notice(true, true).unwrap().path.unwrap();
    let moved = path.with_extension("moved");
    fs::rename(&path, &moved)?;
    fs::write(&path, b"replacement")?;
    log.append(b" tail");
    log.close(true);
    let final_notice = log.notice(false, false).expect("previously disclosed");
    assert_eq!(final_notice.status, Some("unavailable"));
    assert!(final_notice.path.is_none());
    drop(log);
    assert_eq!(fs::read(&moved)?, b"original tail");
    // Deletion reopens the owner after the live descriptor has gone.
    alab_shell_log_store::delete(home.path(), thread)?;
    assert_eq!(fs::read(&path)?, b"replacement");
    assert_eq!(fs::read(&moved)?, b"original tail");
    fs::remove_file(&moved)?;
    fs::remove_file(&path)?;
    alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn incomplete_publication_leaves_no_retained_file_and_can_retry() -> io::Result<()> {
    use crate::alab_shell_log_store;
    use codex_protocol::ThreadId;

    let home = tempfile::tempdir()?;
    let thread = ThreadId::default();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().to_owned(), thread)), tempfile::tempdir)?;
    let output = ShellLog::new(Some(Arc::clone(&directory)));
    let bytes = vec![0x82; MEMORY_PREFIX_BYTES + 1];
    output.append(&bytes);
    let staging = directory
        .directory
        .path()
        .read_dir()?
        .next()
        .unwrap()?
        .path();
    fs::OpenOptions::new()
        .write(true)
        .open(&staging)?
        .set_len(4)?;
    let refused = output.notice(true, true).unwrap();
    assert_eq!(refused.status, Some("unavailable"));
    assert!(refused.path.is_none());
    let retained = alab_shell_log_store::directory(home.path(), thread)?;
    assert_eq!(retained.count(MAX_THREAD_LOGS)?, 0);

    fs::write(&staging, &bytes)?;
    output.append(b" tail");
    output.close(true);
    let retried = output.notice(false, false).unwrap();
    assert_eq!(retried.status, None);
    let mut expected = bytes;
    expected.extend_from_slice(b" tail");
    assert_eq!(fs::read(retried.path.unwrap())?, expected);
    drop(output);
    alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn short_write_before_failure_publishes_every_accepted_byte_as_partial() -> io::Result<()> {
    use crate::alab_shell_log_store;
    use codex_protocol::ThreadId;
    use std::io::Write;

    struct InterruptedThenShort<'a> {
        file: &'a mut fs::File,
        calls: usize,
        ends_with_zero: bool,
    }
    impl Write for InterruptedThenShort<'_> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            match self.calls {
                1 => Err(io::ErrorKind::Interrupted.into()),
                2 => self.file.write_all(&buf[..2]).map(|()| 2),
                _ if self.ends_with_zero => Ok(0),
                _ => Err(io::Error::other("staging write failed")),
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }

    let home = tempfile::tempdir()?;
    let thread = ThreadId::default();
    let directory = ThreadLogs::default()
        .directory_with(Some((home.path().to_owned(), thread)), tempfile::tempdir)?;
    for ends_with_zero in [false, true] {
        let log = ShellLog::new(Some(Arc::clone(&directory)));
        {
            let mut capture = log.capture.lock().unwrap();
            assert!(capture.try_spill_prefix());
            let super::Capture {
                file,
                bytes,
                incomplete,
                ..
            } = &mut *capture;
            let mut writer = InterruptedThenShort {
                file: file.as_mut().unwrap().writer(),
                calls: 0,
                ends_with_zero,
            };
            super::write_captured_bytes(&mut writer, b"\0\xffmissing", bytes, incomplete);
            assert_eq!(*bytes, 2);
            assert!(*incomplete);
        }
        log.append(b"bytes after the gap");
        log.close(true);
        let notice = log.notice(true, false).expect("clipped output");
        assert_eq!(notice.status, Some("partial"));
        let path = notice.path.expect("durable prefix");
        drop(log);
        assert_eq!(fs::read(path)?, b"\0\xff");
    }
    alab_shell_log_store::delete(home.path(), thread)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_writer_recreates_its_original_prefix_after_an_unreserved_stage_is_reclaimed() -> io::Result<()>
{
    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let raw = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let log = ShellLog::new(Some(raw));
    log.initial_response(0);
    log.append(b"late\x00failure");
    log.close(true);
    let first = log.prepare_diagnostic()?.unwrap();
    let previous = first.name.clone();
    drop(first);
    let cleaner = crate::alab_shell_log_store::directory(home.path(), thread)?
        .try_publication_lock()?
        .unwrap();
    cleaner.clean_orphan_stages(&[])?;
    drop(cleaner);
    let next = log.prepare_diagnostic()?.unwrap();
    assert_ne!(next.name, previous);
    assert!(next.publish()?);
    let dir = crate::alab_shell_log_store::directory(home.path(), thread)?;
    assert_eq!(fs::read(dir.path().join(&next.name))?, b"late\x00failure");
    next.cleanup(true)?;
    drop(next);
    log.finish_diagnostic(true);
    dir.retire()
}

#[cfg(unix)]
#[test]
fn a_disclosed_file_edited_before_prepare_is_rejected_against_native_bytes() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let raw = ThreadLogs::default()
        .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
    let log = ShellLog::new(Some(raw));
    log.append(b"early");
    log.initial_response(5);
    let path = log.notice(true, true).unwrap().path.unwrap();
    log.append(b"late");
    log.close(true);
    fs::write(&path, b"altered!!")?;
    let prepared = log.prepare_diagnostic()?.unwrap();
    assert_eq!(
        path.file_name().unwrap().to_str(),
        Some(prepared.name.as_str())
    );
    assert!(!prepared.publish()?);
    prepared.cleanup(false)?;
    assert_eq!(fs::read(&path)?, b"altered!!");
    drop(prepared);
    crate::alab_shell_log_store::delete(home.path(), thread)
}

#[cfg(unix)]
#[test]
fn a_final_poll_uses_the_watchers_original_file_across_sqlite_receipt_orderings() -> io::Result<()>
{
    let home = tempfile::tempdir()?;
    for watcher_finished in [false, true] {
        let thread = codex_protocol::ThreadId::new();
        let raw = ThreadLogs::default()
            .directory_with(Some((home.path().into(), thread)), tempfile::tempdir)?;
        let log = ShellLog::new(Some(raw));
        log.initial_response(0);
        log.append(b"original failure");
        log.close(true);
        let prepared = log.prepare_diagnostic()?.unwrap();
        let name = prepared.name.clone();
        if watcher_finished {
            assert!(prepared.publish()?);
            prepared.cleanup(true)?;
            drop(prepared);
            log.finish_diagnostic(true);
        } else {
            let path = log.notice(true, false).unwrap().path.unwrap();
            assert_eq!(path.file_name().unwrap().to_str(), Some(name.as_str()));
            assert!(prepared.publish()?);
            prepared.cleanup(false)?;
            drop(prepared);
            log.finish_diagnostic(false);
        }
        let dir = crate::alab_shell_log_store::directory(home.path(), thread)?;
        let path = log.notice(true, false).unwrap().path.unwrap();
        assert_eq!(path, dir.path().join(&name));
        assert_eq!(log.retained_basename().as_ref(), Some(&name));
        assert_eq!(fs::read(path)?, b"original failure");
        assert_eq!(dir.count(100)?, 1);
        drop(log);
        dir.retire()?;
    }
    Ok(())
}
