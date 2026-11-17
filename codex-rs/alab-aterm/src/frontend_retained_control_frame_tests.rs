// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual Unix byte/SCM framing only. These frames grant no SDK, foreground,
//! Native origin, Go, replacement or goal-continuation permission.
use super::*;
use std::os::fd::AsRawFd as _;
use std::time::Duration;
use tokio::io::AsyncReadExt as _;
use tokio::io::AsyncWriteExt as _;

const OPERATION: [u8; 32] = *b"0123456789abcdef0123456789abcdef";
const FRONTIER: [u8; 32] = *b"11111111111111111111111111111111";
const NONCE: [u8; 32] = *b"22222222222222222222222222222222";
const ATTACHMENT: [u8; 32] = *b"33333333333333333333333333333333";

fn pair() -> io::Result<(UnixStream, UnixStream)> {
    // Darwin's socket creation/CLOEXEC steps share the actual raw-exec guard.
    // The guard ends before either stream is returned to asynchronous code.
    let _receipt = codex_alab_core::native_fd_custody::receiving()?;
    let (a, b) = std::os::unix::net::UnixStream::pair()?;
    a.set_nonblocking(true)?;
    b.set_nonblocking(true)?;
    Ok((UnixStream::from_std(a)?, UnixStream::from_std(b)?))
}

async fn bounded<T>(future: impl std::future::Future<Output = io::Result<T>>) -> io::Result<T> {
    tokio::time::timeout(Duration::from_secs(3), future)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control framing test stalled"))?
}

fn wire(tag: &[u8], fields: &[[u8; 32]]) -> Vec<u8> {
    let mut bytes = tag.to_vec();
    for field in fields {
        bytes.extend_from_slice(field);
    }
    bytes
}

async fn fragmented(socket: &mut UnixStream, bytes: &[u8]) -> io::Result<()> {
    for byte in bytes {
        socket.write_all(std::slice::from_ref(byte)).await?;
        // A separate asynchronous write for every byte also crosses field and
        // version boundaries. The production parser alone assembles them.
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    Ok(())
}

async fn rights(socket: &UnixStream, bytes: &[u8], descriptors: &[i32]) -> io::Result<()> {
    assert!(!bytes.is_empty());
    assert!(descriptors.len() <= 2);
    loop {
        socket.writable().await?;
        let result = socket.try_io(tokio::io::Interest::WRITABLE, || {
            let mut storage = [0_usize; 16];
            let mut iov = libc::iovec {
                iov_base: bytes.as_ptr().cast_mut().cast(),
                iov_len: bytes.len(),
            };
            // SAFETY: message and aligned ancillary storage remain alive for
            // this synchronous sendmsg, and every sent fd has a live owner.
            let count = unsafe {
                let mut message: libc::msghdr = std::mem::zeroed();
                message.msg_iov = &mut iov;
                message.msg_iovlen = 1;
                if !descriptors.is_empty() {
                    let size = std::mem::size_of_val(descriptors);
                    message.msg_control = storage.as_mut_ptr().cast();
                    message.msg_controllen = libc::CMSG_SPACE(size as _) as _;
                    let header = libc::CMSG_FIRSTHDR(&message);
                    (*header).cmsg_level = libc::SOL_SOCKET;
                    (*header).cmsg_type = libc::SCM_RIGHTS;
                    (*header).cmsg_len = libc::CMSG_LEN(size as _) as _;
                    for (index, descriptor) in descriptors.iter().enumerate() {
                        libc::CMSG_DATA(header)
                            .cast::<libc::c_int>()
                            .add(index)
                            .write_unaligned(*descriptor);
                    }
                }
                libc::sendmsg(socket.as_raw_fd(), &message, 0)
            };
            if count < 0 {
                return Err(io::Error::last_os_error());
            }
            if count as usize != bytes.len() {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "short control fixture send",
                ));
            }
            Ok(())
        });
        match result {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            result => return result,
        }
    }
}

async fn assert_peer_closed(mut peer: UnixStream) -> io::Result<()> {
    let mut byte = [0];
    assert_eq!(
        bounded(peer.read(&mut byte)).await?,
        0,
        "a rejected received descriptor kept its actual peer alive"
    );
    Ok(())
}

