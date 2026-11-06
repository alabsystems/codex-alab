// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(auth-path) glue tests for `codex-login` (`auth::storage::alab_auth_path_tests`):
//! when `codex_alab_core::is_alab_binary()` and `CODEX_AUTH_PATH` names an admitted file,
//! persistent credential modes (`File`, `Auto`, `Keyring` with `Direct` or `Secrets`) read and
//! atomically update `CODEX_AUTH_PATH` without consulting `CODEX_HOME`'s shared `SecretsManager`
//! or unlinking `CODEX_AUTH_PATH` into the OS keyring, in both Enhanced and Vanilla modes. Only
//! privately spawned test children activate ALab's switch or set `CODEX_AUTH_PATH`; the shared
//! test runner preserves upstream behavior.

use super::*;
use codex_keyring_store::tests::MockKeyringStore;
use pretty_assertions::assert_eq;
use std::process::Command;
use std::process::Stdio;

const CHILD_CASE_ENV: &str = "ALAB_AUTH_STORAGE_CHILD_CASE";
const CHILD_COMPLETE: &str = "ALAB_AUTH_STORAGE_CHILD_OK";

fn api_key_doc(key: &str) -> AuthDotJson {
    AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some(key.to_owned()),
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    }
}

fn spawn_case(case: &str, with_auth_path: bool) -> std::io::Result<()> {
    let (_, local_mod) = module_path!().split_once("::").ok_or_else(|| {
        std::io::Error::other("expected nested module path for auth_path_storage_tests")
    })?;
    let exact = format!("{local_mod}::auth_path_storage_child");
    let world = tempfile::Builder::new()
        .prefix("alab-auth-storage-")
        .tempdir()?;
    let codex_home = world.path().join("codex-home");
    if case != "missing-home-delete" {
        std::fs::create_dir_all(&codex_home)?;
    }
    let pool_auth = world.path().join("pool").join("acct-b").join("auth.json");

    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD_CASE_ENV, case)
        .env("HOME", world.path())
        .env("CODEX_HOME", &codex_home)
        .env("TMPDIR", world.path())
        .env("USER", "alab-test")
        .env("LOGNAME", "alab-test")
        .env("PATH", "/usr/bin:/bin")
        .current_dir(world.path())
        .stdin(Stdio::null());
    if with_auth_path {
        cmd.env(codex_alab_core::home::AUTH_PATH_ENV, &pool_auth);
    }
    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "auth storage child ({case}) failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains(CHILD_COMPLETE),
        "auth storage child ({case}) did not complete:\n{stdout}"
    );
    Ok(())
}

#[test]
fn codex_auth_path_isolates_persistent_storage_from_shared_keyring_and_secrets()
-> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    spawn_case("override", true)?;
    spawn_case("default", false)?;
    assert!(!codex_alab_core::active());
    Ok(())
}

#[test]
fn codex_auth_path_deletes_an_existing_override_without_a_codex_home() -> std::io::Result<()> {
    spawn_case("missing-home-delete", true)
}

#[cfg(unix)]
#[test]
fn codex_auth_path_rejects_a_symlink_slot_before_creating_a_target_lock() -> std::io::Result<()> {
    spawn_case("symlink-slot", true)
}

#[cfg(unix)]
#[test]
fn unpinned_auto_fallback_commits_while_the_pool_refresh_lock_is_held() -> std::io::Result<()> {
    spawn_case("auto-fallback-commit", false)
}

