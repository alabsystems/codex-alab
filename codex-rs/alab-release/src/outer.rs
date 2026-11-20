// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The public CLI stays in its caller's job. Its isolated child owns all release
//! leases and keeps the signed compiler selector alive through native settlement.

use codex_alab_release::Result;
use signal_hook::iterator::backend::SignalDelivery;
use signal_hook::iterator::exfiltrator::SignalOnly;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::RawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

const HEADER: &[u8] = b"codex-release owner v1\n";
static EVENTS: OnceLock<Mutex<SignalDelivery<UnixStream, SignalOnly>>> = OnceLock::new();
static LIFELINE: OnceLock<std::fs::File> = OnceLock::new();
static SIGNAL: AtomicI32 = AtomicI32::new(0);

fn cancelled(signal: i32) {
    let _ = SIGNAL.compare_exchange(0, signal, Ordering::Relaxed, Ordering::Relaxed);
}

fn arm() -> io::Result<()> {
    let signals = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGCHLD];
    // A shell may have ignored CHLD or requested SA_NOCLDWAIT. Restore
    // waitable children before installing the signal-hook notification.
    // SAFETY: the initialized disposition is installed only in this CLI.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_sigaction = libc::SIG_DFL;
        if libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let (read, write) = UnixStream::pair()?;
    let events = SignalDelivery::with_pipe(read, write, SignalOnly, signals)?;
    EVENTS
        .set(Mutex::new(events))
        .map_err(|_| io::Error::other("release signals already armed"))?;
    // SAFETY: initialize and unblock only the signals the self-pipe now captures.
    unsafe {
        let mut set = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for sig in signals {
            libc::sigaddset(&mut set, sig);
        }
        let errno = libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        if errno != 0 {
            return Err(io::Error::from_raw_os_error(errno));
        }
    }
    Ok(())
}

fn adopt(fd: RawFd) -> Result<()> {
    // Only the re-exec is a session leader holding this read-only descriptor.
    // SAFETY: these queries take integer descriptors or initialize the supplied stat.
    let mut info: libc::stat = unsafe { std::mem::zeroed() };
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if fd < 3
        || unsafe { libc::getsid(0) != libc::getpid() }
        || flags < 0
        || flags & libc::O_ACCMODE != libc::O_RDONLY
        || unsafe { libc::fstat(fd, &mut info) } != 0
        || info.st_mode & libc::S_IFMT != libc::S_IFIFO
    {
        return Err("invalid release owner lifeline".into());
    }
    // SAFETY: the validated descriptor is the inherited reader. Seal it
    // before any subprocess can start; the header was written before spawn.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
        || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error().into());
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut header = [0; HEADER.len()];
    file.read_exact(&mut header)?;
    if header.as_slice() != HEADER {
        return Err("invalid release owner handshake".into());
    }
    LIFELINE
        .set(file)
        .map_err(|_| "release owner already set")?;
    Ok(())
}

fn interruption() -> Option<i32> {
    if let Some(events) = EVENTS.get() {
        let mut events = events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for signal in events.pending() {
            if signal != libc::SIGCHLD {
                cancelled(signal);
            }
        }
    }
    let signal = SIGNAL.load(Ordering::Relaxed);
    if signal != 0 {
        return Some(signal);
    }
    let mut reader = LIFELINE.get()?;
    let mut byte = [0];
    loop {
        match reader.read(&mut byte) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return None,
            // The caller writes only the header. EOF, unexpected data or an
            // unreadable channel withdraws its authority to start more work.
            _ => {
                cancelled(libc::SIGTERM);
                return Some(SIGNAL.load(Ordering::Relaxed));
            }
        }
    }
}

pub(super) fn check() -> Result<()> {
    if let Some(signal) = interruption() {
        crate::stage_status::require_success("release action", ExitStatus::from_raw(signal))?;
    }
    Ok(())
}

/// The sole waiter either observes exit or signals its own unreaped child.
/// A successful try_wait consumes the PID and returns before any signal.
fn wait(child: &mut Child, publisher: bool) -> io::Result<ExitStatus> {
    let events = EVENTS.get().expect("armed before spawn");
    let wake = events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_read()
        .as_raw_fd();
    let mut ends = [wake, LIFELINE.get().map_or(-1, AsRawFd::as_raw_fd)].map(|fd| libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    });
    loop {
        // Drain the wake before testing the child. An exit or signal after
        // that observation leaves the socket readable until our next check.
        let cancellation = interruption();
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if let Some(signal) = cancellation {
            let signal = if publisher { libc::SIGTERM } else { signal };
            return stop_and_wait(child, signal);
        }
        // SAFETY: both descriptors remain owned by this process for the whole
        // wait. poll ignores the negative slot when there is no outer lifeline.
        if unsafe { libc::poll(ends.as_mut_ptr(), ends.len() as libc::nfds_t, -1) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                stop_and_wait(child, libc::SIGTERM)?;
                return Err(error);
            }
        } else if ends.iter().any(|end| end.revents & libc::POLLNVAL != 0) {
            stop_and_wait(child, libc::SIGTERM)?;
            return Err(io::Error::other(
                "release wait lost its notification descriptor",
            ));
        }
    }
}

fn stop_and_wait(child: &mut Child, signal: i32) -> io::Result<ExitStatus> {
    // SAFETY: CHLD is waitable and no other code reaps this process.
    // Even a child that has since exited still reserves this PID.
    if unsafe { libc::kill(child.id() as libc::pid_t, signal) } != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            eprintln!("release cancellation could not signal its child: {error}");
        }
    }
    child.wait()
}

/// Return the real status after the signed publisher has drained its worker.
/// The caller rechecks source, cancellation and status before reading a receipt.
pub(super) fn publisher_status(command: &mut Command) -> Result<ExitStatus> {
    check()?;
    let mut publisher = command.spawn()?;
    Ok(wait(&mut publisher, true)?)
}

fn isolate(command: &mut Command, fd: RawFd) {
    // SAFETY: the fork child uses only async-signal-safe operations before
    // exec. The reader is the sole newly inherited FD. The writer stays CLOEXEC.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 || libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

pub(super) fn run(fd: Option<RawFd>, action: impl FnOnce() -> Result<()>) -> Result<()> {
    arm()?;
    if let Some(fd) = fd {
        adopt(fd)?;
        check()?;
        action()?;
        return check();
    }
    let (reader, mut writer) = std::io::pipe()?;
    writer.write_all(HEADER)?;
    let fd = reader.as_raw_fd();
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--release-owner-fd")
        .arg(fd.to_string())
        .args(std::env::args_os().skip(1));
    isolate(&mut command, fd);
    check()?;
    let mut owner = command.spawn()?;
    drop(reader);
    let status = wait(&mut owner, false)?;
    drop(writer);
    // The real action has already printed its error; keep the public CLI's
    // status without producing a second diagnostic for the same failure.
    if !status.success() {
        std::process::exit(
            status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)),
        );
    }
    check()
}

#[cfg(test)]
#[path = "outer_tests.rs"]
mod tests;
