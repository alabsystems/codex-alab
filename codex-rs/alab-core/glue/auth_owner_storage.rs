// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A negative canonical-owner marker never authorizes a slot or credential read.

use super::AuthCredentialsStoreMode;
use super::AuthDotJson;
use super::AuthStorageBackend;
use codex_alab_core::managed_auth;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

pub(super) fn check_file(path: &Path) -> std::io::Result<()> {
    if codex_alab_core::is_alab_binary() {
        managed_auth::refuse_unadmitted_owner(path)?;
    }
    Ok(())
}

pub(super) fn wrap(
    storage: Arc<dyn AuthStorageBackend>,
    home: &Path,
    mode: AuthCredentialsStoreMode,
) -> Arc<dyn AuthStorageBackend> {
    wrap_for(
        codex_alab_core::is_alab_binary(),
        storage,
        super::get_auth_file(home),
        mode,
    )
}

fn wrap_for(
    alab: bool,
    storage: Arc<dyn AuthStorageBackend>,
    path: PathBuf,
    mode: AuthCredentialsStoreMode,
) -> Arc<dyn AuthStorageBackend> {
    if !alab || mode == AuthCredentialsStoreMode::Ephemeral {
        return storage;
    }
    Arc::new(Guard { storage, path })
}

struct Guard {
    storage: Arc<dyn AuthStorageBackend>,
    path: PathBuf,
}
impl std::fmt::Debug for Guard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeOwnerGuard(<private>)")
    }
}
impl Guard {
    fn check(&self) -> std::io::Result<()> {
        managed_auth::refuse_unadmitted_owner(&self.path)
    }
}
impl AuthStorageBackend for Guard {
    fn persistent_home(&self) -> Option<&Path> {
        self.storage.persistent_home()
    }
    fn lock_for_write(
        &self,
    ) -> std::io::Result<(std::sync::MutexGuard<'static, ()>, Option<std::fs::File>)> {
        self.check()?;
        let held = self.storage.lock_for_write()?;
        self.check()?;
        Ok(held)
    }
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        self.check()?;
        self.storage.load()
    }
    fn load_account_grant(&self) -> std::io::Result<Option<AuthDotJson>> {
        self.check()?;
        self.storage.load_account_grant()
    }
    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        self.check()?;
        self.storage.save(auth)
    }
    fn compare_and_save(&self, expected: &AuthDotJson, auth: &AuthDotJson) -> std::io::Result<()> {
        self.check()?;
        self.storage.compare_and_save(expected, auth)
    }
    fn delete(&self) -> std::io::Result<bool> {
        self.check()?;
        self.storage.delete()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    #[derive(Debug, Default)]
    struct Backend(AtomicUsize);
    impl AuthStorageBackend for Backend {
        fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
        fn save(&self, _: &AuthDotJson) -> std::io::Result<()> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn delete(&self) -> std::io::Result<bool> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(false)
        }
    }

    #[test]
    fn owner_storage_presence_blocks_read_fallback_write_and_delete_without_accessing_backend() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        let marker = managed_auth::owner_path(&path);
        let backend = Arc::new(Backend::default());
        let guarded = wrap_for(true, backend.clone(), path, AuthCredentialsStoreMode::Auto);
        assert!(guarded.load().unwrap().is_none());
        assert_eq!(backend.0.load(Ordering::Relaxed), 1);
        std::fs::write(&marker, managed_auth::OWNER_MARKER).unwrap();
        for contents in [
            managed_auth::OWNER_MARKER,
            b"invalid owner marker".as_slice(),
        ] {
            std::fs::write(&marker, contents).unwrap();
            assert!(guarded.load().is_err());
            assert!(guarded.load_account_grant().is_err());
            assert!(
                guarded
                    .save(&serde_json::from_str::<AuthDotJson>("{}").unwrap())
                    .is_err()
            );
            assert!(
                guarded
                    .compare_and_save(
                        &serde_json::from_str::<AuthDotJson>("{}").unwrap(),
                        &serde_json::from_str::<AuthDotJson>("{}").unwrap()
                    )
                    .is_err()
            );
            assert!(guarded.delete().is_err());
        }
        assert_eq!(
            backend.0.load(Ordering::Relaxed),
            1,
            "no keyring/file fallback is reached"
        );
        std::fs::remove_file(marker).unwrap();
        assert!(
            guarded.load().unwrap().is_none(),
            "absence is checked afresh; no cached ownership grant"
        );
        assert_eq!(backend.0.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn owner_storage_does_not_convert_ephemeral_or_upstream_backends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(managed_auth::owner_path(&path), managed_auth::OWNER_MARKER).unwrap();
        for (alab, mode) in [
            (true, AuthCredentialsStoreMode::Ephemeral),
            (false, AuthCredentialsStoreMode::File),
        ] {
            let backend = Arc::new(Backend::default());
            let storage = wrap_for(alab, backend.clone(), path.clone(), mode);
            assert!(storage.load().unwrap().is_none());
            assert_eq!(backend.0.load(Ordering::Relaxed), 1);
        }
    }
}
