// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual startup on both sides of same-PID exec, with a private controlling PTY.
use super::*;
use futures::StreamExt;
use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::time::Duration;
use std::time::Instant;

#[test]
fn startup_exec_preserves_the_screen_without_queries_and_completes_the_paste() {
    const CASE: &str = "ALAB_STARTUP_EXEC_CASE";
    const STATE: &str = "ALAB_STARTUP_EXEC_STATE";
    const TEST: &str = "tui::alab_upgrade_startup::tests::startup_exec_preserves_the_screen_without_queries_and_completes_the_paste";
    const DROP_BEGIN: &str = "ALAB_STARTUP_DROP_BEGIN";
    const DROP_END: &str = "ALAB_STARTUP_DROP_END";
    const READY: &str = "ALAB_STARTUP_SOURCE_READY";
    const BEGIN: &str = "ALAB_STARTUP_DEST_BEGIN";
    const END: &str = "ALAB_STARTUP_DEST_END";
    const SUCCESS: &str = "ALAB_STARTUP_EXEC_OK";
    if let Some(stage) = std::env::var_os(CASE) {
        codex_alab_core::activate();
        let path = std::env::var_os(STATE).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _entered = runtime.enter();
        if stage == "source" {
            let initialized = init().unwrap();
            let mut tui = Tui::new(initialized.terminal, true, initialized.stderr_guard);
            tui.terminal_app_over_ssh = true;
            crate::terminal_probe::alab_set_da1_supported(true);
            tui.terminal.tmux_kitty_image_base = Some(17);
            tui.terminal.set_viewport_area(Rect::new(0, 4, 80, 3));
            tui.set_owned_screen(true).unwrap();
            tui.terminal_focused.store(false, Ordering::Relaxed);
            codex_alab_tui::input::buffer_input(b"a\x1b[200~before").unwrap();
            runtime.block_on(async {
                let mut events = tui.event_stream();
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        if matches!(events.next().await, Some(TuiEvent::Key(key)) if key.code == crossterm::event::KeyCode::Char('a')) { break; }
                    }
                }).await.unwrap();
            });
            let mut capture = tui.alab_prepare_upgrade_capture().unwrap();
            let view = capture.startup_snapshot().unwrap().clone();
            assert!(view.screen.active && view.screen.mouse_active && view.owned_screen);
            assert_eq!(view.saved_viewport, Some([0, 4, 80, 3]));
            std::fs::write(
                &path,
                serde_json::to_vec(&(
                    std::process::id(),
                    &view,
                    capture.mode_snapshot(),
                    capture.input_snapshot(),
                ))
                .unwrap(),
            )
            .unwrap();
            assert!(
                codex_alab_tui::upgrade_startup::restore(
                    view,
                    capture.mode_snapshot(),
                    capture.input_snapshot()
                )
                .is_err()
            );
            assert!(capture.valid());
            assert!(capture.commit_if(|| true).unwrap());
            restore_after_exit_with_events(Some(&tui.event_broker)).unwrap();
            write!(std::io::stdout(), "{DROP_BEGIN}").unwrap();
            std::io::stdout().flush().unwrap();
            drop(tui);
            write!(std::io::stdout(), "{DROP_END}").unwrap();
            writeln!(std::io::stdout(), "{READY}").unwrap();
            std::io::stdout().flush().unwrap();
            panic!(
                "exec failed: {}",
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture"])
                    .env(CASE, "destination")
                    .exec()
            );
        }
        assert_eq!(stage, "destination");
        let (pid, view, modes, input): (
            u32,
            Snapshot,
            codex_alab_tui::terminal_modes::Snapshot,
            Vec<u8>,
        ) = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(pid, std::process::id());
        let ready = std::path::PathBuf::from(path).with_extension("ready");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut wrong = view.clone();
        wrong.version = 99;
        assert!(codex_alab_tui::upgrade_startup::restore(wrong, &modes, &input).is_err());
        assert!(
            codex_alab_tui::upgrade_startup::restore(view.clone(), &modes, b"invalid parser")
                .is_err()
        );
        for missing in ["colors", "tmux_image_base", "saved_viewport"] {
            let mut wire = serde_json::to_value(&view).unwrap();
            wire.as_object_mut().unwrap().remove(missing);
            assert!(serde_json::from_value::<Snapshot>(wire).is_err());
        }
        codex_alab_tui::upgrade_startup::restore(view.clone(), &modes, &input).unwrap();
        assert!(codex_alab_tui::upgrade_startup::restore(view.clone(), &modes, &input).is_err());
        write!(std::io::stdout(), "{BEGIN}").unwrap();
        std::io::stdout().flush().unwrap();
        let initialized = init().unwrap();
        assert!(initialized.enhanced_keys_supported && initialized.terminal_app_over_ssh);
        let mut tui = Tui::new(
            initialized.terminal,
            initialized.enhanced_keys_supported,
            initialized.stderr_guard,
        );
        assert_eq!(
            tui.terminal.last_known_screen_size,
            ratatui::layout::Size::new(100, 30)
        );
        let mut expected = view;
        expected.viewport = [0, 0, 100, 30];
        assert_eq!(tui.alab_upgrade_startup_snapshot(), expected);
        // Reapplying configuration must observe the adopted screen and avoid an
        // alternate-screen entry or another keyboard-stack push.
        tui.set_owned_screen(true).unwrap();
        write!(std::io::stdout(), "{END}").unwrap();
        std::io::stdout().flush().unwrap();
        assert!(codex_alab_tui::input::poll(Duration::from_secs(5)).unwrap());
        assert_eq!(
            codex_alab_tui::input::read().unwrap(),
            crossterm::event::Event::Paste("before after\r\n\u{3}".into())
        );
        assert!(codex_alab_tui::input::poll(Duration::from_secs(5)).unwrap());
        assert!(
            matches!(codex_alab_tui::input::read().unwrap(), crossterm::event::Event::Key(key) if key.code == crossterm::event::KeyCode::Char('z'))
        );
        restore_after_exit_with_events(Some(&tui.event_broker)).unwrap();
        let mut mode = std::mem::MaybeUninit::uninit();
        assert_eq!(unsafe { libc::tcgetattr(0, mode.as_mut_ptr()) }, 0);
        assert_ne!(unsafe { mode.assume_init() }.c_lflag & libc::ICANON, 0);
        assert!(!ALTERNATE_SCREEN.is_active());
        writeln!(std::io::stdout(), "{SUCCESS}").unwrap();
        std::io::stdout().flush().unwrap();
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut master, mut slave) = (-1, -1);
    let mut size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe {
            libc::openpty(
                &raw mut master,
                &raw mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut size,
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
        .env(STATE, directory.path().join("state.json"))
        .env("TERM", "xterm-256color")
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(slave);
    // SAFETY: only the child's private terminal descriptors are changed.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY.into(), 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = codex_alab_tui::test_pty::PtyChild::spawn(command, master).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut output = Vec::new();
    let mut sent = false;
    let mut exit_report_sent = false;
    loop {
        child.read_output(&mut output).unwrap_or_else(|error| {
            panic!("PTY: {error}: {}", String::from_utf8_lossy(&output));
        });
        let status = child.try_wait().unwrap();
        if status.is_some() {
            while child.read_output(&mut output).unwrap() > 0 {
                assert!(Instant::now() < deadline, "PTY output did not end");
            }
        }
        let text = String::from_utf8_lossy(&output);
        if !sent && text.contains(READY) {
            let resized = libc::winsize {
                ws_row: 30,
                ws_col: 100,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            assert_eq!(
                unsafe { libc::ioctl(child.master().as_raw_fd(), libc::TIOCSWINSZ, &resized) },
                0
            );
            child
                .master()
                .write_all(b" after\r\n\x03\x1b[201~z")
                .unwrap();
            std::fs::write(directory.path().join("state.ready"), b"ready").unwrap();
            sent = true;
        }
        if !exit_report_sent
            && text
                .split_once(END)
                .is_some_and(|(_, tail)| tail.contains("\x1b[c"))
        {
            child.master().write_all(b"\x1b[?1;2c").unwrap();
            exit_report_sent = true;
        }
        if let Some(status) = status {
            assert!(status.success(), "{text}");
            assert!(sent && exit_report_sent && text.contains(SUCCESS), "{text}");
            let drop_output = text
                .split_once(DROP_BEGIN)
                .unwrap()
                .1
                .split_once(DROP_END)
                .unwrap()
                .0;
            assert_eq!(
                drop_output, "",
                "committed source drop changed terminal state: {text}"
            );
            let between = text.split_once(BEGIN).unwrap().1.split_once(END).unwrap().0;
            assert_eq!(
                between, "",
                "replacement startup emitted terminal commands: {text}"
            );
            break;
        }
        assert!(Instant::now() < deadline, "PTY timed out: {text}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
