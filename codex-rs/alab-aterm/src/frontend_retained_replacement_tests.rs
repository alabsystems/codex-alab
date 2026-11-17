// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Physical codec controls only. No fake Controller, Native Go or UI grant.
use super::*;
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;
use tokio::io::AsyncReadExt as _;
const OP: &str = "0123456789abcdef0123456789abcdef";
const NONCE: &[u8; 32] = b"fedcba9876543210fedcba9876543210";
fn pair() -> (UnixStream, UnixStream) {
    let _receiving = codex_alab_core::native_fd_custody::receiving().unwrap();
    UnixStream::pair().unwrap()
}
fn frame(tag: [u8; 2], body: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    result.extend_from_slice(&tag);
    result.extend_from_slice(OP.as_bytes());
    result.extend_from_slice(NONCE);
    result.extend_from_slice(&(body.len() as u64).to_be_bytes());
    result.extend_from_slice(body);
    result
}
async fn bounded<F: std::future::Future>(work: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .expect("replacement codec hang detector")
}
#[tokio::test]
async fn exact_fragmented_g1_and_e1_require_native_eof_before_ack() {
    for tag in [b"G1", b"E1"] {
        let (mut native, source) = pair();
        let bytes = frame(*tag, b"actual-bounded-data");
        let receiver = async {
            let mut offer = read_offer(source, tag, OP).await?;
            if offer.body != b"actual-bounded-data" || offer.nonce != *NONCE {
                return Err(unavailable());
            }
            offer.acknowledge().await
        };
        let sender = async {
            for chunk in bytes.chunks(3) {
                native.write_all(chunk).await?;
                tokio::task::yield_now().await;
            }
            native.shutdown().await?;
            let mut receipt = [0; 32];
            native.read_exact(&mut receipt).await?;
            if receipt != *NONCE || native.read(&mut [0]).await? != 0 {
                return Err(unavailable());
            }
            Ok::<_, io::Error>(())
        };
        let (received, sent) = bounded(async { tokio::join!(receiver, sender) }).await;
        received.unwrap();
        sent.unwrap();
    }
}
#[tokio::test]
async fn partial_poll_keeps_same_offer_and_cannot_ack_before_eof() {
    let (mut native, source) = pair();
    let bytes = frame(*b"G1", b"body");
    let receiving = read_offer(source, b"G1", OP);
    tokio::pin!(receiving);
    native.write_all(&bytes[..15]).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut receiving)
            .await
            .is_err()
    );
    native.write_all(&bytes[15..]).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut receiving)
            .await
            .is_err()
    );
    native.shutdown().await.unwrap();
    let mut offer = bounded(&mut receiving).await.unwrap();
    let ack = offer.acknowledge();
    tokio::pin!(ack);
    let reader = async {
        let mut nonce = [0; 32];
        native.read_exact(&mut nonce).await.unwrap();
        assert_eq!(nonce, *NONCE);
    };
    let (ack, ()) = bounded(async { tokio::join!(ack, reader) }).await;
    ack.unwrap();
}
#[tokio::test]
async fn malformed_offer_and_any_trailing_byte_fail_without_ack() {
    for mutation in 0..4 {
        let (mut native, source) = pair();
        let mut bytes = frame(*b"G1", b"body");
        match mutation {
            0 => bytes[0] = b'X',
            1 => bytes[2] = b'x',
            2 => bytes[34] = b'X',
            _ => bytes.push(1),
        }
        native.write_all(&bytes).await.unwrap();
        native.shutdown().await.unwrap();
        assert!(bounded(read_offer(source, b"G1", OP)).await.is_err());
        let closed = bounded(native.read(&mut [0])).await;
        assert!(matches!(closed, Ok(0)) || closed.is_err());
    }
}
fn send_right(
    stream: &UnixStream,
    bytes: &[u8],
    right: std::os::fd::BorrowedFd<'_>,
) -> io::Result<()> {
    let mut control = [0usize; 8];
    let mut iov = libc::iovec {
        iov_base: bytes.as_ptr().cast_mut().cast(),
        iov_len: bytes.len(),
    };
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen =
        unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::c_int>() as _) } as _;
    let head = unsafe { libc::CMSG_FIRSTHDR(&message) };
    unsafe {
        (*head).cmsg_level = libc::SOL_SOCKET;
        (*head).cmsg_type = libc::SCM_RIGHTS;
        (*head).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<libc::c_int>() as _) as _;
        *libc::CMSG_DATA(head).cast::<libc::c_int>() = right.as_raw_fd();
    }
    let sent = unsafe { libc::sendmsg(stream.as_raw_fd(), &message, 0) };
    if usize::try_from(sent).ok() == Some(bytes.len()) {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
#[tokio::test]
async fn scm_rights_on_header_body_or_eof_are_adopted_and_closed() {
    for offset in [0, 75, 78] {
        let (mut native, source) = pair();
        let bytes = frame(*b"G1", b"body");
        let (right, mut peer) = pair();
        native.write_all(&bytes[..offset]).await.unwrap();
        let suffix = if offset < bytes.len() {
            &bytes[offset..]
        } else {
            b"extra"
        };
        send_right(&native, suffix, right.as_fd()).unwrap();
        drop(right);
        native.shutdown().await.unwrap();
        assert!(bounded(read_offer(source, b"G1", OP)).await.is_err());
        // Receiver dropped its adopted right while the original sender and
        // native connection remain alive. This is actual kernel EOF, no count.
        assert_eq!(bounded(peer.read(&mut [0])).await.unwrap(), 0);
    }
}
#[test]
fn closed_native_command_rejects_selectors_paths_nul_and_duplicate_environment() {
    let service = "22222222222222222222222222222222";
    let valid = serde_json::json!({"schema":1,"program":b"/real/held/image".as_slice(),
        "argv":[b"--alab-retained-frontend".to_vec(),OP.as_bytes().to_vec(),service.as_bytes().to_vec()],
        "env":[[b"PATH".to_vec(),b"/usr/bin".to_vec()]],"cwd":b"/actual/cwd".as_slice()});
    assert!(CommandData::decode(&serde_json::to_vec(&valid).unwrap(), OP, service).is_ok());
    for mutation in 0..5 {
        let mut value = valid.clone();
        match mutation {
            0 => value["program"] = serde_json::json!(b"relative".as_slice()),
            1 => value["argv"][1] = serde_json::json!(b"foreign".as_slice()),
            2 => value["cwd"] = serde_json::json!(b"/nul\0path".as_slice()),
            3 => {
                let duplicate = value["env"][0].clone();
                value["env"].as_array_mut().unwrap().push(duplicate);
            }
            _ => value["ready"] = serde_json::json!(true),
        }
        assert!(CommandData::decode(&serde_json::to_vec(&value).unwrap(), OP, service).is_err());
    }
}
