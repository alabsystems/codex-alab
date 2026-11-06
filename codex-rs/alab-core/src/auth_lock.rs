// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The file lock aterm takes on a Codex login, taken by codex-alab's refresh too
//! (`ALAB(auth-refresh-lock)`).
//!
//! A ChatGPT refresh token is good for one refresh: the provider answers it with a new one and
//! refuses the old one from then on. aterm refreshes the logins of its account pool itself (its
//! usage reads), in the same `auth.json` files codex-alab sessions read and refresh, so two
//! refreshes of one login that both send its token leave one of them refused, and that login is
//! signed out of aterm's rotation. aterm orders its refreshes of a login by an exclusive
//! `flock(2)` on `auth.json.lock` beside its `auth.json` (`<file>.lock` beside a login at
//! `CODEX_AUTH_PATH`), held from the read before its request to the write of the rotated tokens
//! (aterm's `codex_app_login::AuthLock`). Upstream orders refreshes only within one process (the
//! auth manager's `refresh_lock`, `AUTH_STORAGE_COMMIT` around the write).
//!
//! codex-alab takes the same lock over the same span ([`admit`]): it waits for the lock, reads
//! the login again once it holds it, sends nothing where the file no longer holds the refresh
//! token it was about to send (another process rotated it meanwhile), and otherwise keeps the
//! lock across its request and the write of the answer. `flock` locks belong to an open file
//! description, so two codex auth managers in one process, each with its own open of the lock
//! file, are ordered by it too.
//!
//! Off unix this is a no-op: codex-alab's packaged builds are macOS and Linux ones, and
//! `O_NOFOLLOW`, which keeps a planted link from redirecting the lock, has no std equivalent there.

use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

/// The suffix of the lock file beside a login's `auth.json`, the one aterm uses.
pub const LOCK_SUFFIX: &str = ".lock";

/// How long a refresh waits for the lock before returning a transient failure.
///
/// aterm holds the lock for at most one usage read (its `USAGE_READ_WAIT`, 10 s, bounds both its
/// request and its write), and a sign-in's write waits at most 15 s for it; another codex holds
/// it for one refresh request and write. A refresh waits inside a turn (a proactive refresh
/// before a request, or the recovery from a 401), so a holder that never lets go (a stopped
/// process, a request hung on a dead network) must not stall a turn for good. 30 s is three of
/// aterm's longest holds, so a refresh aterm has in flight finishes first, and a wedged holder
/// costs a turn 30 s before codex returns a retryable error. It must never send a
/// single-use token while another process may be spending it.
pub const WAIT: Duration = Duration::from_secs(30);

/// The pause between looks at a lock another process holds (aterm's own pause).
#[cfg(unix)]
const POLL: Duration = Duration::from_millis(20);

/// `auth_file`'s lock file: its name with [`LOCK_SUFFIX`] after it, in the same directory.
pub fn lock_path(auth_file: &Path) -> PathBuf {
    let mut name = auth_file
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_else(|| "auth.json".into());
    name.push(LOCK_SUFFIX);
    auth_file.with_file_name(name)
}

/// An exclusive `flock(2)` on a login's lock file ([`lock_path`]), released by `LOCK_UN` on drop
/// so a child spawned meanwhile keeps no hold on it.
#[derive(Debug)]
pub struct AuthLock {
    #[cfg(unix)]
    file: std::fs::File,
}

#[cfg(unix)]
impl AuthLock {
    /// The lock file beside `auth_file`, opened (made `0600` where missing) without following a
    /// link. Refused where its name is a link (`ELOOP`) or names no regular file.
    fn open(auth_file: &Path) -> std::io::Result<std::fs::File> {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut options = std::fs::OpenOptions::new();
        options
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let file = options.open(lock_path(auth_file))?;
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the login's lock file is no regular file",
            ));
        }
        Ok(file)
    }

    /// The lock beside `auth_file` if no one holds it now: `Ok(None)` where another open of it
    /// holds it, an error where it is refused ([`AuthLock::open`]) or cannot be taken.
    pub fn try_take(auth_file: &Path) -> std::io::Result<Option<AuthLock>> {
        let file = Self::open(auth_file)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(AuthLock { file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(err)) => Err(err),
        }
    }

    /// The lock beside `auth_file`, waited for up to `wait` without blocking the runtime:
    /// `Ok(None)` where it was still held then, an error where it is refused or cannot be taken.
    pub async fn take(auth_file: &Path, wait: Duration) -> std::io::Result<Option<AuthLock>> {
        let deadline = tokio::time::Instant::now() + wait;
        let file = Self::open(auth_file)?;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Some(AuthLock { file })),
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(err)) => return Err(err),
            }
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            tokio::time::sleep(left.min(POLL)).await;
        }
    }
}

#[cfg(unix)]
impl Drop for AuthLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// What a refresh keeps while it sends and writes: the login's lock, or nothing where no lock
/// applies.
#[derive(Debug)]
pub struct Hold {
    lock: Option<AuthLock>,
}

impl Hold {
    /// A hold with no lock in it: the refresh goes on as upstream's does.
    pub fn unlocked() -> Hold {
        Hold { lock: None }
    }

    /// Whether this hold has the login's lock.
    pub fn is_locked(&self) -> bool {
        self.lock.is_some()
    }
}

/// Whether a refresh sends.
#[derive(Debug)]
pub enum Admission {
    /// The login cannot be locked. Do not send or change its credentials; the
    /// caller can retry after the transient storage or contention failure.
    Unavailable(std::io::Error),
    /// The file no longer holds the token the refresh was about to send (`rotated` said so,
    /// under the lock, and dealt with what it holds now). Send nothing.
    Reused,
    /// Send, keeping this until the answer is written.
    Send(Hold),
}

/// Admits a refresh of the login in `auth_file` (`None` where the login is not a file).
///
/// Waits up to `wait` for the login's lock, then asks `rotated` (the caller's read of the file
/// under the lock) whether the file still holds the refresh token the caller was about to send:
/// [`Admission::Reused`] where it does not, else [`Admission::Send`] with the lock. Where the lock
/// stays held for `wait`, is refused, or cannot be taken, returns [`Admission::Unavailable`]
/// without calling `rotated` or permitting a token request.
pub async fn admit<F, Fut>(auth_file: Option<&Path>, wait: Duration, rotated: F) -> Admission
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = bool>,
{
    let Some(auth_file) = auth_file else {
        return Admission::Send(Hold::unlocked());
    };
    #[cfg(unix)]
    {
        match AuthLock::take(auth_file, wait).await {
            Ok(Some(lock)) => {
                if rotated().await {
                    Admission::Reused
                } else {
                    Admission::Send(Hold { lock: Some(lock) })
                }
            }
            Ok(None) => Admission::Unavailable(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Login refresh is busy in another process; retry later.",
            )),
            Err(err) => Admission::Unavailable(err),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (auth_file, wait, rotated);
        Admission::Send(Hold::unlocked())
    }
}

#[cfg(all(test, unix))]
#[path = "auth_lock_tests.rs"]
mod tests;