#[test]
#[expect(
    clippy::print_stdout,
    reason = "exact completion marker for the parent test process"
)]
fn auth_path_storage_child() -> std::io::Result<()> {
    let Ok(case) = std::env::var(CHILD_CASE_ENV) else {
        return Ok(());
    };
    codex_alab_core::activate();
    let codex_home = PathBuf::from(
        std::env::var_os("CODEX_HOME")
            .ok_or_else(|| std::io::Error::other("missing CODEX_HOME"))?,
    );

    match case.as_str() {
        "missing-home-delete" => {
            let pool_auth = codex_alab_core::home::auth_file(&codex_home);
            assert!(!codex_home.exists());
            for enhanced in [true, false] {
                codex_alab_core::set_enhanced(enhanced);
                for mode in [
                    AuthCredentialsStoreMode::File,
                    AuthCredentialsStoreMode::Auto,
                    AuthCredentialsStoreMode::Keyring,
                ] {
                    codex_alab_core::home::save_auth_bytes(
                        &pool_auth,
                        serde_json::to_string(&api_key_doc("delete-private-slot"))?.as_bytes(),
                    )?;
                    let store = create_auth_storage_with_store(
                        codex_home.clone(),
                        mode,
                        Arc::new(MockKeyringStore::default()),
                        AuthKeyringBackendKind::Direct,
                    );
                    assert_eq!(
                        codex_alab_core::home::auth_commit_home(&codex_home),
                        pool_auth.parent().unwrap_or_else(|| panic!("parent"))
                    );
                    assert!(
                        store.delete()?,
                        "the override exists even without CODEX_HOME: {mode:?}"
                    );
                    assert!(!pool_auth.exists());
                    assert!(
                        !codex_home.exists(),
                        "delete must not create shared CODEX_HOME"
                    );
                    assert!(
                        !store.delete()?,
                        "a second delete has no credentials to remove"
                    );
                }
            }
        }
        #[cfg(unix)]
        "symlink-slot" => {
            use std::os::unix::fs::MetadataExt as _;
            use std::os::unix::fs::PermissionsExt as _;
            let pool_auth = codex_alab_core::home::auth_file(&codex_home);
            std::fs::create_dir_all(pool_auth.parent().unwrap_or_else(|| panic!("parent")))?;
            let target = codex_home.join("outside.json");
            std::fs::write(&target, b"outside-original")?;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640))?;
            let before = std::fs::metadata(&target)?;
            std::os::unix::fs::symlink(&target, &pool_auth)?;
            for enhanced in [true, false] {
                codex_alab_core::set_enhanced(enhanced);
                let store = create_auth_storage_with_store(
                    codex_home.clone(),
                    AuthCredentialsStoreMode::Auto,
                    Arc::new(MockKeyringStore::default()),
                    AuthKeyringBackendKind::Secrets,
                );
                assert!(store.save(&api_key_doc("refused-symlink-write")).is_err());
                assert!(store.delete().is_err());
                assert_eq!(std::fs::read(&target)?, b"outside-original");
                let after = std::fs::metadata(&target)?;
                assert_eq!((after.ino(), after.mode()), (before.ino(), before.mode()));
                assert!(
                    std::fs::symlink_metadata(&pool_auth)?
                        .file_type()
                        .is_symlink()
                );
                assert!(
                    !codex_alab_core::home::auth_commit_lock_path(&target).exists(),
                    "refuse before following the target for its commit lock"
                );
                assert!(!target.with_extension("json.lock").exists());
                assert!(!codex_alab_core::home::auth_commit_lock_path(&pool_auth).exists());
            }
        }
        #[cfg(unix)]
        "auto-fallback-commit" => {
            assert!(!codex_alab_core::home::has_admitted_auth_path());
            let auth_file = get_auth_file(&codex_home);
            for enhanced in [true, false] {
                codex_alab_core::set_enhanced(enhanced);
                let old = api_key_doc("private-fallback-old");
                let new = api_key_doc("private-fallback-new");
                codex_alab_core::home::save_auth_bytes(
                    &auth_file,
                    serde_json::to_string(&old)?.as_bytes(),
                )?;
                let mock = Arc::new(MockKeyringStore::default());
                let store = create_auth_storage_with_store(
                    codex_home.clone(),
                    AuthCredentialsStoreMode::Auto,
                    mock.clone(),
                    AuthKeyringBackendKind::Direct,
                );
                assert_eq!(
                    store.load()?,
                    Some(old.clone()),
                    "actual Auto fallback read"
                );
                let pool = codex_alab_core::auth_lock::AuthLock::try_take(&auth_file)?
                    .unwrap_or_else(|| panic!("private pool lock free"));
                let write = store.lock_for_write()?;
                let serialized = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(codex_home.join("auth.lock"))?;
                assert!(matches!(
                    serialized.try_lock(),
                    Err(std::fs::TryLockError::WouldBlock)
                ));
                store.compare_and_save(&old, &new)?;
                assert_eq!(store.load()?, Some(new));
                assert!(mock.contains(&compute_store_key(&codex_home)?));
                assert!(
                    !auth_file.exists(),
                    "preserve Auto's successful keyring migration"
                );
                assert!(codex_alab_core::auth_lock::AuthLock::try_take(&auth_file)?.is_none());
                let commit = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(codex_alab_core::home::auth_commit_lock_path(&auth_file))?;
                commit.try_lock().map_err(std::io::Error::from)?;
                drop(commit);
                drop(write);
                serialized.try_lock().map_err(std::io::Error::from)?;
                assert!(codex_alab_core::auth_lock::AuthLock::try_take(&auth_file)?.is_none());
                drop(serialized);
                drop(pool);
            }
        }
        "override" => {
            assert!(codex_alab_core::home::has_admitted_auth_path());
            let pool_auth = PathBuf::from(
                std::env::var_os(codex_alab_core::home::AUTH_PATH_ENV)
                    .ok_or_else(|| std::io::Error::other("missing CODEX_AUTH_PATH"))?,
            );

            // Seed shared CODEX_HOME's Direct and Secrets keyring backends with Account A.
            let keyring: Arc<dyn KeyringStore> = Arc::new(MockKeyringStore::default());
            let shared_direct = DirectKeyringAuthStorage::new(
                codex_home.clone(),
                Arc::clone(&keyring),
                AuthCredentialsStoreMode::Keyring,
            );
            shared_direct.save(&api_key_doc("shared-direct-account-a"))?;
            // Also seed the default (non-override) `codex_home` keyring slot directly so a bug
            // ignoring `store_key_target` would leak `default-home-direct-a`.
            let default_canon = codex_home
                .canonicalize()
                .unwrap_or_else(|_| codex_home.clone());
            let mut default_hasher = Sha256::new();
            default_hasher.update(default_canon.to_string_lossy().as_bytes());
            let default_hex = format!("{:x}", default_hasher.finalize());
            let default_key = format!("cli|{}", &default_hex[..16]);
            keyring
                .save(
                    KEYRING_SERVICE,
                    &default_key,
                    &serde_json::to_string(&api_key_doc("default-home-direct-a"))?,
                )
                .map_err(std::io::Error::from)?;
            let shared_secrets = SecretsKeyringAuthStorage::new(
                codex_home.clone(),
                Arc::clone(&keyring),
                AuthCredentialsStoreMode::Keyring,
            );
            shared_secrets.save(&api_key_doc("shared-secrets-account-a"))?;

            // Seed CODEX_AUTH_PATH with Account B.
            codex_alab_core::home::save_auth_bytes(
                &pool_auth,
                serde_json::to_string_pretty(&api_key_doc("pool-account-b"))?.as_bytes(),
            )?;

            for (mode, backend_kind, refreshed_key) in [
                (
                    AuthCredentialsStoreMode::Auto,
                    AuthKeyringBackendKind::Secrets,
                    "pool-account-b-refreshed-1",
                ),
                (
                    AuthCredentialsStoreMode::Auto,
                    AuthKeyringBackendKind::Direct,
                    "pool-account-b-refreshed-2",
                ),
                (
                    AuthCredentialsStoreMode::Keyring,
                    AuthKeyringBackendKind::Secrets,
                    "pool-account-b-refreshed-3",
                ),
                (
                    AuthCredentialsStoreMode::Keyring,
                    AuthKeyringBackendKind::Direct,
                    "pool-account-b-refreshed-4",
                ),
                (
                    AuthCredentialsStoreMode::File,
                    AuthKeyringBackendKind::Direct,
                    "pool-account-b-refreshed-5",
                ),
            ] {
                let store = create_auth_storage_with_store(
                    codex_home.clone(),
                    mode,
                    Arc::clone(&keyring),
                    backend_kind,
                );
                let loaded = store
                    .load()?
                    .ok_or_else(|| std::io::Error::other("expected pool auth"))?;
                assert!(
                    loaded
                        .openai_api_key
                        .as_deref()
                        .is_some_and(|k| k.starts_with("pool-account-b")),
                    "mode={mode:?} backend={backend_kind:?} leaked {loaded:?}"
                );

                // Saving a refreshed token updates CODEX_AUTH_PATH in place and never unlinks it.
                store.save(&api_key_doc(refreshed_key))?;
                assert!(
                    pool_auth.is_file(),
                    "mode={mode:?} backend={backend_kind:?} unlinked CODEX_AUTH_PATH"
                );
                let on_disk: AuthDotJson =
                    serde_json::from_str(&std::fs::read_to_string(&pool_auth)?)?;
                assert_eq!(on_disk, api_key_doc(refreshed_key));

                // Shared CODEX_HOME's SecretsManager and Direct keyring remain untouched.
                assert_eq!(
                    shared_secrets.load()?,
                    Some(api_key_doc("shared-secrets-account-a"))
                );
                assert_eq!(
                    shared_direct.load()?,
                    Some(api_key_doc("shared-direct-account-a"))
                );
                assert_eq!(
                    keyring
                        .load(KEYRING_SERVICE, &default_key)
                        .map_err(std::io::Error::from)?
                        .as_deref()
                        .map(serde_json::from_str::<AuthDotJson>)
                        .transpose()?,
                    Some(api_key_doc("default-home-direct-a"))
                );
            }

            // Vanilla mode (`is_alab_binary() && !active()`) still uses aterm's pool logins at
            // CODEX_AUTH_PATH and still writes them through `save_auth_bytes` (refusing symlinks).
            codex_alab_core::set_enhanced(false);
            assert!(codex_alab_core::is_alab_binary() && !codex_alab_core::active());
            let vanilla_store = create_auth_storage_with_store(
                codex_home.clone(),
                AuthCredentialsStoreMode::Auto,
                Arc::clone(&keyring),
                AuthKeyringBackendKind::Secrets,
            );
            vanilla_store.save(&api_key_doc("pool-account-b-vanilla"))?;
            let on_disk: AuthDotJson = serde_json::from_str(&std::fs::read_to_string(&pool_auth)?)?;
            assert_eq!(on_disk, api_key_doc("pool-account-b-vanilla"));
            #[cfg(unix)]
            {
                let target = codex_home.join("planted-target.json");
                std::fs::write(&target, "unchanged")?;
                std::fs::remove_file(&pool_auth)?;
                std::os::unix::fs::symlink(&target, &pool_auth)?;
                assert!(
                    vanilla_store
                        .save(&api_key_doc("refused-through-symlink"))
                        .is_err(),
                    "vanilla mode must still refuse overwriting a symlinked CODEX_AUTH_PATH"
                );
                assert_eq!(std::fs::read_to_string(&target)?, "unchanged");
            }
        }
        "default" => {
            assert!(!codex_alab_core::home::has_admitted_auth_path());
            let keyring: Arc<dyn KeyringStore> = Arc::new(MockKeyringStore::default());
            let fallback = codex_home.join("auth.json");
            std::fs::write(&fallback, "stale-fallback")?;
            let store = create_auth_storage_with_store(
                codex_home,
                AuthCredentialsStoreMode::Keyring,
                keyring,
                AuthKeyringBackendKind::Direct,
            );
            store.save(&api_key_doc("default-keyring-key"))?;
            assert!(
                !fallback.exists(),
                "without CODEX_AUTH_PATH, keyring save still cleans up fallback auth.json"
            );
            assert_eq!(store.load()?, Some(api_key_doc("default-keyring-key")));
        }
        other => panic!("unexpected child case: {other}"),
    }

    println!("{CHILD_COMPLETE}");
    Ok(())
}
