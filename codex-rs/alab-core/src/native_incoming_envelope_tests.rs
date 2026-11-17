// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Unix endpoint/resource custody; no origin type or SDK permission.
use super::*;
use std::sync::Mutex;
use std::sync::PoisonError;
use tokio::sync::mpsc;

struct Provider {
    queue: Mutex<mpsc::Receiver<AcceptedConnection>>,
}
impl IncomingStreamProvider for Provider {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::poll_fn(|cx| {
            self.queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .poll_recv(cx)
                .map(Ok)
        }))
    }
    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn cancelled_accept_keeps_exact_queued_socket_and_resource_for_next_observer() {
    let (send, receive) = mpsc::channel(1);
    let input = Incoming::new(Provider {
        queue: Mutex::new(receive),
    });
    let mut first = Box::pin(input.accept_envelope());
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(first.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(first);
    let actual = Arc::new(());
    let (mut peer, socket) = UnixStream::pair().unwrap();
    send.send(AcceptedConnection::with_resource(
        socket,
        crate::connection_resource::ConnectionResource::new(actual.clone()),
    ))
    .await
    .unwrap();
    let accepted = input.accept_envelope().await.unwrap().unwrap();
    let (mut stream, resource) = accepted.into_parts();
    let resource = resource.unwrap();
    let same = resource.downcast::<Arc<()>>().unwrap();
    assert!(Arc::ptr_eq(&actual, same.as_ref()));
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    peer.write_all(b"actual endpoint").await.unwrap();
    let mut bytes = [0; 15];
    stream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"actual endpoint");
    input.stop();
}

#[tokio::test]
async fn prep_only_accept_intentionally_drops_resource_and_keeps_same_stream() {
    let (send, receive) = mpsc::channel(1);
    let input = Incoming::new(Provider {
        queue: Mutex::new(receive),
    });
    let actual = Arc::new(());
    let observed = Arc::downgrade(&actual);
    let (mut peer, socket) = UnixStream::pair().unwrap();
    send.send(AcceptedConnection::with_resource(
        socket,
        crate::connection_resource::ConnectionResource::new(actual),
    ))
    .await
    .unwrap();
    let mut stream = input.accept().await.unwrap().unwrap();
    assert!(
        observed.upgrade().is_none(),
        "prep compatibility must not retain/reconstruct an origin"
    );
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    peer.write_all(b"prep").await.unwrap();
    let mut bytes = [0; 4];
    stream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"prep");
    input.stop();
}
