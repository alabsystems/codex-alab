// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One census of actual parent standard descriptions and global saved stderr.
//! These owned CLOEXEC duplicates keep the original descriptions alive; they
//! do not close writers, retire frontend IO, bind another process, or grant
//! Native admission. The editor and ordinary C0 owners supply their own real
//! input/mode/writer fences before relying on this local resource observation.
use super::TerminalStderrGuard;
use std::io;
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;
use std::os::fd::BorrowedFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::fd::RawFd;

fn duplicate_standard(descriptor: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: fcntl validates the supplied native standard descriptor; closed
    // stdin/stdout/stderr returns EBADF, without constructing an invalid borrow.
    let duplicate = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful F_DUPFD_CLOEXEC returned one newly owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}

pub(in crate::tui) struct Captured {
    process: u32,
    parent: [OwnedFd; 3],
    saved_stderr: Option<OwnedFd>,
    owner_active: bool,
    guard_active: bool,
}

impl Captured {
    /// Borrow the genuine current guard. No DATA, descriptor selector, editor
    /// stand-in, or caller boolean can construct this census. Capturing creates
    /// no writer exclusion beyond the existing synchronous FD-custody guard.
    pub(in crate::tui) fn capture(actual: &TerminalStderrGuard) -> io::Result<Self> {
        let _custody = codex_alab_core::native_fd_custody::receiving()?;
        // Keep the SAME original stderr mutex through every descriptor clone.
        // On refusal only new duplicates drop; the original state is untouched.
        #[cfg(target_os = "macos")]
        let state = super::lock_state()?;
        #[cfg(target_os = "macos")]
        let (owner_active, saved_stderr) = (
            state.owner_active,
            state
                .saved_stderr
                .as_ref()
                .map(|actual| duplicate_standard(actual.as_raw_fd()))
                .transpose()?,
        );
        #[cfg(not(target_os = "macos"))]
        let (owner_active, saved_stderr) = (false, None);
        let parent = [
            duplicate_standard(libc::STDIN_FILENO)?,
            duplicate_standard(libc::STDOUT_FILENO)?,
            duplicate_standard(libc::STDERR_FILENO)?,
        ];
        Ok(Self {
            process: std::process::id(),
            parent,
            saved_stderr,
            owner_active,
            guard_active: actual.active,
        })
    }

    /// Process equality only, never Native Current, enrolled-role or Go proof.
    pub(in crate::tui) fn check_process(&self) -> io::Result<()> {
        if self.process != std::process::id() {
            return Err(io::Error::other(
                "original stdio census belongs to another process",
            ));
        }
        Ok(())
    }

    pub(in crate::tui) fn parent_descriptors(&self) -> [BorrowedFd<'_>; 3] {
        [
            self.parent[0].as_fd(),
            self.parent[1].as_fd(),
            self.parent[2].as_fd(),
        ]
    }

    pub(in crate::tui) fn saved_stderr(&self) -> Option<BorrowedFd<'_>> {
        self.saved_stderr.as_ref().map(OwnedFd::as_fd)
    }

    /// Original state DATA. Absence is captured from the native Option, never
    /// supplied as an empty/default replacement by a decoder or another owner.
    pub(in crate::tui) fn stderr_state_data(&self) -> (bool, bool) {
        (self.owner_active, self.guard_active)
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "retained_terminal_stdio_census_tests.rs"]
mod tests;
