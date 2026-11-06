//! Serializes cooperating processes' credential commits, including compare-and-save.
//! The stable sidecar is never renamed or removed; network requests never hold it.

use super::*;
use std::fs::OpenOptions;
use std::fs::TryLockError;
use std::time::Duration;
use std::time::Instant;

pub(super) fn wrap(
    inner: Arc<dyn AuthStorageBackend>,
    home: PathBuf,
    mode: AuthCredentialsStoreMode,
) -> Arc<dyn AuthStorageBackend> {
    if mode == AuthCredentialsStoreMode::Ephemeral {
        return inner;
    }
    Arc::new(CommittedStorage { inner, home, mode })
}

#[derive(Debug)]
struct CommittedStorage {
    inner: Arc<dyn AuthStorageBackend>,
    home: PathBuf,
    mode: AuthCredentialsStoreMode,
}

impl CommittedStorage {
    fn lock(&self) -> std::io::Result<File> {
        let auth_file = get_auth_file(&self.home);
        if self.mode == AuthCredentialsStoreMode::File && codex_alab_core::is_alab_binary() { return codex_alab_core::home::lock_auth_file(&auth_file); } // ALAB(auth-path): lock the original private file slot without following its final symlink
        // Keyring commits can unlink an intermediate symlink used by another
        // home, so resolvable chains must share the target's commit lock.
        let path = match storage_file::resolve_auth_file(auth_file.clone()) {
            Ok(path) => path,
            // Unresolvable fallback links can still be removed by keyring commits.
            Err(_) if self.mode == AuthCredentialsStoreMode::Keyring => auth_file,
            Err(error) => return Err(error),
        };
        let path = if codex_alab_core::is_alab_binary() { codex_alab_core::home::auth_commit_lock_path(&path) } else { path.with_extension("json.lock") }; // ALAB(auth-refresh-lock): Auto fallback and keyring commits share the inner lock, distinct from the pool's outer refresh hold.
        std::fs::create_dir_all(
            path.parent()
                .ok_or_else(|| std::io::Error::other("credential lock has no parent"))?,
        )?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "credential commit lock timed out",
                    ));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl AuthStorageBackend for CommittedStorage {
    fn persistent_home(&self) -> Option<&Path> {
        self.inner.persistent_home()
    }

    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        self.inner.load()
    }
    fn load_account_grant(&self) -> std::io::Result<Option<AuthDotJson>> {
        self.inner.load_account_grant()
    }
    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        let _guard = self.lock()?;
        self.inner.save(auth)
    }
    fn compare_and_save(&self, expected: &AuthDotJson, auth: &AuthDotJson) -> std::io::Result<()> {
        let _guard = self.lock()?;
        self.inner.compare_and_save(expected, auth)
    }
    fn delete(&self) -> std::io::Result<bool> {
        // A missing file-store directory contains no credentials to delete.
        // Linearize this no-op before any concurrent creator, without creating
        // a directory just to leave a lock behind. Secure stores may still have
        // credentials even when the directory is absent and must take the lock.
        if self.mode == AuthCredentialsStoreMode::File && !codex_alab_core::home::auth_commit_home(&self.home).try_exists()? { // ALAB(auth-path): an account slot can exist outside a missing shared CODEX_HOME
            return Ok(false);
        }
        let _guard = self.lock()?;
        self.inner.delete()
    }
}

#[cfg(test)]
#[path = "storage_commit_tests.rs"]
mod tests;
