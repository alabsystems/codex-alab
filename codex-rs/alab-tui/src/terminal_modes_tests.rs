// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual controlling PTY and same-PID exec; no global activation in the parent.
use super::*;
use std::fs::File;
use std::io::Write;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::time::Duration;
use std::time::Instant;

fn current() -> libc::termios {
    let mut mode = MaybeUninit::uninit();
    // SAFETY: this child owns its controlling stdin and the writable result.
    assert_eq!(unsafe { libc::tcgetattr(0, mode.as_mut_ptr()) }, 0);
    unsafe { mode.assume_init() }
}
fn observed() -> serde_json::Value {
    let mode = current();
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let line = mode.c_line;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let line = 0u8;
    serde_json::json!({
        "flags": [mode.c_iflag, mode.c_oflag, mode.c_cflag, mode.c_lflag & !libc::PENDIN],
        "characters": mode.c_cc.to_vec(),
        "input_speed": unsafe { libc::cfgetispeed(&mode) },
        "output_speed": unsafe { libc::cfgetospeed(&mode) },
        "line": line,
    })
}
#[test]
fn native_exec_retains_raw_input_and_restores_the_source_original_mode() {
    const CASE: &str = "ALAB_MODE_PTY_CASE";
    const STATE: &str = "ALAB_MODE_PTY_STATE";
    const TEST: &str = "terminal_modes::tests::native_exec_retains_raw_input_and_restores_the_source_original_mode";
    const READY: &str = "ALAB_MODE_RAW";
    const SUCCESS: &str = "ALAB_MODE_RESTORED";
    const BYTES: &[u8] = b" after\r\n\x03\x1b[201~z";
    if let Some(stage) = std::env::var_os(CASE) {
        codex_alab_core::activate();
        let state = std::env::var_os(STATE).unwrap();
        if stage == "source" {
            assert_ne!(current().c_lflag & libc::ICANON, 0);
            let original = observed();
            enable_raw_mode().unwrap();
            enable_raw_mode().unwrap();
            // An input owner over another native description cannot be paired
            // with this terminal, even with a perfectly valid input checkpoint.
            let (reader, _writer) = std::os::unix::net::UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap();
            let descriptor: std::os::fd::OwnedFd = reader.into();
            let input = File::from(descriptor);
            let foreign = codex_alab_input::Broker::new(move || input.try_clone());
            let source = foreign.subscribe(true).unwrap();
            assert!(
                crate::upgrade_terminal::Prepared::prepare(source.prepare_upgrade().unwrap())
                    .is_err()
            );
            drop(source.prepare_upgrade().unwrap());
            drop(source);
            drop(foreign);

            let mut prepared = prepare_upgrade().unwrap();
            assert!(prepared.valid());
            assert!(!prepared.commit_if(|| false).unwrap());
            assert!(prepared.valid());
            enable_raw_mode().unwrap();
            assert!(!prepared.valid());
            assert!(
                !prepared
                    .commit_if(|| panic!("revoked mode called decision"))
                    .unwrap()
            );
            drop(prepared);
            let mut prepared = prepare_upgrade().unwrap();
            let snapshot = prepared.snapshot().clone();
            assert_eq!(
                serde_json::to_value(&snapshot).unwrap()["original"],
                original
            );
            crate::input::buffer_input(b"a\x1b[200~before").unwrap();
            let stream = crate::input::InputStream::default();
            let mut input = stream.prepare_upgrade().unwrap();
            std::fs::write(
                &state,
                serde_json::to_vec(&(std::process::id(), snapshot, input.snapshot())).unwrap(),
            )
            .unwrap();
            assert!(!preserve_on_exit());
            assert!(prepared.commit_if(|| input.commit_if(|| true)).unwrap());
            assert!(preserve_on_exit());
            assert!(disable_raw_mode().is_err());
            assert!(enable_raw_mode().is_err());
            writeln!(std::io::stdout().lock(), "{READY}").unwrap();
            std::io::stdout().flush().unwrap();
            let error = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CASE, "destination")
                .exec();
            panic!("exec failed: {error}");
        }
        assert_eq!(stage, "destination");
        let (pid, snapshot, input): (u32, Snapshot, Vec<u8>) =
            serde_json::from_slice(&std::fs::read(state).unwrap()).unwrap();
        assert_eq!(std::process::id(), pid);
        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(observed(), value["running"]);
        let transferred_running = current();
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        for field in [
            "version",
            "operating_system",
            "terminal",
            "running",
            "original",
        ] {
            let mut wrong = value.clone();
            match field {
                "version" => wrong[field] = serde_json::json!(99),
                "operating_system" => wrong[field] = serde_json::json!("foreign"),
                "terminal" => wrong[field]["inode"] = serde_json::json!(0),
                "running" => wrong[field]["characters"] = serde_json::json!([]),
                "original" => wrong[field]["characters"] = serde_json::json!([]),
                _ => unreachable!(),
            }
            assert!(
                crate::input::restore_after_exec(&serde_json::from_value(wrong).unwrap(), &input)
                    .is_err()
            );
            assert_eq!(observed(), value["running"]);
            assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        }
        crate::input::restore_after_exec(&snapshot, &input).unwrap();
        assert!(crate::input::restore_after_exec(&snapshot, &input).is_err());
        assert!(adopt(&snapshot).is_err());
        assert!(
            prepare_upgrade().is_err(),
            "Crossterm has not enabled its own cache yet"
        );
        enable_raw_mode().unwrap();
        enable_raw_mode().unwrap();
        assert_eq!(
            serde_json::to_value(prepare_upgrade().unwrap().snapshot()).unwrap()["original"],
            value["original"]
        );
        assert!(crate::input::poll(Duration::from_secs(5)).unwrap());
        assert!(
            matches!(crate::input::read().unwrap(), crossterm::event::Event::Key(key) if key.code == crossterm::event::KeyCode::Char('a'))
        );
        assert!(crate::input::poll(Duration::from_secs(5)).unwrap());
        assert_eq!(
            crate::input::read().unwrap(),
            crossterm::event::Event::Paste("before after\r\n\u{3}".into())
        );
        assert!(crate::input::poll(Duration::from_secs(5)).unwrap());
        assert!(
            matches!(crate::input::read().unwrap(), crossterm::event::Event::Key(key) if key.code == crossterm::event::KeyCode::Char('z'))
        );
        disable_raw_mode().unwrap();
        assert_eq!(observed(), value["original"]);
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        disable_raw_mode().unwrap();
        assert_eq!(observed(), value["original"]);
        // Matching raw bytes cannot authorize a late, second adoption after a
        // real mode cycle. The process's native owner history also matters.
        let restored = current();
        assert_eq!(
            unsafe { libc::tcsetattr(0, libc::TCSANOW, &transferred_running) },
            0
        );
        assert_eq!(observed(), value["running"]);
        assert!(adopt(&snapshot).is_err());
        assert_eq!(observed(), value["running"]);
        assert_eq!(unsafe { libc::tcsetattr(0, libc::TCSANOW, &restored) }, 0);
        // A normal later editor/foreground cycle captures its new original mode.
        let mut changed = current();
        changed.c_lflag ^= libc::ECHO;
        assert_eq!(unsafe { libc::tcsetattr(0, libc::TCSANOW, &changed) }, 0);
        let edited = observed();
        enable_raw_mode().unwrap();
        disable_raw_mode().unwrap();
        assert_eq!(observed(), edited);
        enable_raw_mode().unwrap();
        let mut panic_transfer = prepare_upgrade().unwrap();
        assert!(panic_transfer.commit_if(|| true).unwrap());
        struct PanicRestore;
        impl Drop for PanicRestore {
            fn drop(&mut self) {
                disable_raw_mode().unwrap();
            }
        }
        let failure = std::panic::catch_unwind(|| {
            let _restore = PanicRestore;
            panic!("isolated committed terminal recovery");
        });
        assert!(failure.is_err());
        assert_eq!(observed(), edited);
        assert!(!preserve_on_exit());
        assert!(enable_raw_mode().is_err());
        writeln!(std::io::stdout().lock(), "{SUCCESS}").unwrap();
        std::io::stdout().flush().unwrap();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: openpty initializes two unique file descriptors on success.
    assert_eq!(
        unsafe {
            libc::openpty(
                &raw mut master,
                &raw mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    for file in [&master, &slave] {
        assert_eq!(
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
        0
    );
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST, "--nocapture"])
        .env(CASE, "source")
        .env(STATE, root.path().join("modes.json"))
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(slave);
    // SAFETY: child-only async-signal-safe syscalls establish the private TTY.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY.into(), 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = crate::test_pty::PtyChild::spawn(command, master).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut output = Vec::new();
    let mut sent = false;
    loop {
        child.read_output(&mut output).unwrap_or_else(|error| {
            panic!("PTY read: {error}: {}", String::from_utf8_lossy(&output));
        });
        let status = child.try_wait().unwrap();
        if status.is_some() {
            while child.read_output(&mut output).unwrap() > 0 {
                assert!(Instant::now() < deadline, "PTY output did not end");
            }
        }
        let text = String::from_utf8_lossy(&output);
        if !sent && text.contains(READY) {
            child.master().write_all(BYTES).unwrap();
            sent = true;
        }
        if let Some(status) = status {
            assert!(status.success(), "{text}");
            assert!(sent && text.contains(SUCCESS), "{text}");
            break;
        }
        assert!(Instant::now() < deadline, "PTY timed out: {text}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
