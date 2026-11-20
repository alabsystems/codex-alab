// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use tempfile::TempDir;

#[test]
fn same_host_home_and_thread_reopen_disclosed_bytes_and_retire_only_owned_entries() -> io::Result<()>
{
    let home = TempDir::new()?;
    let other_home = TempDir::new()?;
    let thread = ThreadId::new();
    assert!(existing_directory(home.path(), thread)?.is_none());
    let scope = directory(home.path(), thread)?;
    assert_eq!(
        fs::metadata(scope.path())?.permissions().mode() & 0o777,
        0o700
    );
    let rejected = scope.create_capture(|staging| {
        staging.write_all(b"incomplete prefix")?;
        assert_eq!(scope.count(32)?, 0);
        Err(io::Error::other("injected copy failure"))
    });
    assert!(rejected.is_err());
    assert_eq!(scope.count(32)?, 0);
    let (name, capture) = scope.create_capture(|staging| staging.write_all(b"\x80diagnostic"))?;
    assert_eq!(capture.metadata()?.permissions().mode() & 0o777, 0o600);
    assert!(scope.names_file(name.as_ref(), &capture)?);
    let retained = scope.path().to_owned();
    let foreign = retained.join("unrelated");
    fs::create_dir(&foreign)?;
    fs::write(foreign.join("keep"), b"mine")?;
    let other = directory(other_home.path(), thread)?;
    assert_ne!(scope.path(), other.path());
    drop(scope);
    drop(capture);
    let reopened = existing_directory(home.path(), thread)?.unwrap();
    assert_eq!(fs::read(reopened.path().join(&name))?, b"\x80diagnostic");
    assert_eq!(reopened.count(1)?, 1);
    assert_eq!(reopened.count(32)?, 2);
    delete(home.path(), thread)?;
    assert!(!retained.join(name).exists());
    assert_eq!(fs::read(foreign.join("keep"))?, b"mine");
    fs::remove_file(foreign.join("keep"))?;
    fs::remove_dir(foreign)?;
    delete(home.path(), thread)?;
    delete(home.path(), thread)?;
    assert!(!retained.exists());
    assert!(existing_directory(other_home.path(), thread)?.is_some());
    delete(other_home.path(), thread)
}

#[test]
fn replaced_parent_and_leaf_never_redirect_capture_or_cleanup() -> io::Result<()> {
    let home = TempDir::new()?;
    let foreign = TempDir::new()?;
    let thread = ThreadId::new();
    let scope = directory(home.path(), thread)?;
    let (name, anchor) = scope.create_capture(|_| Ok(()))?;
    let original = scope.path().join(&name);
    let displaced = scope.path().join("displaced");
    fs::rename(&original, &displaced)?;
    let foreign_file = foreign.path().join(&name);
    fs::write(&foreign_file, b"protected")?;
    symlink(&foreign_file, &original)?;
    assert!(!scope.names_file(name.as_ref(), &anchor)?);
    scope.remove(name.as_ref(), &anchor)?;
    assert!(original.is_symlink());
    fs::remove_file(&original)?;
    fs::rename(&displaced, &original)?;
    assert!(scope.names_file(OsStr::new("../other"), &anchor).is_err());

    let detached = scope.path().with_extension("moved");
    fs::rename(scope.path(), &detached)?;
    symlink(foreign.path(), scope.path())?;
    assert!(!scope.names_file(name.as_ref(), &anchor)?);
    assert!(scope.create_capture(|_| Ok(())).is_err());
    assert!(directory(home.path(), thread).is_err());
    assert!(delete(home.path(), thread).is_err());
    assert_eq!(fs::read(&foreign_file)?, b"protected");
    fs::remove_file(scope.path())?;
    fs::rename(&detached, scope.path())?;
    fs::set_permissions(scope.path(), fs::Permissions::from_mode(0o755))?;
    assert!(directory(home.path(), thread).is_err());
    fs::set_permissions(scope.path(), fs::Permissions::from_mode(0o700))?;
    delete(home.path(), thread)
}

#[test]
fn retirement_preserves_same_name_replacements_and_other_hard_links() -> io::Result<()> {
    let home = TempDir::new()?;
    let thread = ThreadId::new();
    let scope = directory(home.path(), thread)?;
    let (name, _original) =
        scope.create_capture(|staging| staging.write_all(b"original output"))?;
    let retained = scope.path().join(&name);
    let other_link = scope.path().join("other-link");
    fs::hard_link(&retained, &other_link)?;
    let (replaced_name, first) = scope.create_capture(|_| Ok(()))?;
    let replaced = scope.path().join(replaced_name);
    let replacement = scope.path().join("replacement");
    fs::write(&replacement, b"unrelated replacement")?;
    fs::rename(&replacement, &replaced)?;
    drop(first);
    delete(home.path(), thread)?;
    assert!(!retained.exists());
    assert_eq!(fs::read(&other_link)?, b"original output");
    assert_eq!(fs::read(&replaced)?, b"unrelated replacement");
    fs::remove_file(replaced)?;
    fs::remove_file(other_link)?;
    delete(home.path(), thread)
}

