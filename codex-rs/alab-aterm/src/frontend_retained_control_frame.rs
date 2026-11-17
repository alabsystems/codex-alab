// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Fixed physical frames from the SAME inherited Native control channel.
//! These bytes are selection/correlation data; no label creates permission.
use super::fd;
use codex_alab_core::managed_auth::unavailable;
use std::io;
use std::os::fd::OwnedFd;
use tokio::net::UnixStream;
use tokio::time::Instant;

pub(super) enum Frame {
    Attachment {
        descriptor: OwnedFd,
        deadline: Instant,
    },
    Hold(Hold),
    Runtime(Runtime),
    SourceInhibit(Source),
    SourceSelection(Source),
}
pub(super) struct Hold {
    pub(super) operation: [u8; 32],
    pub(super) nonce: [u8; 32],
    pub(super) deadline: Instant,
}
pub(super) struct Source {
    pub(super) parent: [u8; 32],
    pub(super) operation: [u8; 32],
    pub(super) nonce: [u8; 32],
    pub(super) attachment: [u8; 32],
    pub(super) deadline: Instant,
}
pub(super) struct Runtime {
    pub(super) go: Option<[u8; 32]>,
    pub(super) frontier: [u8; 32],
    pub(super) nonce: [u8; 32],
    pub(super) attachment: [u8; 32],
    pub(super) deadline: Instant,
}

pub(super) async fn receive(socket: &UnixStream) -> io::Result<Frame> {
    let head = fd::receive_head(socket).await?;
    if head.tag == b'A' {
        return Ok(Frame::Attachment {
            descriptor: head.descriptor.ok_or_else(unavailable)?,
            deadline: head.deadline,
        });
    }
    // Every byte/ancillary fragment uses the guarded decoder. A right attached
    // to any H/R header or body is rejected/closed, never discarded by Read.
    tokio::time::timeout_at(head.deadline, async {
        let version = fd::receive_byte(socket).await?;
        match (head.tag, version) {
            (b'H', b'1') => Ok(Frame::Hold(Hold {
                operation: fd::receive_nonce(socket).await?,
                nonce: fd::receive_nonce(socket).await?,
                deadline: head.deadline,
            })),
            (b'R', b'0' | b'1') => Ok(Frame::Runtime(Runtime {
                go: if version == b'1' {
                    Some(fd::receive_nonce(socket).await?)
                } else {
                    None
                },
                frontier: fd::receive_nonce(socket).await?,
                nonce: fd::receive_nonce(socket).await?,
                attachment: fd::receive_nonce(socket).await?,
                deadline: head.deadline,
            })),
            (b'S', b'1' | b'2') => {
                let packet = Source {
                    parent: fd::receive_nonce(socket).await?,
                    operation: fd::receive_nonce(socket).await?,
                    nonce: fd::receive_nonce(socket).await?,
                    attachment: fd::receive_nonce(socket).await?,
                    deadline: head.deadline,
                };
                Ok(if version == b'1' {
                    Frame::SourceInhibit(packet)
                } else {
                    Frame::SourceSelection(packet)
                })
            }
            _ => Err(unavailable()),
        }
    })
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

#[cfg(test)]
#[path = "frontend_retained_control_frame_tests.rs"]
mod tests;
