// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

/// Only the native file-descriptor readers can positively establish EOF.
#[derive(Clone, Debug, Default)]
pub struct NativeOutputEof(Option<Arc<AtomicU8>>);

impl NativeOutputEof {
    pub(crate) fn new() -> Self {
        Self(cfg!(unix).then(|| Arc::new(AtomicU8::new(0))))
    }

    pub(crate) fn finish(&self, streams: u8) {
        if let Some(state) = &self.0 {
            state.fetch_or(streams, Ordering::Release);
        }
    }

    pub fn observed(&self) -> Option<bool> {
        self.0
            .as_ref()
            .map(|state| state.load(Ordering::Acquire) & 7 == 3)
    }

    pub fn forced_before_exit(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|state| state.load(Ordering::Acquire) & 8 != 0)
    }

    pub(crate) fn interrupt(&self, root_already_exited: bool) {
        if let Some(state) = &self.0 {
            let mut bits = state.load(Ordering::Acquire);
            loop {
                let mut next = bits;
                if bits & 3 != 3 {
                    next |= 4;
                }
                if !root_already_exited {
                    next |= 8;
                }
                if bits == next {
                    return;
                }
                match state.compare_exchange_weak(bits, next, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => return,
                    Err(current) => bits = current,
                }
            }
        }
    }

    #[cfg(unix)]
    pub(crate) fn pty_hung_up(fd: std::os::fd::RawFd, err: &std::io::Error) -> bool {
        if err.raw_os_error() != Some(libc::EIO) {
            return false;
        }
        let mut status = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // The original PTY master is held across this nonblocking hangup check.
        unsafe { libc::poll(&mut status, 1, 0) > 0 && status.revents & libc::POLLHUP != 0 }
    }
}

impl crate::ProcessHandle {
    pub fn native_output_eof(&self) -> NativeOutputEof {
        self.native_output_eof.clone()
    }

    pub(crate) fn with_native_output_eof(mut self, proof: NativeOutputEof) -> Self {
        self.native_output_eof = proof;
        self
    }
}

#[cfg(all(test, unix))]
#[path = "native_output_eof_tests.rs"]
mod tests;
