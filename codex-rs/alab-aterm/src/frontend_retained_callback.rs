// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private physical callbacks. Peer equality is a consistency check against the
//! original authenticated control socket, not an SDK or foreground grant.
use super::*;
use crate::retained_fd::receive_chunk;
use crate::retained_fd::validate_endpoint;
use std::os::fd::OwnedFd;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt as _;
use tokio::net::UnixListener;

pub(super) struct Callback {
    listener: UnixListener,
    directory: PathBuf,
    pub(super) path: PathBuf,
}
impl Callback {
    pub(super) fn bind() -> io::Result<Self> {
        for _ in 0..16 {
            let directory = std::env::temp_dir().join(format!(
                "cx-{:016x}.noindex",
                codex_alab_core::native_source::connection_request_prefix()
            ));
            match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
                Ok(()) => {
                    let path = directory.join("s");
                    let bound = codex_alab_core::native_fd_custody::receiving()
                        .and_then(|_custody| UnixListener::bind(&path));
                    return match bound {
                        Ok(listener) => Ok(Self {
                            listener,
                            directory,
                            path,
                        }),
                        Err(error) => {
                            let _ = std::fs::remove_dir(&directory);
                            Err(error)
                        }
                    };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(unavailable())
    }

    pub(super) async fn accept(
        &self,
        transport: &source_member::Transport,
        peer: u32,
    ) -> io::Result<UnixStream> {
        loop {
            // poll_accept performs its accept/CLOEXEC work synchronously. The
            // guard drops before Pending yields; it is never held while idle.
            let (stream, _) = std::future::poll_fn(|context| {
                let _custody = match codex_alab_core::native_fd_custody::receiving() {
                    Ok(custody) => custody,
                    Err(error) => return std::task::Poll::Ready(Err(error)),
                };
                self.listener.poll_accept(context)
            })
            .await?;
            let actual = stream
                .peer_cred()?
                .pid()
                .and_then(|pid| u32::try_from(pid).ok());
            if actual == Some(peer) && transport.original_server_pid()? == peer {
                crate::conn::set_cloexec(&stream).map_err(|_| unavailable())?;
                return Ok(stream);
            }
            // An unrelated local connection never receives startup values or
            // supplies an attachment. Keep listening for the actual native peer.
        }
    }

    pub(super) async fn settings(
        &self,
        transport: &source_member::Transport,
        peer: u32,
        settings: &StartupSettings,
    ) -> io::Result<()> {
        // Native checks the callback's kernel peer after connect, before
        // reading settings. Retain the read side through its control response:
        // a fast write/full-close would race that peer observation on Darwin.
        loop {
            let mut stream = self.accept(transport, peer).await?;
            // Startup values require the still-live original authenticated
            // connection. Historical peer data only permits endpoint custody.
            if transport.server_pid()? != peer {
                return Err(unavailable());
            }
            settings.write_and_shutdown(&mut stream).await?;
            // Retain this one actual read side until native finishes it. A
            // Busy retry may then use the same callback again without keeping
            // old sockets or imposing a second request-attempt limit.
            let mut byte = [0; 1];
            loop {
                stream.readable().await?;
                match stream.try_io(tokio::io::Interest::READABLE, || {
                    receive_chunk(&stream, &mut byte, 0)
                }) {
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Ok((0, _)) => break,
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "unexpected native startup data",
                        ));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }

    pub(super) async fn endpoint(
        &self,
        transport: &source_member::Transport,
        peer: u32,
    ) -> io::Result<EndpointReceipt> {
        let callback = self
            .accept(transport, peer)
            .await
            .map_err(|error| io::Error::new(error.kind(), "retained callback peer check failed"))?;
        let descriptor = loop {
            callback.readable().await?;
            match callback.try_io(tokio::io::Interest::READABLE, || receive(&callback)) {
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                result => {
                    break result.map_err(|error| {
                        io::Error::new(error.kind(), "retained callback descriptor receipt failed")
                    })?;
                }
            }
        };
        // The first recvmsg consumed F and its unique descriptor. Accumulate
        // version+nonce without discarding any later ancillary rights.
        let mut tail = [0_u8; 33];
        let mut offset = 0;
        while offset < tail.len() {
            callback.readable().await?;
            match callback.try_io(tokio::io::Interest::READABLE, || {
                receive_chunk(&callback, &mut tail[offset..], 0)
            }) {
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Ok((0, _)) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok((count, _)) => offset += count,
                Err(error) => return Err(error),
            }
        }
        if tail[0] != b'1'
            || !std::str::from_utf8(&tail[1..]).is_ok_and(|nonce| valid_hex(nonce, 32))
        {
            return Err(unavailable());
        }
        let socket = std::os::unix::net::UnixStream::from(descriptor);
        socket.set_nonblocking(true)?;
        Ok(EndpointReceipt {
            stream: UnixStream::from_std(socket)?,
            callback,
            nonce: tail[1..].try_into().map_err(|_| unavailable())?,
        })
    }
}

pub(super) struct EndpointReceipt {
    stream: UnixStream,
    callback: UnixStream,
    nonce: [u8; 32],
}
impl EndpointReceipt {
    /// Retain actual endpoint custody BEFORE any receipt await. An uncertain
    /// nonce write, write-half-close or control reply never drops/recreates this delivery.
    pub(super) async fn acknowledge(
        mut self,
        received: &Mutex<Option<UnixStream>>,
    ) -> io::Result<()> {
        *received.lock().map_err(|_| unavailable())? = Some(self.stream);
        self.callback.write_all(&self.nonce).await?;
        // Close only the receipt writer: native requires EOF after exactly32
        // nonce bytes and rejects any trailing payload or rights. Its correlated
        // control reply follows actual receipt and final native checks. Source
        // never waits for callback read-EOF: native retains that transport.
        self.callback.shutdown().await
    }
}
impl Drop for Callback {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

pub(super) fn receive(stream: &UnixStream) -> io::Result<OwnedFd> {
    let mut byte = [0_u8];
    let (count, mut descriptors) = receive_chunk(stream, &mut byte, 1)?;
    if count != 1 || byte != *b"F" {
        return Err(unavailable());
    }
    let descriptor = descriptors.pop().ok_or_else(unavailable)?;
    validate_endpoint(&descriptor)?;
    Ok(descriptor)
}
