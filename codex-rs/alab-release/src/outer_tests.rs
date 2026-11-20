// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

#[test]
fn isolated_owner_fixture() -> Result<()> {
    let Some(fd) = std::env::var_os("ALAB_TEST_RELEASE_OWNER_FD") else {
        return Ok(());
    };
    let result = run(Some(fd.to_str().unwrap().parse()?), || {
        let mut already_exited = Command::new("/bin/sh").args(["-c", "exit 75"]).spawn()?;
        // Observe a real child exit without consuming the wait used below.
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::uninit();
        loop {
            // SAFETY: this is our child; WNOWAIT retains its identity and the
            // kernel writes to a properly sized siginfo_t.
            if unsafe {
                libc::waitid(
                    libc::P_PID,
                    already_exited.id(),
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOWAIT,
                )
            } == 0
            {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
        assert_eq!(wait(&mut already_exited, false)?.code(), Some(75));
        let lease = File::open("lease")?;
        // SAFETY: this fixture owns the open descriptor throughout the action.
        if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let mut publisher = Command::new("/bin/sh");
        publisher.args([
            "-c",
            "trap ': > cancelled' TERM; : > ready; n=0; while [ ! -f finish ] && [ \"$n\" -lt 1600 ]; do sleep 0.05; n=$((n+1)); done",
        ]);
        let status = publisher_status(&mut publisher)?;
        check()?;
        crate::stage_status::require_success("fixture publisher", status)?;
        std::fs::write("completed", [])?;
        Ok(())
    });
    assert_eq!(
        crate::stage_status::exit_code(result.unwrap_err().as_ref()),
        Some(143)
    );
    Ok(())
}

#[test]
fn owner_loss_before_dispatch_and_during_native_settlement() -> Result<()> {
    for cancel_early in [true, false] {
        let root = tempfile::tempdir()?;
        let lease = File::create(root.path().join("lease"))?;
        let (reader, mut writer) = std::io::pipe()?;
        writer.write_all(HEADER)?;
        let mut writer = Some(writer);
        if cancel_early {
            drop(writer.take());
        }
        let fd = reader.as_raw_fd();
        let mut fixture = Command::new(std::env::current_exe()?);
        fixture
            .args(["--exact", "outer::tests::isolated_owner_fixture"])
            .env("ALAB_TEST_RELEASE_OWNER_FD", fd.to_string())
            .current_dir(root.path());
        isolate(&mut fixture, fd);
        // SAFETY: only the fork child changes its signal disposition. The
        // isolated owner's adopted CHLD policy must make its publisher waitable.
        unsafe {
            fixture.pre_exec(|| {
                if libc::signal(libc::SIGCHLD, libc::SIG_IGN) == libc::SIG_ERR {
                    return Err(io::Error::last_os_error());
                }
                let mut set = std::mem::zeroed();
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGCHLD);
                let rc = libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
                if rc != 0 {
                    return Err(io::Error::from_raw_os_error(rc));
                }
                Ok(())
            });
        }
        let mut child = fixture.spawn()?;
        drop(reader);
        let observation = (|| -> Result<()> {
            if !cancel_early {
                wait_for(&root.path().join("ready"))?;
                drop(writer.take());
                wait_for(&root.path().join("cancelled"))?;
                if child.try_wait()?.is_some() {
                    return Err("release owner ended before its publisher settled".into());
                }
                // SAFETY: independent open of the same lease. Neither process
                // may release the original action's lock during native drain.
                if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    return Err("release lease ended before its publisher settled".into());
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::WouldBlock {
                    return Err(error.into());
                }
            }
            Ok(())
        })();
        drop(writer);
        std::fs::write(root.path().join("finish"), [])?;
        let status = child.wait()?;
        observation?;
        assert!(status.success(), "isolated owner fixture returned {status}");
        assert_eq!(root.path().join("ready").exists(), !cancel_early);
        assert!(!root.path().join("completed").exists());
        // SAFETY: the fixture has now exited and released its own descriptor.
        assert_eq!(
            unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
    }
    Ok(())
}

fn wait_for(path: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(format!("fixture never wrote {}", path.display()).into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
