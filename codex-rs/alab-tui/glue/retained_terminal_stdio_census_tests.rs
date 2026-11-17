// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real isolated stdio/global-stderr descriptions. No editor, C0 retirement,
//! SDK/Native enrollment or frontend permission is fabricated here.
use super::*;
use std::fs::File;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::time::Duration;
use std::time::Instant;

fn replace(source: BorrowedFd<'_>, target: RawFd) -> io::Result<()> {
    // SAFETY: source remains genuinely owned through this child-only dup2.
    if unsafe { libc::dup2(source.as_raw_fd(), target) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn child_case(case: &str) -> io::Result<()> {
    if case == "redirected" {
        let mut original = tempfile::tempfile()?;
        original.write_all(b"prefix\nnext marker\n")?;
        original.seek(SeekFrom::Start(7))?;
        replace(original.as_fd(), libc::STDERR_FILENO)?;
        let guard = TerminalStderrGuard::install()?;
        let census = Captured::capture(&guard)?;
        census.check_process()?;
        assert_eq!(census.stderr_state_data(), (false, false));
        assert!(census.saved_stderr().is_none());
        let stderr = census.parent_descriptors()[2].try_clone_to_owned()?;
        let mut stderr = File::from(stderr);
        assert_eq!(stderr.stream_position()?, 7);
        stderr.seek(SeekFrom::Start(11))?;
        assert_eq!(original.stream_position()?, 11);
        original.seek(SeekFrom::Start(7))?;
        assert_eq!(stderr.stream_position()?, 7);
        return Ok(());
    }

    let guard = TerminalStderrGuard::install()?;
    // All three inherited real descriptions are the SAME controlling PTY;
    // original native policy therefore created actual stderr suppression.
    assert!(guard.active);
    let census = Captured::capture(&guard)?;
    assert_eq!(census.stderr_state_data(), (true, true));
    assert!(census.saved_stderr().is_some());
    for actual in census
        .parent_descriptors()
        .into_iter()
        .chain(census.saved_stderr())
    {
        // SAFETY: the census itself still owns each supplied description.
        let flags = unsafe { libc::fcntl(actual.as_raw_fd(), libc::F_GETFD) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }
    if case == "saved" {
        super::super::pause()?;
        let paused = Captured::capture(&guard)?;
        assert_eq!(paused.stderr_state_data(), (true, true));
        assert!(paused.saved_stderr().is_none());
        // The actual prior saved description survives its original global
        // owner's release; no absent descriptor is manufactured or reopened.
        let retained = census
            .saved_stderr()
            .ok_or_else(|| io::Error::other("saved stderr lost"))?;
        // SAFETY: the retained descriptor is still owned by census.
        if unsafe { libc::fcntl(retained.as_raw_fd(), libc::F_GETFD) } < 0 {
            return Err(io::Error::last_os_error());
        }
    } else {
        assert_eq!(case, "closed");
        let saved_stdin = duplicate_standard(libc::STDIN_FILENO)?;
        // SAFETY: child-only close creates a genuine absent standard input.
        if unsafe { libc::close(libc::STDIN_FILENO) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let error = Captured::capture(&guard)
            .err()
            .ok_or_else(|| io::Error::other("closed stdin was accepted"))?;
        replace(saved_stdin.as_fd(), libc::STDIN_FILENO)?;
        assert_eq!(error.raw_os_error(), Some(libc::EBADF));
        let current = Captured::capture(&guard)?;
        assert_eq!(current.stderr_state_data(), (true, true));
        assert!(current.saved_stderr().is_some());
    }
    super::super::finish()?;
    Ok(())
}

#[test]
fn actual_stdio_census_preserves_aliases_saved_absence_and_closed_fd_refusal() -> io::Result<()> {
    const CASE: &str = "ALAB_STDIO_CENSUS_PTY_CASE";
    const NAME: &str = "actual_stdio_census_preserves_aliases_saved_absence_and_closed_fd_refusal";
    const SUCCESS: &str = "ALAB_STDIO_CENSUS_OK";
    if let Some(case) = std::env::var_os(CASE) {
        let case = case
            .to_str()
            .ok_or_else(|| io::Error::other("invalid stdio census case"))?;
        child_case(case)?;
        writeln!(std::io::stdout().lock(), "{SUCCESS}")?;
        return Ok(());
    }
    let module = module_path!()
        .split_once("::")
        .ok_or_else(|| io::Error::other("native test module missing"))?
        .1;
    let test = format!("{module}::{NAME}");
    for case in ["redirected", "saved", "closed"] {
        let (mut master, mut slave) = (-1, -1);
        let mut size = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: successful openpty initializes exactly two owned descriptors.
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
        // SAFETY: the successful openpty call transferred each unique resource.
        let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        for file in [&master, &slave] {
            // SAFETY: this actual File owns the supplied descriptor.
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        // SAFETY: the original master remains held by the bounded child owner.
        if unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command
            .args(["--exact", &test, "--nocapture"])
            .env(CASE, case)
            .env("TERM", "xterm-256color")
            .stdin(slave.try_clone()?)
            .stdout(slave.try_clone()?)
            .stderr(slave);
        // SAFETY: only child-side async-signal-safe terminal setup is performed.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY.into(), 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = codex_alab_tui::test_pty::PtyChild::spawn(command, master)?;
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut output = Vec::new();
        loop {
            child.read_output(&mut output)?;
            if let Some(status) = child.try_wait()? {
                while child.read_output(&mut output)? > 0 {
                    assert!(
                        Instant::now() < deadline,
                        "actual census output did not finish"
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
                "{case}: actual stdio census timed out"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}
