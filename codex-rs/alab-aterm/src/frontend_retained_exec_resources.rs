// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Only the SAME real GrantedFrontendCommand owns this bounded cleanup scope.
//! It adds no execution/Native admission; caller-owned guards cannot escape it.
use codex_alab_core::native_fd_custody::Guard;
use std::cell::RefCell;
use std::fs::File;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

struct SocketState {
    actual: Option<UnixStream>,
    inherited: bool,
}
/// Existing prepared kernel socket. No decoded fd/path/selector constructor or
/// public descriptor accessor can make it inheritable outside actual command scope.
#[derive(Clone)]
pub struct PreparedFrontendSocket {
    state: Arc<Mutex<SocketState>>,
}
impl PreparedFrontendSocket {
    pub fn from_owned(actual: UnixStream) -> io::Result<Self> {
        if !rustix::io::fcntl_getfd(&actual)?.contains(rustix::io::FdFlags::CLOEXEC) {
            return Err(io::Error::other(
                "frontend resource socket is not close-on-exec",
            ));
        }
        Ok(Self {
            state: Arc::new(Mutex::new(SocketState {
                actual: Some(actual),
                inherited: false,
            })),
        })
    }
    pub fn is_owned(&self) -> io::Result<bool> {
        Ok(self
            .state
            .lock()
            .map_err(|_| io::Error::other("frontend resource socket interrupted"))?
            .actual
            .is_some())
    }
}
struct Rollback {
    state: Arc<Mutex<SocketState>>,
    flags: rustix::io::FdFlags,
}
impl Drop for Rollback {
    fn drop(&mut self) {
        let close = match self.state.lock() {
            Ok(mut state) => {
                let restored = state
                    .actual
                    .as_ref()
                    .is_some_and(|socket| rustix::io::fcntl_setfd(socket, self.flags).is_ok());
                state.inherited = false;
                if restored { None } else { state.actual.take() }
            }
            Err(poisoned) => poisoned.into_inner().actual.take(),
        };
        // Only prepared transport/queued duplicates close on rollback failure;
        // original NamedTempFile owners remain in the actual caller's resource slot.
        drop(close);
    }
}
pub struct FrontendCommandExecScope<'a> {
    _guard: &'a Guard<'static>,
    program: &'a Path,
    cleanup: RefCell<Vec<Rollback>>,
}
impl std::fmt::Debug for FrontendCommandExecScope<'_> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("FrontendCommandExecScope(<same actual granted command>)")
    }
}
impl FrontendCommandExecScope<'_> {
    /// Compare the SAME genuine command's actual program with an already-opened
    /// candidate. This is image identity DATA, never package/Go permission.
    pub fn require_target_image(&self, expected: &Path, opened: &File) -> io::Result<()> {
        let physical = self.program.canonicalize()?;
        let expected = expected.canonicalize()?;
        let held = opened.metadata()?;
        let named = std::fs::metadata(&physical)?;
        if physical != expected
            || !held.is_file()
            || !named.is_file()
            || (
                held.dev(),
                held.ino(),
                held.len(),
                held.mtime(),
                held.mtime_nsec(),
            ) != (
                named.dev(),
                named.ino(),
                named.len(),
                named.mtime(),
                named.mtime_nsec(),
            )
        {
            return Err(io::Error::other(
                "frontend resource support belongs to a different command image",
            ));
        }
        Ok(())
    }

    pub fn inherit_socket(&self, original: &PreparedFrontendSocket) -> io::Result<()> {
        let mut state = original
            .state
            .lock()
            .map_err(|_| io::Error::other("frontend resource socket interrupted"))?;
        if state.inherited {
            return Err(io::Error::other(
                "frontend resource socket already registered",
            ));
        }
        let actual = state
            .actual
            .as_ref()
            .ok_or_else(|| io::Error::other("frontend resource socket was closed"))?;
        let flags = rustix::io::fcntl_getfd(actual)?;
        if !flags.contains(rustix::io::FdFlags::CLOEXEC) {
            return Err(io::Error::other(
                "frontend resource socket already inheritable",
            ));
        }
        // The SCOPE owns cleanup BEFORE changing any inheritance. Stashing a
        // prepared-owner clone or returning an unrelated value cannot evade it.
        self.cleanup.borrow_mut().push(Rollback {
            state: original.state.clone(),
            flags,
        });
        rustix::io::fcntl_setfd(actual, rustix::io::FdFlags::empty())?;
        state.inherited = true;
        Ok(())
    }
}
pub(super) fn within_command(
    guard: &Guard<'static>,
    program: &Path,
    prepare: impl FnOnce(&FrontendCommandExecScope<'_>) -> io::Result<()>,
    execute: impl FnOnce() -> io::Error,
) -> io::Error {
    let scope = FrontendCommandExecScope {
        _guard: guard,
        program,
        cleanup: RefCell::new(Vec::new()),
    };
    if let Err(error) = prepare(&scope) {
        drop(scope);
        return error;
    }
    let error = execute();
    // Scope-owned registered rollback completes before the caller's SAME
    // original executing guard releases, regardless of callback ownership.
    drop(scope);
    error
}

#[cfg(test)]
#[path = "frontend_retained_exec_resources_tests.rs"]
mod tests;
