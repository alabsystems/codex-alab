// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Process-local exclusion between ancillary FD receipt and raw exec.
//!
//! A synchronous receiver holds this guard from before recvmsg until every
//! received descriptor has an owner and is CLOEXEC, including rejection paths.
//! Raw exec holds the same guard before making custody inheritable through the
//! exec attempt and failure cleanup. The guard grants no native admission.
use std::io;
use std::sync::Mutex;
use std::sync::MutexGuard;

static EXCLUSION: Mutex<()> = Mutex::new(());

/// An opaque exclusion guard. Keep it in the synchronous critical section;
/// it cannot move to another thread. Never hold it across an await.
#[must_use = "retain FD exclusion until receipt or exec cleanup is complete"]
pub struct Guard<'a> {
    _held: MutexGuard<'a, ()>,
}

fn acquire(exclusion: &Mutex<()>) -> io::Result<Guard<'_>> {
    exclusion
        .lock()
        .map(|held| Guard { _held: held })
        .map_err(|_| io::Error::other("native descriptor custody was interrupted"))
}

/// Exclude raw exec while synchronously receiving and securing every FD.
///
/// # Errors
/// A previous critical section panicked, leaving descriptor custody uncertain.
pub fn receiving() -> io::Result<Guard<'static>> {
    acquire(&EXCLUSION)
}

/// Exclude ancillary receipt through inheritance, exec, and failure cleanup.
/// This does not close or select descriptors and grants no execution permission.
///
/// # Errors
/// A previous critical section panicked, leaving descriptor custody uncertain.
pub fn executing() -> io::Result<Guard<'static>> {
    acquire(&EXCLUSION)
}

#[cfg(test)]
#[path = "native_fd_custody_tests.rs"]
mod tests;
