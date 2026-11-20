// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Cancellation of a single publisher-selected worker, including while it is
//! parsing a live compiler pipe. Its child owner remains in the publisher's
//! original cohort until the protected command has been reaped and drained.

use super::Protected;
use crate::Result;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

#[derive(Debug)]
pub struct WorkerCancelled(i32);

impl WorkerCancelled {
    pub fn exit_code(&self) -> i32 {
        128 + self.0
    }
}

impl fmt::Display for WorkerCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native compiler worker received signal {}", self.0)
    }
}

impl std::error::Error for WorkerCancelled {}

pub struct Worker {
    reader: UnixStream,
    writer: Arc<UnixStream>,
    signal: Arc<AtomicI32>,
    handlers: Vec<signal_hook::SigId>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        for id in self.handlers.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

impl Worker {
    pub fn new() -> Result<Self> {
        let (reader, writer) = UnixStream::pair()?;
        let mut worker = Self {
            reader,
            writer: Arc::new(writer),
            signal: Arc::new(AtomicI32::new(0)),
            handlers: Vec::new(),
        };
        // All children start after the handlers are armed. A signal already
        // pending in a launcher's blocked mask must retire the worker as well.
        let mut mask = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
        // SAFETY: sigemptyset initializes the complete set.
        unsafe { libc::sigemptyset(mask.as_mut_ptr()) };
        // SAFETY: the set was fully initialized above.
        let mut mask = unsafe { mask.assume_init() };
        // An explicitly ignored SIGCHLD can auto-reap the helper before the
        // owning wait. The worker is the lifetime owner for its own children.
        // SAFETY: SIG_DFL is a valid disposition for SIGCHLD.
        if unsafe { libc::signal(libc::SIGCHLD, libc::SIG_DFL) } == libc::SIG_ERR {
            return Err(io::Error::last_os_error().into());
        }
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGQUIT] {
            let writer = Arc::clone(&worker.writer);
            let signal = Arc::clone(&worker.signal);
            // SAFETY: the callback only updates a lock-free atomic and performs
            // async-signal-safe shutdown on a descriptor retained by the closure.
            let id = unsafe {
                signal_hook::low_level::register(sig, move || {
                    let _ = signal.compare_exchange(0, sig, Ordering::SeqCst, Ordering::SeqCst);
                    libc::shutdown(writer.as_raw_fd(), libc::SHUT_WR);
                })?
            };
            worker.handlers.push(id);
            // SAFETY: sig is a valid signal and mask is an initialized set.
            unsafe { libc::sigaddset(&mut mask, sig) };
        }
        // SAFETY: mask is initialized; this dedicated worker process starts
        // single-threaded. No previous mask is needed after worker completion.
        let rc = unsafe { libc::pthread_sigmask(libc::SIG_UNBLOCK, &mask, std::ptr::null_mut()) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc).into());
        }
        worker.check()?;
        Ok(worker)
    }

    pub fn check(&self) -> Result<()> {
        let signal = self.signal.load(Ordering::SeqCst);
        if signal != 0 {
            return Err(WorkerCancelled(signal).into());
        }
        Ok(())
    }

    /// Abandon the current stage after a rejected streamed output record. Its
    /// original parse error remains authoritative while its child owner drains.
    pub fn retire(&self) -> io::Result<()> {
        self.writer.shutdown(Shutdown::Write)
    }

    pub fn command(
        &self,
        protected: &Protected,
        program: &OsStr,
        args: &[OsString],
        certified: bool,
    ) -> Result<Command> {
        self.owned_command(protected, program, args, certified, "compiler")
    }

    pub fn disposable_command(
        &self,
        protected: &Protected,
        program: &OsStr,
        args: &[OsString],
    ) -> Result<Command> {
        self.owned_command(protected, program, args, false, "disposable")
    }

    fn owned_command(
        &self,
        protected: &Protected,
        program: &OsStr,
        args: &[OsString],
        certified: bool,
        mode: &str,
    ) -> Result<Command> {
        self.check()?;
        let reader = self.reader.try_clone()?;
        let mut owner = Command::new("/usr/bin/python3");
        owner
            .args([
                "-I",
                "-B",
                "-S",
                "-c",
                include_str!("../../../alab/native_worker.py"),
                include_str!("../../../alab/command_guard.py"),
                if certified {
                    include_str!("../../../alab/trust_memory.py")
                } else {
                    ""
                },
            ])
            .arg(reader.as_raw_fd().to_string())
            .arg(mode)
            .arg("/usr/bin/sandbox-exec");
        // SAFETY: fcntl is async-signal-safe; the command owns the duplicate
        // through spawn. Only the child changes its descriptor flags, and the
        // fixed owner restores CLOEXEC before launching any source command.
        unsafe {
            owner.pre_exec(move || {
                if libc::fcntl(reader.as_raw_fd(), libc::F_SETFD, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let profile = if certified {
            protected.certified_profile()?
        } else {
            protected.profile.clone()
        };
        Ok(protected.profile_command(owner, &profile, program, args))
    }
}
