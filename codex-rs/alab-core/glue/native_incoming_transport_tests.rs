// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! These are wire/ownership controls, not evidence of native SDK admission.
use super::*;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCNotification;
use futures::SinkExt;
use std::sync::Mutex;
use tokio::net::UnixStream;
use tokio_tungstenite::client_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

struct Provider {
    queue: Mutex<mpsc::Receiver<UnixStream>>,
    owner: CancellationToken,
}
impl IncomingStreamProvider for Provider {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::poll_fn(|context| match self.queue.lock() {
            Ok(mut queue) => queue.poll_recv(context).map(|next| {
                Ok(next.map(codex_alab_core::native_incoming::AcceptedConnection::stream_only))
            }),
            Err(_) => std::task::Poll::Ready(Err(io::Error::other("fixture queue poisoned"))),
        }))
    }
    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(self.owner.cancelled())
    }
}
struct Fixture {
    incoming: Incoming,
    queue: mpsc::Sender<UnixStream>,
    owner: CancellationToken,
    shutdown: CancellationToken,
    events: mpsc::Receiver<TransportEvent>,
    acceptor: JoinHandle<()>,
}
impl Fixture {
    fn new(compression: WebSocketCompression) -> Self {
        let (queue, receive) = mpsc::channel(4);
        let owner = CancellationToken::new();
        let incoming = Incoming::new(Provider {
            queue: Mutex::new(receive),
            owner: owner.clone(),
        });
        let shutdown = CancellationToken::new();
        let (send, events) = mpsc::channel(16);
        let acceptor =
            start_native_incoming_acceptor(incoming.clone(), send, shutdown.clone(), compression);
        Self {
            incoming,
            queue,
            owner,
            shutdown,
            events,
            acceptor,
        }
    }
    async fn connect(&self) -> tokio_tungstenite::WebSocketStream<UnixStream> {
        let (client, server) = UnixStream::pair().unwrap();
        self.queue.send(server).await.unwrap();
        timeout(
            Duration::from_secs(2),
            client_async("ws://localhost/rpc", client),
        )
        .await
        .unwrap()
        .unwrap()
        .0
    }
    async fn event(&mut self) -> TransportEvent {
        timeout(Duration::from_secs(2), self.events.recv())
            .await
            .unwrap()
            .unwrap()
    }
}

#[tokio::test]
async fn native_incoming_reuses_real_connection_ids_events_and_websocket_ping() {
    let mut fixture = Fixture::new(WebSocketCompression::Plain);
    let mut first = fixture.connect().await;
    let first_id = match fixture.event().await {
        TransportEvent::ConnectionOpened {
            connection_id,
            auth,
            ..
        } => {
            assert!(auth.is_none());
            connection_id
        }
        _ => panic!("expected actual connection opened"),
    };
    let notification = JSONRPCMessage::Notification(JSONRPCNotification {
        method: "initialized".to_owned(),
        params: None,
    });
    first
        .send(Message::Text(
            serde_json::to_string(&notification).unwrap().into(),
        ))
        .await
        .unwrap();
    match fixture.event().await {
        TransportEvent::IncomingMessage {
            connection_id,
            message,
        } => {
            assert_eq!(connection_id, first_id);
            assert_eq!(message, notification);
        }
        _ => panic!("expected existing transport message routing"),
    }
    first
        .send(Message::Ping(b"native".to_vec().into()))
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(2), first.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Message::Pong(b"native".to_vec().into())
    );
    first.close(None).await.unwrap();
    assert!(
        matches!(fixture.event().await, TransportEvent::ConnectionClosed { connection_id } if connection_id == first_id)
    );
    assert!(
        matches!(fixture.event().await, TransportEvent::AlabIngressDrained { connection_id } if connection_id == first_id)
    ); // ALAB(retained-ui): actual joined reader cut precedes next connection.
    let _second = fixture.connect().await;
    match fixture.event().await {
        TransportEvent::ConnectionOpened { connection_id, .. } => {
            assert_ne!(connection_id, first_id)
        }
        _ => panic!("second descriptor must get its own connection in the same acceptor"),
    }
    fixture.shutdown.cancel();
    timeout(Duration::from_secs(2), fixture.acceptor)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn native_incoming_owner_withdrawal_closes_active_and_stalled_handshake_sockets() {
    let mut fixture = Fixture::new(WebSocketCompression::Plain);
    let mut active = fixture.connect().await;
    let (active_id, writer) = match fixture.event().await {
        TransportEvent::ConnectionOpened {
            connection_id,
            writer,
            ..
        } => (connection_id, writer),
        _ => panic!("expected active connection"),
    };
    let (mut stalled, server) = UnixStream::pair().unwrap();
    fixture.queue.send(server).await.unwrap();
    timeout(Duration::from_secs(2), async {
        while fixture.queue.capacity() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fixture.owner.cancel();
    timeout(Duration::from_secs(2), fixture.acceptor)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(fixture.events.recv().await, Some(TransportEvent::ConnectionClosed { connection_id }) if connection_id == active_id)
    );
    timeout(Duration::from_secs(2), writer.closed())
        .await
        .unwrap();
    let message = timeout(Duration::from_secs(2), active.next())
        .await
        .unwrap();
    assert!(!matches!(
        message,
        Some(Ok(Message::Text(_) | Message::Binary(_)))
    ));
    use tokio::io::AsyncReadExt;
    let mut byte = [0];
    assert_eq!(
        timeout(Duration::from_secs(2), stalled.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    timeout(Duration::from_secs(2), fixture.incoming.withdrawn())
        .await
        .unwrap();
}

#[tokio::test]
async fn native_incoming_negotiates_existing_zstd_and_advertised_frame_limit() {
    let fixture = Fixture::new(WebSocketCompression::Zstd);
    let (client, server) = UnixStream::pair().unwrap();
    fixture.queue.send(server).await.unwrap();
    let mut request = "ws://localhost/rpc".into_client_request().unwrap();
    request.headers_mut().insert(
        WEBSOCKET_COMPRESSION_HEADER,
        HeaderValue::from_static(ZSTD_STREAM_PROTOCOL),
    );
    let (_client, response) = client_async(request, client).await.unwrap();
    assert_eq!(
        response.headers()[WEBSOCKET_COMPRESSION_HEADER],
        ZSTD_STREAM_PROTOCOL
    );
    assert!(
        response
            .headers()
            .contains_key("x-codex-websocket-max-unfragmented-message-bytes")
    );
    fixture.shutdown.cancel();
    timeout(Duration::from_secs(2), fixture.acceptor)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn native_incoming_stalled_handshake_is_bounded_and_queue_end_withdraws() {
    use tokio::io::AsyncReadExt;
    let fixture = Fixture::new(WebSocketCompression::Plain);
    let (mut client, server) = UnixStream::pair().unwrap();
    fixture.queue.send(server).await.unwrap();
    let mut byte = [0];
    assert_eq!(client.read(&mut byte).await.unwrap(), 0);
    drop(fixture.queue);
    timeout(Duration::from_secs(2), fixture.incoming.withdrawn())
        .await
        .unwrap();
    timeout(Duration::from_secs(2), fixture.acceptor)
        .await
        .unwrap()
        .unwrap();
}