#[tokio::test]
async fn fragmented_h1_preserves_both_fields_and_bounded_head_deadline() -> io::Result<()> {
    let (receiver, mut sender) = pair()?;
    let bytes = wire(b"H1", &[OPERATION, NONCE]);
    let before = Instant::now();
    let (received, written) = tokio::join!(
        bounded(receive(&receiver)),
        bounded(fragmented(&mut sender, &bytes))
    );
    written?;
    let Frame::Hold(hold) = received? else {
        panic!("H1 did not decode as Hold");
    };
    assert_eq!(hold.operation, OPERATION);
    assert_eq!(hold.nonce, NONCE);
    assert!(hold.deadline > before);
    assert!(hold.deadline <= Instant::now() + Duration::from_secs(60));
    Ok(())
}

#[tokio::test]
async fn fragmented_r0_and_r1_preserve_distinct_initial_and_replacement_fields() -> io::Result<()> {
    for go in [None, Some(OPERATION)] {
        let (receiver, mut sender) = pair()?;
        let bytes = match go {
            Some(go) => wire(b"R1", &[go, FRONTIER, NONCE, ATTACHMENT]),
            None => wire(b"R0", &[FRONTIER, NONCE, ATTACHMENT]),
        };
        let before = Instant::now();
        let (received, written) = tokio::join!(
            bounded(receive(&receiver)),
            bounded(fragmented(&mut sender, &bytes))
        );
        written?;
        let Frame::Runtime(runtime) = received? else {
            panic!("R frame did not decode as Runtime");
        };
        assert_eq!(runtime.go, go);
        assert_eq!(runtime.frontier, FRONTIER);
        assert_eq!(runtime.nonce, NONCE);
        assert_eq!(runtime.attachment, ATTACHMENT);
        assert!(runtime.deadline > before);
        assert!(runtime.deadline <= Instant::now() + Duration::from_secs(60));
    }
    Ok(())
}

#[tokio::test]
async fn a_adopts_one_cloexec_endpoint_and_leaves_nonce_for_its_original_consumer() -> io::Result<()>
{
    let (receiver, mut sender) = pair()?;
    let (sent_endpoint, mut physical_peer) = pair()?;
    let mut bytes = vec![b'A'];
    bytes.extend_from_slice(&NONCE);
    rights(&sender, &bytes, &[sent_endpoint.as_raw_fd()]).await?;
    let before = Instant::now();
    let Frame::Attachment {
        descriptor,
        deadline,
    } = bounded(receive(&receiver)).await?
    else {
        panic!("A did not decode as Attachment");
    };
    // The real Native sender retains its offer until actual receipt. Retire
    // its copy only after the receiver owns the transferred descriptor, then
    // prove that the adopted endpoint works independently of that copy.
    drop(sent_endpoint);
    assert!(deadline > before);
    assert!(deadline <= Instant::now() + Duration::from_secs(60));
    // SAFETY: F_GETFD inspects the still-owned actual received descriptor.
    let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD) };
    assert!(flags >= 0);
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    assert_eq!(bounded(fd::receive_nonce(&receiver)).await?, NONCE);
    let mut endpoint = fd::from_descriptor(descriptor)?;
    physical_peer.write_all(b"actual-endpoint").await?;
    let mut actual = [0; 15];
    bounded(endpoint.read_exact(&mut actual))
        .await
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("read adopted endpoint after sender copy retired: {error}"),
            )
        })?;
    assert_eq!(&actual, b"actual-endpoint");
    drop(endpoint);
    assert_peer_closed(physical_peer).await?;
    sender.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_without_right_or_with_two_actual_rights_is_rejected_and_all_rights_close()
-> io::Result<()> {
    let (receiver, sender) = pair()?;
    rights(&sender, b"A", &[]).await?;
    assert!(bounded(receive(&receiver)).await.is_err());
    let (receiver, sender) = pair()?;
    let (descriptor, peer) = pair()?;
    rights(
        &sender,
        b"A",
        &[descriptor.as_raw_fd(), descriptor.as_raw_fd()],
    )
    .await?;
    drop(descriptor);
    assert!(bounded(receive(&receiver)).await.is_err());
    assert_peer_closed(peer).await
}

