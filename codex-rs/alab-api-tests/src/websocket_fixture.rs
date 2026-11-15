// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One real websocket request with independent observed-request and response gates.

use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

const WAIT: Duration = Duration::from_secs(5);

pub(crate) struct WebsocketFixture {
    uri: String,
    observed: oneshot::Receiver<Value>,
    release: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl WebsocketFixture {
    pub(crate) async fn start(failure: Value) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback websocket listener");
        let uri = format!("ws://{}", listener.local_addr().unwrap());
        let (observed, request) = oneshot::channel();
        let (release, gate) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (socket, _) = tokio::time::timeout(WAIT, listener.accept())
                .await
                .expect("websocket accept deadline")
                .expect("one loopback connection");
            let mut socket = tokio::time::timeout(WAIT, accept_async(socket))
                .await
                .expect("websocket handshake deadline")
                .expect("real websocket handshake");
            let request = tokio::time::timeout(WAIT, async {
                loop {
                    match socket
                        .next()
                        .await
                        .expect("one outgoing request")
                        .expect("websocket request frame")
                    {
                        Message::Text(text) => {
                            break serde_json::from_str::<Value>(&text).expect("JSON request");
                        }
                        Message::Ping(data) => socket
                            .send(Message::Pong(data))
                            .await
                            .expect("websocket pong"),
                        Message::Pong(_) => {}
                        _ => panic!("expected a websocket text request"),
                    }
                }
            })
            .await
            .expect("websocket request deadline");
            assert!(
                observed.send(request).is_ok(),
                "test observes the actual request"
            );
            // Never send the scripted error until the test confirms the caller's
            // subscriber and parent spans have exited. No protocol parsing is copied.
            tokio::time::timeout(WAIT, gate)
                .await
                .expect("response release deadline")
                .expect("caller context exited");
            tokio::time::timeout(WAIT, socket.send(Message::Text(failure.to_string().into())))
                .await
                .expect("websocket response send deadline")
                .expect("scripted semantic failure");
            // The consumer can retire the socket immediately after the terminal
            // error. Closing is bounded cleanup, not another protocol assertion.
            let _ = tokio::time::timeout(WAIT, socket.close(None)).await;
        });
        Self {
            uri,
            observed: request,
            release: Some(release),
            task: Some(task),
        }
    }

    pub(crate) fn uri(&self) -> &str {
        &self.uri
    }

    pub(crate) async fn observed_request(&mut self) -> Value {
        tokio::time::timeout(WAIT, &mut self.observed)
            .await
            .expect("observed request deadline")
            .expect("server observed request")
    }

    pub(crate) fn release_failure(&mut self) {
        self.release
            .take()
            .expect("one response release")
            .send(())
            .expect("server is waiting for caller context exit");
    }

    pub(crate) async fn finish(mut self) {
        // Leave the handle owned by self while waiting, so cancellation of
        // finish also reaches Drop and aborts the fixture task.
        match tokio::time::timeout(WAIT, self.task.as_mut().expect("one fixture task")).await {
            Ok(result) => {
                drop(self.task.take());
                result.expect("websocket fixture completion");
            }
            Err(_) => {
                let task = self.task.take().expect("one fixture task");
                task.abort();
                let _ = task.await;
                panic!("websocket fixture cleanup exceeded its deadline");
            }
        }
    }
}

impl Drop for WebsocketFixture {
    fn drop(&mut self) {
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
    }
}
