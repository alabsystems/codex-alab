// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real controlling PTY through the TUI's production source factory and pause path.
use super::super::CrosstermEventSource;
use super::super::EventBroker;
use super::super::EventSource;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;
use std::time::Duration;
use std::time::Instant;
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn next(broker: &EventBroker) -> Event {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = {
            let mut state = broker
                .state
                .lock()
                .unwrap_or_else(|error| panic!("native broker mutex failed: {error}"));
            Pin::new(
                state
                    .active_event_source_mut()
                    .unwrap_or_else(|| panic!("native input source is paused")),
            )
            .poll_next(&mut context)
        };
        match result {
            Poll::Ready(Some(event)) => {
                return event.unwrap_or_else(|error| panic!("native reader failed: {error}"));
            }
            Poll::Ready(None) => panic!("native input ended"),
            Poll::Pending => {
                assert!(Instant::now() < deadline);
                std::thread::park_timeout(Duration::from_millis(10));
            }
        }
    }
}
#[test]
fn native_tui_pause_joins_and_retains_partial_input_on_the_controlling_pty() {
    const CASE: &str = "ALAB_TUI_INPUT_PTY_CASE";
    const TEST: &str = "tui::event_stream::alab_kitty_replies::owned_input_tests::native_tui_pause_joins_and_retains_partial_input_on_the_controlling_pty";
    const READY: &str = "ALAB_TUI_INPUT_READY";
    const FROZEN: &str = "ALAB_TUI_INPUT_FROZEN";
    const SUCCESS: &str = "ALAB_TUI_INPUT_OK";
    const EXIT_OUTPUT: &str = "ALAB_TUI_INPUT_OUTPUT_DRAINED";
    const EXIT_BYTES: usize = 1024 * 1024;
    const HANDOFF: &str = "ALAB_TUI_INPUT_HANDOFF";
    const TAIL: &[u8] = b"handoff\r\n\x03";
    if std::env::var_os(CASE).is_some() {
        codex_alab_core::activate();
        codex_alab_tui::terminal_modes::enable_raw_mode().unwrap();
        let broker = Arc::new(EventBroker::<CrosstermEventSource>::new());
        writeln!(std::io::stdout().lock(), "{READY}").unwrap();
        std::io::stdout().flush().unwrap();
        assert!(matches!(next(&broker), Event::Key(key) if key.code == KeyCode::Char('a')));
        {
            let state = broker.state.lock().unwrap();
            let super::super::EventBrokerState::Running(source) = &*state else {
                panic!("source is absent");
            };
            assert!(matches!(&source.events.inner, super::Input::Owned(_)));
        }
        let fence = broker.prepare_upgrade_input().unwrap();
        broker.pause_events();
        let snapshot: serde_json::Value = serde_json::from_slice(fence.snapshot()).unwrap();
        assert_eq!(
            snapshot["buffer"],
            serde_json::json!(b"\x1b[200~before".to_vec())
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (release, released) = tokio::sync::oneshot::channel();
        let (request, _cancellation) = codex_alab_core::session::UpgradeRequest::with_context(
            move |input: codex_alab_tui::input::UpgradeInput| {
                Some(codex_alab_core::session::UpgradeCompletion::new(
                    async move {
                        released.await.map_err(|error| error.to_string())?;
                        drop(input);
                        Ok(())
                    },
                ))
            },
        );
        let completion = {
            let _entered = runtime.enter();
            let codex_alab_core::session::UpgradeDispatch::Capturing(completion) =
                request.decide_with_context(true, fence)
            else {
                panic!("native input custody was not dispatched");
            };
            completion
        };
        let waiter = runtime.spawn({
            let completion = completion.clone();
            async move { completion.wait().await }
        });
        waiter.abort();
        assert!(runtime.block_on(waiter).unwrap_err().is_cancelled());
        writeln!(std::io::stdout().lock(), "{FROZEN}").unwrap();
        std::io::stdout().flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut count: libc::c_int = 0;
            // SAFETY: stdin is this fixture's controlling PTY; this only queries bytes.
            assert_eq!(
                unsafe { libc::ioctl(libc::STDIN_FILENO, libc::FIONREAD, &raw mut count) },
                0
            );
            if count > 0 {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        release.send(()).unwrap();
        runtime.block_on(completion.wait()).unwrap();
        broker.resume_events();
        assert_eq!(next(&broker), Event::Paste("before after".into()));
        assert!(matches!(next(&broker), Event::Key(key) if key.code == KeyCode::Char('z')));
        for cancelled in [true, false] {
            let capture = broker.prepare_upgrade_capture().unwrap();
            assert!(capture.valid());
            let (request, cancellation) = if cancelled {
                codex_alab_core::session::UpgradeRequest::with_context(
                    |_: codex_alab_tui::upgrade_terminal::Prepared| -> Option<codex_alab_core::session::UpgradeCompletion> {
                        panic!("cancelled capture invoked its source callback");
                    },
                )
            } else {
                codex_alab_core::session::UpgradeRequest::with_context(
                    |_: u64| -> Option<codex_alab_core::session::UpgradeCompletion> {
                        panic!("wrong context invoked its source callback");
                    },
                )
            };
            let _cancellation = if cancelled {
                drop(cancellation);
                None
            } else {
                Some(cancellation)
            };
            let _entered = runtime.enter();
            assert!(matches!(
                request.decide_with_context(true, capture),
                codex_alab_core::session::UpgradeDispatch::Refused
            ));
            // The next iteration/real preparation must acquire both owners
            // again, proving refusal rolled back input AND mode admission.
        }
        let mut capture = broker.prepare_upgrade_capture().unwrap();
        assert!(capture.valid());
        assert!(!capture.committed());
        assert!(!capture.commit_if(|| false).unwrap());
        assert!(capture.valid());
        assert!(!capture.input_snapshot().is_empty());
        let _mode_data = serde_json::to_value(capture.mode_snapshot()).unwrap();
        assert!(capture.commit_if(|| true).unwrap());
        assert!(capture.committed());
        assert!(
            !capture
                .commit_if(|| panic!("committed capture reused its decision"))
                .unwrap()
        );
        writeln!(std::io::stdout().lock(), "{HANDOFF}").unwrap();
        std::io::stdout().flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let queued = loop {
            let mut queued: libc::c_int = 0;
            assert_eq!(
                unsafe { libc::ioctl(0, libc::FIONREAD, &raw mut queued) },
                0
            );
            if queued >= libc::c_int::try_from(TAIL.len()).unwrap() {
                break queued;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        let mut restore = crate::TerminalRestoreGuard::new();
        restore.events = Some(broker.clone());
        restore.restore().unwrap();
        let mut remaining: libc::c_int = 0;
        assert_eq!(
            unsafe { libc::ioctl(0, libc::FIONREAD, &raw mut remaining) },
            0
        );
        assert_eq!(
            remaining, queued,
            "the actual exit guard must not flush input"
        );
        assert!(crossterm::terminal::is_raw_mode_enabled().unwrap());
        let mut tail = vec![0; TAIL.len()];
        std::io::stdin().read_exact(&mut tail).unwrap();
        assert_eq!(tail, TAIL);
        capture.restore_after_failure().unwrap();
        assert!(!codex_alab_tui::terminal_modes::preserve_on_exit());
        assert!(codex_alab_tui::terminal_modes::enable_raw_mode().is_err());
        // Aborting a committed exec restores normal exit cleanup, without
        // reopening terminal admission or a source input reader.
        let mut aborted = crate::TerminalRestoreGuard::new();
        aborted.events = Some(broker);
        aborted.restore().unwrap();
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{SUCCESS}").unwrap();
        stdout.flush().unwrap();
        // Exceed the PTY output queue after the protocol succeeds. The parent
        // must keep reading until exit instead of waiting with a full queue.
        for _ in 0..EXIT_BYTES / 4096 {
            stdout.write_all(&[b'.'; 4096]).unwrap();
        }
        writeln!(stdout, "{EXIT_OUTPUT}").unwrap();
        stdout.flush().unwrap();
        return;
    }
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: checked openpty initializes these two uniquely owned descriptors.
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
        .env(CASE, "native")
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(slave);
    // SAFETY: this child-only hook uses async-signal-safe syscalls after stdio
    // setup, establishing this fixture's independent controlling terminal.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY.into(), 0) < 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = codex_alab_tui::test_pty::PtyChild::spawn(command, master).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut output = Vec::new();
    for marker in [READY, FROZEN, HANDOFF, SUCCESS] {
        while !String::from_utf8_lossy(&output).contains(marker) {
            child.read_output(&mut output).unwrap_or_else(|error| {
                panic!(
                    "PTY failed before {marker}: {error}: {}",
                    String::from_utf8_lossy(&output)
                );
            });
            assert!(
                Instant::now() < deadline,
                "PTY timed out before {marker}: {}",
                String::from_utf8_lossy(&output)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        if marker == READY {
            child.master().write_all(b"a\x1b[200~before").unwrap();
        }
        if marker == FROZEN {
            child.master().write_all(b" after\x1b[201~z").unwrap();
        }
        if marker == HANDOFF {
            child.master().write_all(TAIL).unwrap();
        }
    }
    loop {
        child.read_output(&mut output).unwrap();
        if let Some(status) = child.try_wait().unwrap() {
            while child.read_output(&mut output).unwrap() > 0 {
                assert!(Instant::now() < deadline, "PTY output did not end");
            }
            let text = String::from_utf8_lossy(&output);
            assert!(
                status.success(),
                "PTY child exited with {status}: {}",
                String::from_utf8_lossy(&output[output.len().saturating_sub(4096)..])
            );
            let tail = text.split_once(SUCCESS).unwrap().1;
            let payload = tail.split_once(EXIT_OUTPUT).expect("final output marker").0;
            assert_eq!(payload.trim_start_matches(['\r', '\n']).len(), EXIT_BYTES);
            assert!(
                payload
                    .trim_start_matches(['\r', '\n'])
                    .bytes()
                    .all(|b| b == b'.')
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "PTY child did not exit after success ({} output bytes): {}",
            output.len(),
            String::from_utf8_lossy(&output[output.len().saturating_sub(4096)..])
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
