// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::io::Read as _;
use std::os::fd::FromRawFd as _;

#[tokio::test]
async fn frame_transfers_three_actual_descriptors_and_exact_body() {
    let (sender, receiver) = UnixStream::pair().unwrap();
    let mut receiver = receiver.into_std().unwrap();
    receiver.set_nonblocking(false).unwrap();
    let (sent, peers): (Vec<_>, Vec<_>) = (0..3)
        .map(|_| std::os::unix::net::UnixStream::pair().unwrap())
        .unzip();
    let descriptors: [OwnedFd; 3] = sent
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let reader = std::thread::spawn(move || {
        let mut byte = 0_u8;
        let mut vector = libc::iovec {
            iov_base: std::ptr::addr_of_mut!(byte).cast(),
            iov_len: 1,
        };
        let mut ancillary = [0_usize; 16];
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut vector;
        message.msg_iovlen = 1;
        message.msg_control = ancillary.as_mut_ptr().cast();
        message.msg_controllen = std::mem::size_of_val(&ancillary) as _;
        assert_eq!(
            unsafe { libc::recvmsg(receiver.as_raw_fd(), &mut message, 0) },
            1
        );
        assert_eq!(byte, b'T');
        let descriptors = unsafe {
            let header = libc::CMSG_FIRSTHDR(&message);
            assert!(!header.is_null());
            assert_eq!((*header).cmsg_level, libc::SOL_SOCKET);
            assert_eq!((*header).cmsg_type, libc::SCM_RIGHTS);
            assert_eq!((*header).cmsg_len as usize, libc::CMSG_LEN(12) as usize);
            let fds = libc::CMSG_DATA(header).cast::<libc::c_int>();
            (0..3)
                .map(|i| OwnedFd::from_raw_fd(*fds.add(i)))
                .collect::<Vec<_>>()
        };
        let mut body = [0_u8; 40];
        receiver.read_exact(&mut body).unwrap();
        assert_eq!(&body[..32], b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(&body[32..36], &4_u32.to_be_bytes());
        assert_eq!(&body[36..], b"body");
        descriptors
    });
    send(&sender, &"a".repeat(32), b"body", descriptors)
        .await
        .unwrap();
    let received = reader.join().unwrap();
    drop(received);
    for mut peer in peers {
        let mut byte = [0];
        assert_eq!(peer.read(&mut byte).unwrap(), 0);
    }
}

#[tokio::test]
async fn dropping_partial_frame_withdraws_write_side() {
    let (sender, mut receiver) = UnixStream::pair().unwrap();
    drop(PartialFrame(&sender, false));
    let mut byte = [0];
    use tokio::io::AsyncReadExt as _;
    assert_eq!(receiver.read(&mut byte).await.unwrap(), 0);
}

#[tokio::test]
async fn listener_publication_sends_the_actual_socket_and_nonce() {
    let (sender, receiver) = UnixStream::pair().unwrap();
    let mut receiver = receiver.into_std().unwrap();
    receiver.set_nonblocking(false).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    use std::os::fd::AsFd as _;
    let descriptor = listener.as_fd().try_clone_to_owned().unwrap();
    let reader = std::thread::spawn(move || {
        let mut byte = 0_u8;
        let mut vector = libc::iovec {
            iov_base: std::ptr::addr_of_mut!(byte).cast(),
            iov_len: 1,
        };
        let mut ancillary = [0_usize; 16];
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut vector;
        message.msg_iovlen = 1;
        message.msg_control = ancillary.as_mut_ptr().cast();
        message.msg_controllen = std::mem::size_of_val(&ancillary) as _;
        assert_eq!(
            unsafe { libc::recvmsg(receiver.as_raw_fd(), &mut message, 0) },
            1
        );
        assert_eq!(byte, b'L');
        let descriptor = unsafe {
            let header = libc::CMSG_FIRSTHDR(&message);
            assert!(!header.is_null());
            assert_eq!((*header).cmsg_type, libc::SCM_RIGHTS);
            assert_eq!((*header).cmsg_len as usize, libc::CMSG_LEN(4) as usize);
            OwnedFd::from_raw_fd(*libc::CMSG_DATA(header).cast::<libc::c_int>())
        };
        let mut nonce = [0; 32];
        receiver.read_exact(&mut nonce).unwrap();
        assert_eq!(&nonce, b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        std::net::TcpListener::from(descriptor)
    });
    send_listener(&sender, &"a".repeat(32), descriptor)
        .await
        .unwrap();
    let received = reader.join().unwrap();
    drop(listener);
    assert_eq!(received.local_addr().unwrap(), address);
}
