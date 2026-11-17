// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower routing controls only. No Native Controller/E1/Go is fabricated.
use super::*;

#[test]
fn preparation_preserves_exact_final_owner_until_final_route() {
    let owner = Arc::new(());
    let mut seed = Some(owner.clone());
    assert!(take_final_seed(EndpointPurpose::Preparation, &mut seed).is_none());
    assert!(
        seed.as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &owner))
    );
    assert!(take_final_seed(EndpointPurpose::Preparation, &mut seed).is_none());
    let selected = take_final_seed(EndpointPurpose::FinalSession, &mut seed);
    assert!(
        selected
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &owner))
    );
    assert!(seed.is_none());
    assert!(take_final_seed(EndpointPurpose::FinalSession, &mut seed).is_none());
}

#[test]
fn cancelled_preparation_cannot_destroy_seed_resource() -> io::Result<()> {
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    let (retained, mut peer) = {
        let _fd_custody = codex_alab_core::native_fd_custody::receiving()?;
        UnixStream::pair()?
    };
    peer.set_nonblocking(true)?;
    let mut seed = Some(retained);
    drop(take_final_seed(EndpointPurpose::Preparation, &mut seed));
    let mut byte = [0; 1];
    let before_final = peer.read(&mut byte);
    assert!(matches!(before_final, Err(ref error) if error.kind() == io::ErrorKind::WouldBlock));
    drop(take_final_seed(EndpointPurpose::FinalSession, &mut seed));
    assert_eq!(peer.read(&mut byte)?, 0);
    Ok(())
}