#[test]
fn a_replacement_claims_only_the_exact_prepared_prefix() -> io::Result<()> {
    let home = TempDir::new()?;
    let thread = ThreadId::new();
    let writer = directory(home.path(), thread)?
        .try_publication_lock()?
        .unwrap();
    let (name, hash) = writer.prepare_copy(5, |file| file.write_all(b"\0bad\xff"))?;
    let held = directory(home.path(), thread)?;
    assert!(!held.path().join(&name).exists());
    assert!(
        directory(home.path(), thread)?
            .try_publication_lock()?
            .is_none()
    );
    assert_eq!(held.count(32)?, 1);
    drop(writer);
    let replacement = directory(home.path(), thread)?
        .try_publication_lock()?
        .unwrap();
    replacement.clean_orphan_stages(std::slice::from_ref(&name))?;
    assert!(replacement.publish(&name, 5, &hash)?);
    assert_eq!(fs::read(held.path().join(&name))?, b"\0bad\xff");
    assert_eq!(held.count(32)?, 1);
    drop(replacement);
    let replacement = directory(home.path(), thread)?
        .try_publication_lock()?
        .unwrap();
    assert!(replacement.publish(&name, 5, &hash)?);
    replacement.clean_orphan_stages(&[])?;
    assert_eq!(held.count(32)?, 1);
    replacement.cleanup(&name, false)?;
    assert_eq!(fs::read(held.path().join(&name))?, b"\0bad\xff");
    drop(replacement);
    held.retire()
}

#[test]
fn replaced_stage_and_modified_disclosed_file_are_never_attachment_evidence() -> io::Result<()> {
    let home = TempDir::new()?;
    let thread = ThreadId::new();
    let held = directory(home.path(), thread)?;
    let (disclosed, _file) = held.create_capture(|file| file.write_all(b"actual"))?;
    let disclosed = disclosed.to_str().unwrap();
    let writer = directory(home.path(), thread)?
        .try_publication_lock()?
        .unwrap();
    let mut native = codex_alab_core::shell_log_store::PrefixDigest::default();
    native.append(b"actual");
    let hash = native.hex();
    fs::write(held.path().join(disclosed), b"forged")?;
    assert!(!writer.publish(disclosed, 6, &hash)?);
    writer.cleanup(disclosed, false)?;
    assert_eq!(fs::read(held.path().join(disclosed))?, b"forged");

    let (private, hash) = writer.prepare_copy(6, |file| file.write_all(b"actual"))?;
    let stage = held.path().join(format!(".pending-{private}"));
    let keep = held.path().join("ordinary");
    fs::write(&keep, b"forged")?;
    fs::rename(&keep, &stage)?;
    assert!(!writer.publish(&private, 6, &hash)?);
    writer.cleanup(&private, false)?;
    assert_eq!(fs::read(&stage)?, b"forged");
    fs::remove_file(stage)?;
    drop(writer);
    held.retire()
}

#[test]
fn ordinary_captures_publish_during_sqlite_receipts_and_share_the_same_slot_bound() -> io::Result<()>
{
    let home = TempDir::new()?;
    let thread = ThreadId::new();
    let direct = directory(home.path(), thread)?;
    let writer = directory(home.path(), thread)?
        .try_publication_lock()?
        .unwrap();
    let (name, hash) = writer.prepare_copy(4, |file| file.write_all(b"late"))?;
    for _ in 1..codex_alab_core::shell_log_store::MAX_THREAD_LOGS {
        let (visible, _) = direct.create_capture(|file| file.write_all(b"clipped"))?;
        assert_eq!(fs::read(direct.path().join(visible))?, b"clipped");
    }
    assert!(direct.create_capture(|_| Ok(())).is_err());
    assert!(writer.publish(&name, 4, &hash)?);
    assert_eq!(
        direct.count(100)?,
        codex_alab_core::shell_log_store::MAX_THREAD_LOGS
    );
    assert!(direct.create_capture(|_| Ok(())).is_err());
    writer.cleanup(&name, true)?;
    drop(writer);
    direct.retire()
}
