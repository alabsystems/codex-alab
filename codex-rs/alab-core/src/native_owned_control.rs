// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Physical backend bootstrap binding, separate from Source and RPC admission.
use crate::managed_auth::unavailable;
use crate::managed_auth::valid_hex;
use crate::native_source::Member;
use std::io;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;

const IO_TIMEOUT: Duration = Duration::from_secs(60);

/// Retains the actual socket and its original SDK member. Native code must
/// independently observe the binding and SDK construction before attachment.
pub struct Bound {
    stream: UnixStream,
    _member: Arc<dyn Member>,
}
impl Bound {
    pub(crate) fn retains_member(&self, member: &Arc<dyn Member>) -> bool {
        Arc::ptr_eq(&self._member, member)
    }

    pub(super) fn into_parts(self) -> (UnixStream, Arc<dyn Member>) {
        (self.stream, self._member)
    }

    pub fn stream(&mut self) -> &mut UnixStream {
        &mut self.stream
    }
}
impl std::fmt::Debug for Bound {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("NativeOwnedControl(<private>)")
    }
}

/// Consume the actual private socket, receive its native challenge, and
/// acknowledge it on this constructor's original authenticated SDK channel.
/// Failure closes the socket; it never falls back to a public locator or a
/// claimed process identity. This grants neither source use nor attachment.
pub async fn bind(member: Arc<dyn Member>, stream: StdUnixStream) -> io::Result<Bound> {
    if !member.live() {
        return Err(unavailable());
    }
    let operation = format!("{:032x}", rand::random::<u128>());
    member.begin_owned_control(&operation).await?;
    let (stream, nonce) = receive(stream).await?;
    member.acknowledge_owned_control(&operation, &nonce).await?;
    if !member.live() {
        return Err(unavailable());
    }
    Ok(Bound {
        stream,
        _member: member,
    })
}

async fn receive(stream: StdUnixStream) -> io::Result<(UnixStream, String)> {
    stream.set_nonblocking(true)?;
    let mut stream = UnixStream::from_std(stream)?;
    let mut frame = [0; 40];
    tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut frame))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    let nonce = std::str::from_utf8(&frame[8..]).map_err(|_| unavailable())?;
    if &frame[..8] != b"ALABCTL1" || !valid_hex(nonce, 32) {
        return Err(unavailable());
    }
    let nonce = nonce.to_owned();
    Ok((stream, nonce))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn fragmented_challenge_keeps_following_socket_bytes_unconsumed() {
        let (reader, mut writer) = StdUnixStream::pair().unwrap();
        let producer = std::thread::spawn(move || {
            writer.write_all(b"ALAB").unwrap();
            writer.write_all(b"CTL1").unwrap();
            writer
                .write_all(b"0123456789abcdef0123456789abcdefnext")
                .unwrap();
        });
        let (mut stream, nonce) = receive(reader).await.unwrap();
        assert_eq!(nonce, "0123456789abcdef0123456789abcdef");
        let mut following = [0; 4];
        stream.read_exact(&mut following).await.unwrap();
        assert_eq!(&following, b"next");
        producer.join().unwrap();
    }

    #[tokio::test]
    async fn malformed_or_truncated_challenge_never_binds() {
        for bytes in [
            b"BADFRAME0123456789abcdef0123456789abcdef".as_slice(),
            b"ALABCTL1g123456789abcdef0123456789abcdef",
            b"ALABCTL10123456789abcdef",
        ] {
            let (reader, mut writer) = StdUnixStream::pair().unwrap();
            writer.write_all(bytes).unwrap();
            drop(writer);
            assert!(receive(reader).await.is_err());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_partial_challenge_closes_its_actual_socket() {
        let (reader, mut writer) = StdUnixStream::pair().unwrap();
        writer.write_all(b"ALABCTL1").unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), receive(reader))
                .await
                .is_err()
        );
        writer.set_nonblocking(true).unwrap();
        let mut writer = UnixStream::from_std(writer).unwrap();
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), writer.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0,
            "cancelling bootstrap must close its physical endpoint"
        );
    }
}
