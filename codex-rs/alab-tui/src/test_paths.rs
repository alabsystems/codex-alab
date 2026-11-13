// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Short, private fixture roots for native socket and path-rendering tests.

#![cfg(feature = "test-support")]

#[cfg(unix)]
fn private_tempdir_in(
    prefix: &str,
    parent: &std::path::Path,
) -> std::io::Result<tempfile::TempDir> {
    use std::os::unix::fs::PermissionsExt;

    tempfile::Builder::new()
        .prefix(prefix)
        .rand_bytes(6)
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)
}

/// Create a private directory with tempfile's automatic cleanup while
/// bounding paths used by fixtures whose behavior depends on their length.
/// Contained tests use their admitted scratch root even if TMPDIR is nested later.
pub fn short_tempdir() -> std::io::Result<tempfile::TempDir> {
    #[cfg(unix)]
    {
        let scratch = std::env::var_os("ALAB_TEST_SCRATCH").map(std::path::PathBuf::from);
        short_tempdir_in(scratch.as_deref())
    }
    #[cfg(not(unix))]
    {
        tempfile::tempdir()
    }
}

#[cfg(unix)]
fn short_tempdir_in(scratch: Option<&std::path::Path>) -> std::io::Result<tempfile::TempDir> {
    let (prefix, parent) = match scratch {
        Some(parent) if !parent.is_absolute() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "configured native fixture root must be absolute",
            ));
        }
        Some(parent) => ("c", parent),
        None => ("cx.", std::path::Path::new("/tmp")),
    };
    let root = private_tempdir_in(prefix, parent)?;
    check_short_root(root.path())?;
    Ok(root)
}

#[cfg(unix)]
fn check_short_root(root: &std::path::Path) -> std::io::Result<()> {
    // A 32-byte root leaves room for the app-server control socket below
    // macOS's 103-byte limit and the destination in an 80-column startup row.
    // Check the resolved root too: macOS expands /tmp to /private/tmp.
    const MAX_ROOT_BYTES: usize = 32;
    let resolved = root.canonicalize()?;
    if [root, resolved.as_path()]
        .into_iter()
        .any(|path| path.as_os_str().as_encoded_bytes().len() > MAX_ROOT_BYTES)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "native fixture root exceeds its 32-byte path budget",
        ));
    }
    Ok(())
}

/// Prefer the configured temporary directory and retain only roots that can hold
/// the longest upgrade fixture socket, including its canonical path on macOS.
/// The caller decides whether to retain the witness beyond the test's lifetime.
#[cfg(unix)]
pub fn upgrade_fixture_dir() -> std::io::Result<tempfile::TempDir> {
    upgrade_fixture_dir_in(&std::env::temp_dir(), std::path::Path::new("/tmp"))
}

