// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// Actual controlling PTY/original Child only; no Native/Entry/Go fabricated.
use super::*;
use std::io::Write;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;
fn current() -> io::Result<libc::termios> {
    let mut value = MaybeUninit::uninit();
    // SAFETY: private helper owns its controlling stdin and writable out-value.
    if unsafe { libc::tcgetattr(0, value.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { value.assume_init() })
}
fn observed() -> io::Result<serde_json::Value> {
    serde_json::to_value(Mode::capture(&current()?)).map_err(io::Error::other)
}
fn error(message: &'static str) -> io::Error {
    io::Error::other(message)
}
async fn helper() -> io::Result<()> {
    codex_alab_core::activate();
    let before = observed()?;
    super::super::enable_raw_mode()?;
    let root = tempfile::tempdir()?;
    // Keep a real second subscription alive. Pausing only the Tui's event
    // source would not stop this owner; explicit sticky prepare must join it.
    let surviving_input = crate::input::InputStream::default();
    let cancelled_marker = root.path().join("cancelled-editor-must-not-start");
    let stale_source = Arc::new(Mutex::new(super::prepare_upgrade()?));
    // A real same-TTY mutation leaves the revocable token valid but makes its
    // saved running snapshot stale. Pre-child mode admission must check both.
    let parent_running = current()?;
    let mut changed_running = parent_running;
    changed_running.c_lflag |= libc::ICANON;
    // SAFETY: isolated helper's actual controlling stdin, initialized termios.
    if unsafe { libc::tcsetattr(0, libc::TCSANOW, &changed_running) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if !stale_source
        .lock()
        .map_err(|_| error("test mode source poisoned"))?
        .transfer
        .valid()
    {
        return Err(error(
            "direct real TTY mutation unexpectedly revoked admission token",
        ));
    }
    let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .args(["-c", "touch \"$1\"", "editor"])
        .arg(&cancelled_marker);
    let cancelled_operation = Operation::prepare(command, document)?;
    let partial = {
        let before = cancelled_operation.before_start()?;
        let refused = match crate::input::OriginalEditorResources::prepare(
            &cancelled_operation,
            &before,
            &stale_source,
        ) {
            Ok(_) => return Err(error("stale actual running modes were accepted")),
            Err(refused) => refused,
        };
        let mut partial = refused.actual;
        if partial.input().is_none() || partial.modes().is_some() {
            return Err(error("mode refusal lost its actual partial parser owner"));
        }
        let cancelled = before.cancel()?;
        // Exercise the actual production partial-mode rollback, not a raw
        // test-only setter or crossterm's cached enable. A genuine receipt from
        // another no-child operation must leave the current changed modes alone.
        let foreign_document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
        let foreign =
            Operation::prepare(tokio::process::Command::new("/bin/false"), foreign_document)?;
        let foreign_cancelled = foreign.cancel_before_start()?;
        let changed_before_foreign = observed()?;
        if super::restore_cancelled_original_editor(
            &cancelled_operation,
            &foreign_cancelled,
            partial
                .input()
                .ok_or_else(|| error("partial parser absent"))?,
            &stale_source,
        )
        .is_ok()
            || observed()? != changed_before_foreign
        {
            return Err(error(
                "foreign no-child receipt restored partial parent modes",
            ));
        }
        super::restore_cancelled_original_editor(
            &cancelled_operation,
            &cancelled,
            partial
                .input()
                .ok_or_else(|| error("partial parser absent"))?,
            &stale_source,
        )?;
        if current()?.c_lflag & libc::ICANON != 0 {
            return Err(error(
                "production partial-mode handback did not restore actual raw modes",
            ));
        }
        partial
            .input_mut()
            .ok_or_else(|| error("partial parser absent"))?
            .finish_after_cancelled_handoff(&cancelled)?;
        partial
    };
    if cancelled_operation.wait().await.is_ok() || cancelled_marker.exists() {
        return Err(error(
            "partial preparation cancellation started a real child",
        ));
    }
    drop(partial);
    drop(stale_source);
    // A full pre-child bundle can also hand back without inventing Finished.
    let source = Arc::new(Mutex::new(super::prepare_upgrade()?));
    let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .args(["-c", "touch \"$1\"", "editor"])
        .arg(&cancelled_marker);
    let cancelled_operation = Operation::prepare(command, document)?;
    let full = {
        let before = cancelled_operation.before_start()?;
        let mut full =
            crate::input::OriginalEditorResources::prepare(&cancelled_operation, &before, &source)
                .map_err(|refused| refused.error)?;
        let cancelled = before.cancel()?;
        full.modes()
            .ok_or_else(|| error("pre-child modes absent"))?
            .release_for_cancelled_handback(&cancelled)?;
        full.input_mut()
            .ok_or_else(|| error("pre-child parser absent"))?
            .finish_after_cancelled_handoff(&cancelled)?;
        full
    };
    if cancelled_operation.wait().await.is_ok() || cancelled_marker.exists() {
        return Err(error("full preparation cancellation started a real child"));
    }
    // Only genuine exact handback permits these actual wrappers to retire.
    drop(full);
    drop(source);
    // Actual writer resource holds the SAME mode-installation mutex through
    // real descriptor mutation. No timing/scheduler assumption or new harness.
    let output = tempfile::tempfile()?;
    // SAFETY: duplicate actual private stderr above the standard descriptors.
    let saved = unsafe { libc::fcntl(2, libc::F_DUPFD_CLOEXEC, 3) };
    if saved < 0 {
        return Err(io::Error::last_os_error());
    }
    let saved = unsafe { std::os::fd::OwnedFd::from_raw_fd(saved) };
    let writer = super::parent_stdio_writer()?;
    let held_before = matches!(
        ORIGINAL.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    );
    // SAFETY: this isolated helper owns both descriptions. Restore under SAME
    // writer guard before reporting a failed assertion or starting any editor.
    let mutation = if unsafe { libc::dup2(output.as_raw_fd(), 2) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    };
    let held_during = matches!(
        ORIGINAL.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    );
    let restored = if unsafe { libc::dup2(saved.as_raw_fd(), 2) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    };
    drop(writer);
    let released = ORIGINAL.try_lock().is_ok();
    mutation?;
    restored?;
    if !held_before || !held_during || !released {
        return Err(error(
            "parent writer did not retain actual mode lock through FD mutation",
        ));
    }
    let source = Arc::new(Mutex::new(super::prepare_upgrade()?));
    let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
    let path = document.to_path_buf();
    let ready = root.path().join("actual-editor-ready");
    let (reader, mut writer) = std::os::unix::net::UnixStream::pair()?;
    let reader: std::os::fd::OwnedFd = reader.into();
    let mut command = tokio::process::Command::new("/bin/sh");
    command.args(["-c","stty sane </dev/tty && printf ready > \"$2\"; IFS= read -r release; printf 'real editor result\\n' > \"$1\"","editor"])
        .arg(&path).arg(&ready).stdin(Stdio::from(reader)).stdout(Stdio::null()).stderr(Stdio::null());
    let operation = Operation::prepare(command, document)?;
    let mut resources = {
        let before = operation.before_start()?;
        let resources =
            crate::input::OriginalEditorResources::prepare(&operation, &before, &source)
                .map_err(|refused| refused.error)?;
        if ready.exists() || !super::editor_owned() {
            return Err(error(
                "pre-child actual bundle was not retained before start",
            ));
        }
        // This exact resource slot is installed before the SAME guard drops.
        resources
    };
    let observation = tokio::spawn({
        let operation = operation.clone();
        async move { operation.wait().await }
    });
    tokio::time::timeout(Duration::from_secs(60), async {
        while !ready.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(io::Error::other)?;
    if current()?.c_lflag & libc::ICANON == 0 {
        return Err(error("real child did not change its controlling TTY"));
    }
    // A genuine accepted spawn joins before moving ANY inherited parent IO.
    // C0 staging moves the same actual originals without mutating child modes.
    let capture = operation.capture_c0()?;
    let spawn = tokio::time::timeout(Duration::from_secs(60), capture.spawn_completed())
        .await
        .map_err(io::Error::other)??;
    let (canonical_fd, mode_fd) = {
        let state = ORIGINAL
            .lock()
            .map_err(|_| error("test original mode owner poisoned"))?;
        let canonical = state
            .original
            .as_ref()
            .ok_or_else(|| error("actual canonical File absent"))?
            .terminal
            .as_raw_fd();
        let modes = resources
            .modes()
            .ok_or_else(|| error("actual pre-child modes absent"))?;
        let terminal = modes
            .actual
            .terminal
            .lock()
            .map_err(|_| error("test mode File poisoned"))?;
        (
            canonical,
            terminal
                .as_ref()
                .ok_or_else(|| error("actual mode File absent"))?
                .as_raw_fd(),
        )
    };
    let before_stage = observed()?;
    let stage = resources
        .stage_c0(&capture, &spawn)
        .map_err(|refused| refused.error)?;
    let source_absent = {
        let state = ORIGINAL
            .lock()
            .map_err(|_| error("test original mode owner poisoned"))?;
        state.original.is_none() && state.editor.is_some()
    };
    // No duplicate stands in for retirement. The same actual numeric Files
    // remain open, pinned by native original owners, while local slots are empty.
    let originals_open = unsafe { libc::fcntl(canonical_fd, libc::F_GETFD) } >= 0
        && unsafe { libc::fcntl(mode_fd, libc::F_GETFD) } >= 0;
    let blocked_while_staged =
        super::parent_stdio_writer().is_err() && super::super::enable_raw_mode().is_err();
    let while_staged = observed()?;
    resources = stage
        .rollback_before_offer()
        .map_err(|refused| refused.error)?;
    let same_files = {
        let state = ORIGINAL
            .lock()
            .map_err(|_| error("test original mode owner poisoned"))?;
        let canonical = state
            .original
            .as_ref()
            .ok_or_else(|| error("returned canonical File absent"))?
            .terminal
            .as_raw_fd();
        let modes = resources
            .modes()
            .ok_or_else(|| error("returned mode owner absent"))?;
        let terminal = modes
            .actual
            .terminal
            .lock()
            .map_err(|_| error("test returned mode File poisoned"))?;
        canonical == canonical_fd
            && terminal
                .as_ref()
                .ok_or_else(|| error("returned mode File absent"))?
                .as_raw_fd()
                == mode_fd
    };
    if !source_absent
        || !originals_open
        || !blocked_while_staged
        || !same_files
        || while_staged != before_stage
    {
        return Err(error(
            "C0 staging changed, dropped or reopened original mode ownership",
        ));
    }
    let fence = operation.freeze_before_h1().await?;
    let modes = resources
        .modes()
        .ok_or_else(|| error("retained actual modes absent"))?;
    let input = resources
        .input()
        .ok_or_else(|| error("retained actual parser absent"))?;
    let snapshot = modes.snapshot_for_exec(&fence, input.fence())?;
    if serde_json::to_value(&snapshot).map_err(io::Error::other)?["original"] != before {
        return Err(error(
            "mode resource replaced real original canonical modes",
        ));
    }
    if snapshot.running == Mode::capture(&current()?) {
        return Err(error(
            "live editor modes incorrectly equal old parent raw modes",
        ));
    }
    let child_modes = observed()?;
    if !super::editor_owned() || !super::super::preserve_on_exit() {
        return Err(error("actual editor restriction was absent"));
    }
    let parent_writer_denied = matches!(super::parent_stdio_writer(),Err(error) if error.kind()==io::ErrorKind::WouldBlock);
    if super::super::enable_raw_mode().is_ok() || super::super::disable_raw_mode().is_ok() {
        return Err(error("generic mode setter escaped actual editor owner"));
    }
    struct OnPanic(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for OnPanic {
        fn drop(&mut self) {
            self.0.store(
                super::super::disable_raw_mode().is_err() && super::super::preserve_on_exit(),
                Ordering::Release,
            );
        }
    }
    let denied = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let _ = std::panic::catch_unwind({
        let denied = denied.clone();
        move || {
            let _probe = OnPanic(denied);
            panic!("isolated actual panic mode control");
        }
    });
    std::panic::set_hook(hook);
    if !denied.load(Ordering::Acquire) || observed()? != child_modes {
        return Err(error("panic fallback changed editor modes"));
    }
    // This writer is the SAME original stdin description supplied before
    // Operation::prepare. No Child/PID constructor or replacement input is used.
    writer.write_all(b"original release\n")?;
    drop(writer);
    drop(fence);
    let finished = tokio::time::timeout(Duration::from_secs(60), observation)
        .await
        .map_err(io::Error::other)?
        .map_err(io::Error::other)??;
    if finished
        .outcome()
        .map_err(|error| io::Error::other(error.to_string()))?
        != "real editor result\n"
    {
        return Err(error("original child/read outcome changed"));
    }
    modes.release_for_original_handback(&finished)?;
    if observed()? != serde_json::to_value(&snapshot).map_err(io::Error::other)?["running"] {
        return Err(error(
            "actual finished child did not restore saved parent running modes",
        ));
    }
    // Local parser cleanup only; this control claims no full Tui post-policy,
    // Native handback, E1, application, or UI ACK.
    if modes.release_for_original_handback(&finished).is_ok() {
        return Err(error("duplicate mode handback repeated"));
    }
    resources
        .input_mut()
        .ok_or_else(|| error("retained parser handback absent"))?
        .finish_after_original_handoff(&finished)?;
    finished.apply_and_retire(|| Ok(()))?;
    // Actual root/read/Tui-local handback completed; the same writer resource
    // can now be acquired. Drop it before any generic mode operation.
    drop(super::parent_stdio_writer()?);
    drop(resources);
    drop(surviving_input);
    drop(source);
    super::super::disable_raw_mode()?;
    if observed()? != before {
        return Err(error("original canonical mode was lost"));
    }
    if !parent_writer_denied {
        return Err(error("actual editor admitted a parent stdio writer"));
    }
    writeln!(std::io::stdout().lock(), "ALAB_ORIGINAL_EDITOR_MODES_OK")?;
    Ok(())
}
#[test]
fn actual_editor_modes_keep_source_owner_while_child_changes_running_modes() -> io::Result<()> {
    const CASE: &str = "ALAB_ORIGINAL_EDITOR_MODE_PTY";
    const TEST: &str = "terminal_modes::native::original_editor_tests::actual_editor_modes_keep_source_owner_while_child_changes_running_modes";
    if std::env::var_os(CASE).is_some() {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(helper());
    }
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: valid descriptor out-parameters; no optional termios/window input.
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    let master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    for file in [&master, &slave] {
        // SAFETY: same private actual PTY descriptors; helpers must not retain
        // parent-side master/slave copies through their original Child execs.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        if flags < 0
            || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .args(["--exact", TEST, "--nocapture"])
        .env(CASE, "1")
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave));
    unsafe {
        command.pre_exec(|| {
            // Forked child only: async-signal-safe calls before the helper exec.
            if libc::setsid() < 0
                || libc::ioctl(0, libc::TIOCSCTTY.into(), 0) < 0
                || libc::tcsetpgrp(0, libc::getpgrp()) < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    // Consuming Command closes its parent-side slave handles immediately.
    // The maintained owner drains while the helper is live and closes master
    // before forced kill/wait on refusal/unwind, preserving bounded cleanup.
    let mut child = crate::test_pty::PtyChild::spawn(command, master)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut output = Vec::new();
    loop {
        child.read_output(&mut output)?;
        if let Some(status) = child.try_wait()? {
            while child.read_output(&mut output)? > 0 {
                if Instant::now() >= deadline {
                    return Err(error("original mode helper output did not end"));
                }
            }
            let output = String::from_utf8_lossy(&output);
            if !status.success() || !output.contains("ALAB_ORIGINAL_EDITOR_MODES_OK") {
                return Err(io::Error::other(format!(
                    "original mode helper failed: {output}"
                )));
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err(error("original mode helper timed out"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
