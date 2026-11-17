// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual Unix/WS framing and real allocated ConnectionId carry one resource.
//! Arbitrary fixture custody is not an AcceptedOrigin or Native authorization.
use super::*;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::AcceptedConnection;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;
use futures::FutureExt;
use futures::SinkExt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

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
async fn native_envelope_resource_reaches_same_real_websocket_connection_event() {
    let (send, receive) = mpsc::channel(1);
    let incoming = Incoming::new(Provider {
        queue: Mutex::new(receive),
    });
    let stop = CancellationToken::new();
    let (events, mut received) = mpsc::channel(8);
    let acceptor = start_native_incoming_acceptor(
        incoming.clone(),
        events,
        stop.clone(),
        WebSocketCompression::Plain,
    );
    let result = std::panic::AssertUnwindSafe(async {
        let actual = Arc::new(());
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        send.send(AcceptedConnection::with_resource(
            server,
            codex_alab_core::connection_resource::ConnectionResource::new(actual.clone()),
        ))
        .await
        .unwrap();
        let (mut wire, _) = timeout(
            Duration::from_secs(5),
            tokio_tungstenite::client_async("ws://localhost/rpc", client),
        )
        .await
        .unwrap()
        .unwrap();
        let opened = timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap()
            .unwrap();
        let connection = match opened {
            TransportEvent::ConnectionOpened {
                connection_id,
                origin,
                auth,
                resource,
                ..
            } => {
                assert_eq!(origin, super::super::ConnectionOrigin::WebSocket);
                assert!(auth.is_none());
                let actual_resource = resource.unwrap().downcast::<Arc<()>>().unwrap();
                assert!(Arc::ptr_eq(&actual, actual_resource.as_ref()));
                connection_id
            }
            _ => panic!("actual WebSocket did not open"),
        };
        wire.send(tokio_tungstenite::tungstenite::Message::Text(
            "{\"method\":\"initialized\"}".into(),
        ))
        .await
        .unwrap();
        match timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap()
            .unwrap()
        {
            TransportEvent::IncomingMessage { connection_id, .. } => {
                assert_eq!(connection_id, connection)
            }
            _ => panic!("actual JSON-RPC did not preserve the real connection"),
        }
        wire.close(None).await.unwrap();
    })
    .catch_unwind()
    .await;
    incoming.stop();
    stop.cancel();
    let cleaned = timeout(Duration::from_secs(5), acceptor).await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    cleaned.unwrap().unwrap();
}