#[cfg(unix)]
fn upgrade_fixture_dir_in(
    preferred: &std::path::Path,
    fallback: &std::path::Path,
) -> std::io::Result<tempfile::TempDir> {
    for parent in [preferred, fallback] {
        let root = private_tempdir_in("ac.", parent)?;
        let resolved = root.path().canonicalize()?;
        let longest_socket = format!("held-completion/runtime/aterm/aterm-{}.sock", u32::MAX);
        if [root.path(), resolved.as_path()].into_iter().all(|path| {
            path.join(&longest_socket)
                .as_os_str()
                .as_encoded_bytes()
                .len()
                <= 103
        }) {
            return Ok(root);
        }
        // Only a path-budget rejection proceeds to the next parent. The owning
        // TempDir cleans this candidate before the fallback is created.
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "native upgrade fixture socket exceeds its 103-byte path budget",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::check_short_root;
    use super::short_tempdir;
    use super::short_tempdir_in;
    use super::upgrade_fixture_dir_in;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn roots_are_private_unique_and_removed_on_drop() {
        let first = short_tempdir().expect("first native fixture root");
        let second = short_tempdir().expect("second native fixture root");
        let first_path = first.path().to_path_buf();
        let second_path = second.path().to_path_buf();
        assert_ne!(first_path, second_path);
        for root in [&first_path, &second_path] {
            let permissions = root.metadata().expect("root exists").permissions();
            assert_eq!(permissions.mode() & 0o077, 0, "fixture root is private");
        }
        drop(first);
        drop(second);
        assert!(!first_path.exists());
        assert!(!second_path.exists());
    }

    #[test]
    fn configured_short_root_uses_private_seven_byte_leaves_and_cleans_them() {
        let owner = short_tempdir().unwrap();
        let parent = owner.path().parent().unwrap();
        let first = short_tempdir_in(Some(parent)).unwrap();
        let second = short_tempdir_in(Some(parent)).unwrap();
        assert_ne!(first.path(), second.path());
        for root in [&first, &second] {
            assert_eq!(root.path().parent(), Some(parent));
            let leaf = root.path().file_name().unwrap().as_encoded_bytes();
            assert_eq!(leaf.len(), 7);
            assert_eq!(leaf[0], b'c');
            assert_eq!(
                root.path().metadata().unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert!(root.path().as_os_str().as_encoded_bytes().len() <= 32);
            assert!(
                root.path()
                    .canonicalize()
                    .unwrap()
                    .as_os_str()
                    .as_encoded_bytes()
                    .len()
                    <= 32
            );
        }
        let paths = [first.path().to_path_buf(), second.path().to_path_buf()];
        drop(first);
        drop(second);
        assert!(paths.iter().all(|path| !path.exists()));
    }

    #[test]
    fn configured_long_root_is_rejected_without_leaving_a_candidate() {
        let owner = short_tempdir().unwrap();
        let parent = owner.path().join("long".repeat(8));
        std::fs::create_dir(&parent).unwrap();
        let error = short_tempdir_in(Some(&parent)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
    }

    #[test]
    fn canonical_short_root_budget_rejects_a_symlink_to_a_long_directory() {
        let owner = short_tempdir().unwrap();
        let target = owner.path().join("long".repeat(8));
        std::fs::create_dir(&target).unwrap();
        let alias = short_tempdir().unwrap();
        assert!(alias.path().as_os_str().as_encoded_bytes().len() <= 32);
        std::fs::remove_dir(alias.path()).unwrap();
        std::os::unix::fs::symlink(&target, alias.path()).unwrap();
        let error = check_short_root(alias.path()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        std::fs::remove_file(alias.path()).unwrap();
        assert!(target.is_dir());
    }

    #[test]
    fn configured_root_creation_error_does_not_fall_back_to_ambient_tmp() {
        let owner = short_tempdir().unwrap();
        let missing = owner.path().join("missing");
        let error = short_tempdir_in(Some(&missing)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(!missing.exists());
        for relative in ["", "relative"] {
            let error = short_tempdir_in(Some(std::path::Path::new(relative))).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn upgrade_root_prefers_configured_parent_and_binds_the_longest_socket() {
        let base = short_tempdir().expect("private test root");
        let preferred = base.path().join("p");
        let fallback = base.path().join("f");
        std::fs::create_dir(&preferred).unwrap();
        std::fs::create_dir(&fallback).unwrap();
        let root = upgrade_fixture_dir_in(&preferred, &fallback).unwrap();
        assert_eq!(root.path().parent(), Some(preferred.as_path()));
        assert_eq!(
            root.path().metadata().unwrap().permissions().mode() & 0o077,
            0
        );
        assert_eq!(std::fs::read_dir(&fallback).unwrap().count(), 0);

        let socket = root.path().join(format!(
            "held-completion/runtime/aterm/aterm-{}.sock",
            u32::MAX
        ));
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&socket)
            .expect("the deepest native fixture socket fits");
        drop(listener);
        let path = root.path().to_path_buf();
        drop(root);
        assert!(!path.exists(), "normal cleanup removes the nested socket");
    }

    #[test]
    fn overlong_preferred_root_is_cleaned_before_private_fallback() {
        let base = short_tempdir().unwrap();
        let preferred = base.path().join("p".repeat(104));
        let fallback = base.path().join("f");
        std::fs::create_dir(&preferred).unwrap();
        std::fs::create_dir(&fallback).unwrap();
        let root = upgrade_fixture_dir_in(&preferred, &fallback).unwrap();
        assert_eq!(root.path().parent(), Some(fallback.as_path()));
        assert_eq!(std::fs::read_dir(&preferred).unwrap().count(), 0);
        assert_eq!(
            root.path().metadata().unwrap().permissions().mode() & 0o077,
            0
        );
        drop(root);
        assert_eq!(std::fs::read_dir(&fallback).unwrap().count(), 0);
    }

    #[test]
    fn canonical_socket_budget_rejects_a_short_alias_to_a_long_parent() {
        let base = short_tempdir().unwrap();
        let target = base.path().join("p".repeat(104));
        let alias = base.path().join("p");
        let fallback = base.path().join("f");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        std::fs::create_dir(&fallback).unwrap();
        let root = upgrade_fixture_dir_in(&alias, &fallback).unwrap();
        assert_eq!(root.path().parent(), Some(fallback.as_path()));
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    #[test]
    fn overlong_fallback_is_rejected_and_failed_candidates_are_removed() {
        let base = short_tempdir().unwrap();
        let preferred = base.path().join("p".repeat(104));
        let fallback = base.path().join("f".repeat(104));
        std::fs::create_dir(&preferred).unwrap();
        std::fs::create_dir(&fallback).unwrap();
        let error = upgrade_fixture_dir_in(&preferred, &fallback).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read_dir(&preferred).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(&fallback).unwrap().count(), 0);
    }

    #[test]
    fn preferred_creation_error_is_preserved_without_using_fallback() {
        let base = short_tempdir().unwrap();
        let preferred = base.path().join("missing");
        let fallback = base.path().join("f");
        std::fs::create_dir(&fallback).unwrap();
        let error = upgrade_fixture_dir_in(&preferred, &fallback).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(std::fs::read_dir(&fallback).unwrap().count(), 0);
    }

    #[test]
    fn fallback_creation_error_preserves_error_and_cleans_rejected_preferred() {
        let base = short_tempdir().unwrap();
        let preferred = base.path().join("p".repeat(104));
        let fallback = base.path().join("missing");
        std::fs::create_dir(&preferred).unwrap();
        let error = upgrade_fixture_dir_in(&preferred, &fallback).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(std::fs::read_dir(&preferred).unwrap().count(), 0);
    }
}
