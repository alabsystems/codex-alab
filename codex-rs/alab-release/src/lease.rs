// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use rustix::fs::FlockOperation;
use rustix::fs::flock;
use std::fs::File;
use std::os::fd::AsFd;
use std::os::fd::BorrowedFd;

pub(crate) struct Lease(File);

impl Lease {
    pub(crate) fn acquire(file: File) -> std::io::Result<Self> {
        flock(&file, FlockOperation::NonBlockingLockExclusive)?;
        Ok(Self(file))
    }
}

impl AsFd for Lease {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        // A concurrent fork can keep the open file description even with CLOEXEC.
        // Unlock when the owner finishes, before its own descriptor closes.
        while flock(&self.0, FlockOperation::Unlock) == Err(rustix::io::Errno::INTR) {}
    }
}