#[tokio::test]
async fn h_and_r_heads_refuse_actual_rights_and_close_them() -> io::Result<()> {
    for head in *b"HRX" {
        let (receiver, sender) = pair()?;
        let (descriptor, peer) = pair()?;
        rights(&sender, &[head], &[descriptor.as_raw_fd()]).await?;
        drop(descriptor);
        assert!(bounded(receive(&receiver)).await.is_err());
        assert_peer_closed(peer).await?;
    }
    Ok(())
}

#[tokio::test]
async fn every_h_r_version_and_body_field_refuses_rights_and_closes_them() -> io::Result<()> {
    for (bytes, offsets) in [
        (wire(b"H1", &[OPERATION, NONCE]), vec![1, 2, 34]),
        (
            wire(b"R0", &[FRONTIER, NONCE, ATTACHMENT]),
            vec![1, 2, 34, 66],
        ),
        (
            wire(b"R1", &[OPERATION, FRONTIER, NONCE, ATTACHMENT]),
            vec![1, 2, 34, 66, 98],
        ),
    ] {
        for offset in offsets {
            let (receiver, mut sender) = pair()?;
            let (descriptor, peer) = pair()?;
            sender.write_all(&bytes[..offset]).await?;
            let mut receiving = Box::pin(receive(&receiver));
            // Keep this SAME partially polled decoder. A header-only success
            // would fail here; a dropped/restarted parser cannot pass this test.
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut receiving)
                    .await
                    .is_err()
            );
            rights(
                &sender,
                &bytes[offset..offset + 1],
                &[descriptor.as_raw_fd()],
            )
            .await?;
            drop(descriptor);
            assert!(bounded(receiving).await.is_err());
            assert_peer_closed(peer).await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn malformed_tag_version_and_each_nonhex_body_field_are_rejected() -> io::Result<()> {
    let mut malformed = vec![
        b"X".to_vec(),
        b"H0".to_vec(),
        b"H2".to_vec(),
        b"R2".to_vec(),
    ];
    for (bytes, offsets) in [
        (wire(b"H1", &[OPERATION, NONCE]), vec![2, 34]),
        (wire(b"R0", &[FRONTIER, NONCE, ATTACHMENT]), vec![2, 34, 66]),
        (
            wire(b"R1", &[OPERATION, FRONTIER, NONCE, ATTACHMENT]),
            vec![2, 34, 66, 98],
        ),
    ] {
        for offset in offsets {
            let mut invalid = bytes.clone();
            invalid[offset] = b'g';
            malformed.push(invalid);
        }
        let mut invalid_utf8 = bytes;
        invalid_utf8[2] = 0xff;
        malformed.push(invalid_utf8);
    }
    for bytes in malformed {
        let (receiver, mut sender) = pair()?;
        sender.write_all(&bytes).await?;
        sender.shutdown().await?;
        assert!(bounded(receive(&receiver)).await.is_err());
    }
    Ok(())
}

#[tokio::test]
async fn truncation_at_header_and_every_field_boundary_fails_without_a_frame() -> io::Result<()> {
    for (bytes, lengths) in [
        (wire(b"H1", &[OPERATION, NONCE]), vec![0, 1, 2, 33, 34, 65]),
        (
            wire(b"R0", &[FRONTIER, NONCE, ATTACHMENT]),
            vec![0, 1, 2, 33, 34, 65, 66, 97],
        ),
        (
            wire(b"R1", &[OPERATION, FRONTIER, NONCE, ATTACHMENT]),
            vec![0, 1, 2, 33, 34, 65, 66, 97, 98, 129],
        ),
    ] {
        for length in lengths {
            let (receiver, mut sender) = pair()?;
            sender.write_all(&bytes[..length]).await?;
            sender.shutdown().await?;
            assert!(bounded(receive(&receiver)).await.is_err());
        }
    }
    Ok(())
}

#[path = "frontend_source_selection_frame_tests.rs"]
mod source_selection;
