// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::Mutex;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

struct Provider {
    queue: Mutex<mpsc::Receiver<UnixStream>>,
    owner: watch::Receiver<bool>,
}
impl IncomingStreamProvider for Provider {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::poll_fn(|context| match self.queue.lock() {
            Ok(mut queue) => queue
                .poll_recv(context)
                .map(|next| Ok(next.map(AcceptedConnection::stream_only))),
            Err(_) => std::task::Poll::Ready(Err(io::Error::other("fixture queue poisoned"))),
        }))
    }
    fn withdrawn(&self) -> Withdrawal<'_> {
        let mut owner = self.owner.clone();
        Box::pin(async move {
            while !*owner.borrow_and_update() {
                if owner.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}
fn fixture() -> (Incoming, mpsc::Sender<UnixStream>, watch::Sender<bool>) {
    let (send, receive) = mpsc::channel(2);
    let (owner, withdrawn) = watch::channel(false);
    (
        Incoming::new(Provider {
            queue: Mutex::new(receive),
            owner: withdrawn,
        }),
        send,
        owner,
    )
}

#[tokio::test]
async fn cancelled_accept_preserves_actual_queued_endpoint() {
    let (incoming, send, _owner) = fixture();
    let mut pending = Box::pin(incoming.accept());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1), &mut pending)
            .await
            .is_err()
    );
    drop(pending);
    let (mut client, server) = UnixStream::pair().unwrap();
    send.send(server).await.unwrap();
    let mut actual = incoming.accept().await.unwrap().unwrap();
    client.write_all(b"custody").await.unwrap();
    let mut bytes = [0; 7];
    actual.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"custody");
}

#[tokio::test]
async fn queue_end_and_actual_owner_withdrawal_are_cleanup_inputs() {
    let (incoming, send, _owner) = fixture();
    drop(send);
    assert!(incoming.accept().await.unwrap().is_none());
    tokio::time::timeout(std::time::Duration::from_secs(1), incoming.withdrawn())
        .await
        .unwrap();
    let (incoming, _send, owner) = fixture();
    owner.send_replace(true);
    tokio::time::timeout(std::time::Duration::from_secs(1), incoming.withdrawn())
        .await
        .unwrap();
}

#[tokio::test]
async fn local_stop_is_sticky_and_clones_retain_the_same_provider() {
    let (incoming, _send, _owner) = fixture();
    assert_eq!(incoming, incoming.clone());
    let (other, _other_send, _other_owner) = fixture();
    assert_ne!(incoming, other);
    incoming.stop();
    assert!(incoming.clone().accept().await.unwrap().is_none());
    tokio::time::timeout(std::time::Duration::from_secs(1), incoming.withdrawn())
        .await
        .unwrap();
    assert!(incoming.failure().is_none());
}

struct FailingProvider;
impl IncomingStreamProvider for FailingProvider {
    fn accept(&self) -> Accept<'_> {
        Box::pin(async { Err(io::Error::from_raw_os_error(libc::ECONNRESET)) })
    }
    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn provider_failure_survives_withdrawal_and_retains_the_first_actual_error() {
    let incoming = Incoming::new(FailingProvider);
    assert_eq!(
        incoming.accept().await.unwrap_err().kind(),
        io::ErrorKind::ConnectionReset
    );
    incoming.withdrawn().await;
    let first = incoming.failure().unwrap();
    assert_eq!(first.raw_os_error(), Some(libc::ECONNRESET));
    incoming.fail(io::Error::other("later cleanup failure"));
    assert!(Arc::ptr_eq(&first, &incoming.clone().failure().unwrap()));
    assert!(incoming.accept().await.unwrap().is_none());
}
