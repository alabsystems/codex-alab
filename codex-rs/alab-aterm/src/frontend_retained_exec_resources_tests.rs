// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual shared exclusion/FD rollback only. No Native/SDK command or grant is
//! fabricated; the private production section runs an intentional no-exec body.
use super::*;
use std::os::fd::AsRawFd;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

fn cloexec(raw: i32) -> io::Result<bool> {
    // SAFETY: the actual source/temporary inheritance guard keeps this fd alive.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(flags & libc::FD_CLOEXEC != 0)
}

#[test]
fn returned_exec_error_rolls_back_actual_socket_before_original_guard_releases() -> io::Result<()> {
    let (socket, _peer) = UnixStream::pair()?;
    let raw = socket.as_raw_fd();
    let owner = PreparedFrontendSocket::from_owned(socket)?;
    assert!(cloexec(raw)?);
    let core = codex_alab_core::native_fd_custody::executing()?;
    let saw_inherited = AtomicBool::new(false);
    let error = within_command(
        &core,
        std::path::Path::new("/intentional-no-exec"),
        |scope| scope.inherit_socket(&owner),
        || {
            saw_inherited.store(matches!(cloexec(raw), Ok(false)), Ordering::SeqCst);
            io::Error::new(io::ErrorKind::PermissionDenied, "intentional lower no-exec")
        },
    );
    // This assertion is deliberately before releasing the SAME original guard.
    let restored_under_guard = cloexec(raw)?;
    drop(core);
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(saw_inherited.load(Ordering::SeqCst));
    assert!(restored_under_guard);
    assert!(owner.is_owned()?);
    Ok(())
}

#[test]
fn preparation_refusal_does_not_call_exec_body_or_change_owned_socket() -> io::Result<()> {
    let (socket, _peer) = UnixStream::pair()?;
    let raw = socket.as_raw_fd();
    let owner = PreparedFrontendSocket::from_owned(socket)?;
    let core = codex_alab_core::native_fd_custody::executing()?;
    let called = AtomicBool::new(false);
    let error = within_command(
        &core,
        std::path::Path::new("/intentional-no-exec"),
        |_| -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "intentional prepared-resource refusal",
            ))
        },
        || {
            called.store(true, Ordering::SeqCst);
            io::Error::other("exec body must remain uncalled")
        },
    );
    let flags = cloexec(raw)?;
    drop(core);
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert!(!called.load(Ordering::SeqCst));
    assert!(flags);
    assert!(owner.is_owned()?);
    Ok(())
}

#[test]
fn unwinding_actual_section_restores_socket_while_caller_still_holds_exclusion() -> io::Result<()> {
    let (socket, _peer) = UnixStream::pair()?;
    let raw = socket.as_raw_fd();
    let owner = PreparedFrontendSocket::from_owned(socket)?;
    let core = codex_alab_core::native_fd_custody::executing()?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        within_command(
            &core,
            std::path::Path::new("/intentional-no-exec"),
            |scope| scope.inherit_socket(&owner),
            || panic!("intentional lower resource unwind"),
        )
    }));
    let restored_under_guard = cloexec(raw)?;
    drop(core);
    assert!(result.is_err());
    assert!(restored_under_guard);
    assert!(owner.is_owned()?);
    Ok(())
}

#[test]
fn stashed_prepared_owner_cannot_escape_command_owned_rollback() -> io::Result<()> {
    let (socket, _peer) = UnixStream::pair()?;
    let raw = socket.as_raw_fd();
    let owner = PreparedFrontendSocket::from_owned(socket)?;
    let mut stashed = None;
    let core = codex_alab_core::native_fd_custody::executing()?;
    let error = within_command(
        &core,
        std::path::Path::new("/intentional-no-exec"),
        |scope| {
            scope.inherit_socket(&owner)?;
            stashed = Some(owner.clone());
            Ok(())
        },
        || io::Error::other("intentional actual no-exec return"),
    );
    let restored_under_guard = cloexec(raw)?;
    drop(core);
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(restored_under_guard);
    assert!(
        stashed
            .as_ref()
            .is_some_and(|owner| matches!(owner.is_owned(), Ok(true)))
    );
    Ok(())
}

#[test]
fn actual_command_image_comparison_refuses_another_open_inode_under_same_section() -> io::Result<()>
{
    let image = tempfile::NamedTempFile::new()?;
    let other = tempfile::NamedTempFile::new()?;
    let core = codex_alab_core::native_fd_custody::executing()?;
    let mut checked = false;
    let _error = within_command(
        &core,
        image.path(),
        |scope| {
            scope.require_target_image(image.path(), image.as_file())?;
            assert!(
                scope
                    .require_target_image(other.path(), other.as_file())
                    .is_err()
            );
            checked = true;
            Ok(())
        },
        || io::Error::other("intentional image identity no-exec body"),
    );
    drop(core);
    assert!(checked);
    Ok(())
}
