// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Framing on the original backend socket. This module never locates an owner
//! or admits a command; native admission remains on the original SDK member.
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::OwnedFd;
use tokio::net::UnixStream;

struct PartialFrame<'a>(&'a UnixStream, bool);
impl Drop for PartialFrame<'_> {
    fn drop(&mut self) {
        if !self.1 {
            // A cancelled or failed partial frame cannot be retried on this
            // byte stream. Withdraw it so the receiver cannot misframe later IO.
            unsafe { libc::shutdown(self.0.as_raw_fd(), libc::SHUT_WR) };
        }
    }
}

pub(super) async fn send(
    stream: &UnixStream,
    nonce: &str,
    payload: &[u8],
    descriptors: [OwnedFd; 3],
) -> io::Result<()> {
    if !crate::managed_auth::valid_hex(nonce, 32)
        || payload.is_empty()
        || payload.len() > 512 * 1024
    {
        return Err(crate::managed_auth::unavailable());
    }
    let mut partial = PartialFrame(stream, false);
    loop {
        stream.writable().await?;
        match stream.try_io(tokio::io::Interest::WRITABLE, || {
            marker(stream, b'T', &descriptors)
        }) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
            result => result?,
        }
        break;
    }
    for mut bytes in [
        nonce.as_bytes(),
        &(payload.len() as u32).to_be_bytes(),
        payload,
    ] {
        while !bytes.is_empty() {
            stream.writable().await?;
            match stream.try_write(bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => bytes = &bytes[count..],
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                Err(error) => return Err(error),
            }
        }
    }
    partial.1 = true;
    Ok(())
}

pub(super) async fn send_listener(
    stream: &UnixStream,
    nonce: &str,
    descriptor: OwnedFd,
) -> io::Result<()> {
    if !crate::managed_auth::valid_hex(nonce, 32) {
        return Err(crate::managed_auth::unavailable());
    }
    let mut partial = PartialFrame(stream, false);
    loop {
        stream.writable().await?;
        match stream.try_io(tokio::io::Interest::WRITABLE, || {
            marker(stream, b'L', std::slice::from_ref(&descriptor))
        }) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
            result => result?,
        }
        break;
    }
    let mut bytes = nonce.as_bytes();
    while !bytes.is_empty() {
        stream.writable().await?;
        match stream.try_write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
            Err(error) => return Err(error),
        }
    }
    partial.1 = true;
    Ok(())
}

fn marker(stream: &UnixStream, mut byte: u8, descriptors: &[OwnedFd]) -> io::Result<()> {
    let mut vector = libc::iovec {
        iov_base: std::ptr::addr_of_mut!(byte).cast(),
        iov_len: 1,
    };
    let fds: Vec<_> = descriptors.iter().map(AsRawFd::as_raw_fd).collect();
    // usize storage supplies the alignment required by cmsghdr on both hosts.
    let mut ancillary = [0_usize; 16];
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut vector;
    message.msg_iovlen = 1;
    message.msg_control = ancillary.as_mut_ptr().cast();
    message.msg_controllen =
        unsafe { libc::CMSG_SPACE(std::mem::size_of_val(fds.as_slice()) as _) } as _;
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(fds.as_slice()) as _) as _;
        std::ptr::copy_nonoverlapping(
            fds.as_ptr().cast::<u8>(),
            libc::CMSG_DATA(header),
            std::mem::size_of_val(fds.as_slice()),
        );
        match libc::sendmsg(stream.as_raw_fd(), &message, libc::MSG_NOSIGNAL) {
            1 => Ok(()),
            -1 => Err(io::Error::last_os_error()),
            _ => Err(io::ErrorKind::WriteZero.into()),
        }
    }
}

#[cfg(test)]
#[path = "native_root_io_tests.rs"]
mod tests;
