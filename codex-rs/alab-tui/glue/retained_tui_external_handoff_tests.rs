// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real native Tui pre/post operations on a private controlling terminal.
//! These are local handoff controls, not EditorFinished/Native grant fixtures.
use super::*;
use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::time::Instant;

fn input_line_modes() -> io::Result<libc::tcflag_t> {
    let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: the child owns this PTY, and tcgetattr initializes mode on success.
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, mode.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: tcgetattr succeeded.
    Ok(unsafe { mode.assume_init() }.c_lflag & (libc::ICANON | libc::ECHO | libc::ISIG))
}

#[test]
fn real_original_handoff_retains_tty_boundary_until_actual_resume_or_error() -> io::Result<()> {
    const CASE: &str = "ALAB_EDITOR_HANDOFF_PTY_CASE";
    const TEST: &str = "tui::alab_external_handoff::tests::real_original_handoff_retains_tty_boundary_until_actual_resume_or_error";
    const SUCCESS: &str = "ALAB_EDITOR_HANDOFF_OK";
    if let Some(case) = std::env::var_os(CASE) {
        codex_alab_core::activate();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let _entered = runtime.enter();
        let original_modes = input_line_modes()?;
        set_modes()?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        assert!(tui.alab_parent_tty_available());
        if case == "ordinary" {
            let changes = tui.event_broker.resume_events_rx();
            let output =
                runtime.block_on(tui.with_restored(TerminalHandoff::KeepScreen, || async {
                    assert!(
                        std::process::Command::new("stty")
                            .arg("sane")
                            .status()?
                            .success()
                    );
                    assert_eq!(input_line_modes()?, libc::ICANON | libc::ECHO | libc::ISIG);
                    assert!(crossterm::terminal::is_raw_mode_enabled()?);
                    Ok::<_, io::Error>(423)
                }))?;
            assert_eq!(output, 423);
            assert_eq!(input_line_modes()?, 0);
            assert!(tui.alab_parent_tty_available());
            assert!(changes.has_changed().map_err(io::Error::other)?);
        } else {
            let changes = tui.event_broker.resume_events_rx();
            let mut owner = tui.alab_begin_external_handoff(TerminalHandoff::KeepScreen)?;
            assert!(!tui.alab_parent_tty_available());
            assert!(!changes.has_changed().map_err(io::Error::other)?);
            assert!(
                tui.alab_begin_external_handoff(TerminalHandoff::Restore)
                    .is_err()
            );
            if case == "held" {
                owner.finish_with(&mut tui, || Ok(()))?;
                assert_eq!(owner.actual.phase.load(Ordering::Acquire), RESTORED);
                assert!(changes.has_changed().map_err(io::Error::other)?);
                assert!(tui.alab_parent_tty_available());
                assert!(
                    owner
                        .finish_with(&mut tui, || panic!("actual original handback repeated"))
                        .is_err()
                );
            } else {
                assert_eq!(case, "failed");
                assert!(
                    owner
                        .finish_with(&mut tui, || Err(io::Error::other(
                            "original input handback unavailable"
                        )))
                        .is_err()
                );
                assert_eq!(owner.actual.phase.load(Ordering::Acquire), UNKNOWN);
                assert!(!changes.has_changed().map_err(io::Error::other)?);
                assert!(!tui.alab_parent_tty_available());
                assert!(
                    owner
                        .finish_with(&mut tui, || panic!("uncertain original handback repeated"))
                        .is_err()
                );
                drop(owner);
                assert!(!tui.alab_parent_tty_available());
            }
        }
        // Check that exit restores the original shell flags on the private PTY.
        restore_after_exit_with_events(Some(&tui.event_broker))?;
        assert_eq!(input_line_modes()?, original_modes);
        writeln!(std::io::stdout().lock(), "{SUCCESS}")?;
        return Ok(());
    }
    for case in ["ordinary", "held", "failed"] {
        let (mut master, mut slave) = (-1, -1);
        let mut size = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: successful openpty initializes two uniquely owned descriptors.
        if unsafe {
            libc::openpty(
                &raw mut master,
                &raw mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut size,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        for file in [&master, &slave] {
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        if unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command
            .args(["--exact", TEST, "--nocapture"])
            .env(CASE, case)
            .env("TERM", "xterm-256color")
            .stdin(slave.try_clone()?)
            .stdout(slave.try_clone()?)
            .stderr(slave);
        // SAFETY: child-only async-signal-safe controlling-terminal setup.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY.into(), 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = codex_alab_tui::test_pty::PtyChild::spawn(command, master)?;
        let mut output = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            child.read_output(&mut output)?;
            if let Some(status) = child.try_wait()? {
                while child.read_output(&mut output)? > 0 {
                    assert!(
                        Instant::now() < deadline,
                        "actual handoff output did not end"
                    );
                }
                let output = String::from_utf8_lossy(&output);
                assert!(
                    status.success() && output.contains(SUCCESS),
                    "{case}: {output}"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{case}: actual handoff timed out"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}
