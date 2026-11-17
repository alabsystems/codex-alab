// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Shared private FD primitive for frontend F1 and backend A receipt.
//! No wire bytes construct SDK or foreground authority.
use codex_alab_core::managed_auth::unavailable;
use codex_alab_core::managed_auth::valid_hex;
use std::io;
use std::os::fd::AsFd as _;
use std::os::fd::AsRawFd as _;
use std::os::fd::FromRawFd as _;
use std::os::fd::OwnedFd;
use std::time::Duration;
use tokio::net::UnixStream;
use tokio::time::Instant;

pub(super) struct ControlHead {
    pub(super) tag: u8,
    pub(super) descriptor: Option<OwnedFd>,
    pub(super) deadline: Instant,
}

pub(super) fn take_private_stdin() -> io::Result<UnixStream> {
    let _custody = codex_alab_core::native_fd_custody::receiving()?;
    let descriptor = std::io::stdin().as_fd().try_clone_to_owned()?;
    validate_endpoint(&descriptor)?;
    let null = std::fs::File::open("/dev/null")?;
    // SAFETY: both descriptors are owned and stdin's private socket was cloned
    // before its replacement. Helpers must never inherit the original channel.
    if unsafe { libc::dup2(null.as_raw_fd(), libc::STDIN_FILENO) } < 0 {
        return Err(io::Error::last_os_error());
    }
    from_descriptor(descriptor)
}

#[cfg(test)]
pub(super) fn adopt_socket(socket: std::os::unix::net::UnixStream) -> io::Result<UnixStream> {
    let _custody = codex_alab_core::native_fd_custody::receiving()?;
    let descriptor = OwnedFd::from(socket);
    validate_endpoint(&descriptor)?;
    from_descriptor(descriptor)
}

pub(super) fn from_descriptor(descriptor: OwnedFd) -> io::Result<UnixStream> {
    let socket = std::os::unix::net::UnixStream::from(descriptor);
    socket.set_nonblocking(true)?;
    UnixStream::from_std(socket)
}

pub(super) fn shutdown(socket: &UnixStream) {
    // SAFETY: shutdown only withdraws this actual owned channel, no PID or wire
    // identity selects another resource. It reports no resource settlement.
    unsafe {
        libc::shutdown(socket.as_raw_fd(), libc::SHUT_RDWR);
    }
}

pub(super) async fn receive_head(socket: &UnixStream) -> io::Result<ControlHead> {
    // Idle ownership has no arbitrary sixty-second expiry. Once an actual
    // descriptor arrives, one absolute deadline bounds the complete transaction.
    loop {
        socket.readable().await?;
        let mut tag = [0; 1];
        match socket.try_io(tokio::io::Interest::READABLE, || {
            receive_chunk_count(socket, &mut tag, None)
        }) {
            Err(error) if retry(&error) => {}
            Err(error) => return Err(error),
            Ok((count, mut descriptors)) => {
                if count != 1 {
                    return Err(unavailable());
                }
                let descriptor = match tag[0] {
                    b'A' if descriptors.len() == 1 => {
                        let descriptor = descriptors.pop().ok_or_else(unavailable)?;
                        validate_endpoint(&descriptor)?;
                        Some(descriptor)
                    }
                    b'H' | b'R' | b'S' if descriptors.is_empty() => None,
                    _ => return Err(unavailable()),
                };
                return Ok(ControlHead {
                    tag: tag[0],
                    descriptor,
                    deadline: Instant::now() + Duration::from_secs(60),
                });
            }
        }
    }
}

pub(super) async fn receive_byte(socket: &UnixStream) -> io::Result<u8> {
    loop {
        socket.readable().await?;
        let mut byte = [0];
        match socket.try_io(tokio::io::Interest::READABLE, || {
            receive_chunk(socket, &mut byte, 0)
        }) {
            Err(error) if retry(&error) => {}
            Err(error) => return Err(error),
            Ok((1, _)) => return Ok(byte[0]),
            Ok(_) => return Err(io::ErrorKind::UnexpectedEof.into()),
        }
    }
}

pub(super) async fn receive_nonce(socket: &UnixStream) -> io::Result<[u8; 32]> {
    let mut nonce = [0; 32];
    let mut offset = 0;
    while offset < nonce.len() {
        socket.readable().await?;
        match socket.try_io(tokio::io::Interest::READABLE, || {
            receive_chunk(socket, &mut nonce[offset..], 0)
        }) {
            Err(error) if retry(&error) => {}
            Err(error) => return Err(error),
            Ok((0, _)) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok((count, _)) => offset += count,
        }
    }
    if !std::str::from_utf8(&nonce).is_ok_and(|nonce| valid_hex(nonce, 32)) {
        return Err(unavailable());
    }
    Ok(nonce)
}

/// While awaiting queue space/ACK writability, native may send no further
/// payload. An original-channel EOF/error is withdrawal; any bytes/rights are
/// a protocol failure. Every received right is still adopted/closed safely.
pub(super) async fn owner_ended(socket: &UnixStream) -> io::Error {
    loop {
        if let Err(error) = socket.readable().await {
            return error;
        }
        let mut byte = [0; 1];
        match socket.try_io(tokio::io::Interest::READABLE, || {
            receive_chunk(socket, &mut byte, 0)
        }) {
            Err(error) if retry(&error) => {}
            Err(error) => return error,
            Ok((0, _)) => return io::ErrorKind::BrokenPipe.into(),
            Ok(_) => {
                return io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected backend control frame before receipt",
                );
            }
        }
    }
}
fn retry(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

pub(super) fn receive_chunk(
    stream: &UnixStream,
    payload: &mut [u8],
    expected: usize,
) -> io::Result<(usize, Vec<OwnedFd>)> {
    receive_chunk_count(stream, payload, Some(expected))
}

fn receive_chunk_count(
    stream: &UnixStream,
    payload: &mut [u8],
    expected: Option<usize>,
) -> io::Result<(usize, Vec<OwnedFd>)> {
    // This synchronous, nonblocking receipt shares the actual process-wide
    // guard held by raw exec transitions. Never carry it across an await.
    let _custody = codex_alab_core::native_fd_custody::receiving()?;
    // Match native fdpass's XNU UIPC_MAX_CMSG_FD capacity: XNU can install
    // rights before truncating copyout. Keep every installed right visible so
    // refusal can close it, despite accepting exactly one endpoint.
    let mut control =
        [0_usize; (512 * std::mem::size_of::<libc::c_int>() + 128) / std::mem::size_of::<usize>()];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    // SAFETY: zero initializes the optional address and ancillary fields.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = std::mem::size_of_val(&control) as _;
    // SAFETY: message borrows live, correctly aligned input/output buffers.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let flags = libc::MSG_CMSG_CLOEXEC;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let flags = 0;
    let count = unsafe { libc::recvmsg(stream.as_raw_fd(), &mut message, flags) };
    if count < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut descriptors = Vec::new();
    let mut malformed = false;
    // SAFETY: recvmsg populated this bounded, aligned ancillary buffer. All
    // transferred rights are immediately owned, including on rejection.
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&message);
        while !header.is_null() {
            let offset = (header as usize).checked_sub(control.as_ptr() as usize);
            let Some(remaining) = offset.and_then(|offset| {
                (message.msg_controllen as usize)
                    .min(std::mem::size_of_val(&control))
                    .checked_sub(offset)
            }) else {
                malformed = true;
                break;
            };
            if remaining < std::mem::size_of::<libc::cmsghdr>() {
                malformed = true;
                break;
            }
            let minimum = libc::CMSG_LEN(0) as usize;
            let length = (*header).cmsg_len as usize;
            if length < minimum || length > remaining {
                malformed = true;
                break;
            }
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                let bytes = length - minimum;
                if !bytes.is_multiple_of(std::mem::size_of::<libc::c_int>()) {
                    malformed = true;
                }
                for index in 0..bytes / std::mem::size_of::<libc::c_int>() {
                    let raw = libc::CMSG_DATA(header)
                        .cast::<libc::c_int>()
                        .add(index)
                        .read_unaligned();
                    if raw < 0 {
                        malformed = true;
                    } else {
                        descriptors.push(OwnedFd::from_raw_fd(raw));
                    }
                }
            } else {
                malformed = true;
            }
            header = libc::CMSG_NXTHDR(&message, header);
        }
    }
    for descriptor in &descriptors {
        // SAFETY: each right is now owned locally. Mark even rejected rights
        // before dropping them, following native fdpass's receipt contract.
        unsafe {
            let flags = libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD);
            if flags < 0
                || libc::fcntl(
                    descriptor.as_raw_fd(),
                    libc::F_SETFD,
                    flags | libc::FD_CLOEXEC,
                ) < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
    }
    if malformed
        || message.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0
        || expected.map_or(descriptors.len() > 1, |expected| {
            descriptors.len() != expected
        })
    {
        return Err(unavailable());
    }
    Ok((count as usize, descriptors))
}

pub(super) fn validate_endpoint(descriptor: &OwnedFd) -> io::Result<()> {
    // SAFETY: these getsockopt/fcntl calls only inspect/change this owned fd.
    unsafe {
        let mut kind: libc::c_int = 0;
        let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
        if libc::getsockopt(
            descriptor.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut length,
        ) < 0
            || kind != libc::SOCK_STREAM
        {
            return Err(unavailable());
        }
        let mut address: libc::sockaddr_storage = std::mem::zeroed();
        let mut length = std::mem::size_of_val(&address) as libc::socklen_t;
        if libc::getsockname(
            descriptor.as_raw_fd(),
            (&mut address as *mut libc::sockaddr_storage).cast(),
            &mut length,
        ) < 0
            || length < 2
            || i32::from(address.ss_family) != libc::AF_UNIX
        {
            return Err(unavailable());
        }
        length = std::mem::size_of_val(&address) as libc::socklen_t;
        if libc::getpeername(
            descriptor.as_raw_fd(),
            (&mut address as *mut libc::sockaddr_storage).cast(),
            &mut length,
        ) < 0
            || length < 2
            || i32::from(address.ss_family) != libc::AF_UNIX
        {
            return Err(unavailable());
        }
    }
    Ok(())
}
