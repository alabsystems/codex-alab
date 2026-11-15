// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Public Responses exchanges retain negotiated compression across socket reuse.

use codex_api::ResponseEvent;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use std::io;
use std::io::Cursor;
use std::io::IoSlice;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::ReadBuf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_tungstenite::accept_async_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::extensions::compression::deflate::DeflateConfig;
use tokio_tungstenite::tungstenite::handshake::client::Response as HandshakeResponse;
use tokio_tungstenite::tungstenite::handshake::machine::TryParse;
use tokio_tungstenite::tungstenite::handshake::server::Request as HandshakeRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::protocol::frame::FrameSocket;
use tokio_tungstenite::tungstenite::protocol::frame::coding::Data;
use tokio_tungstenite::tungstenite::protocol::frame::coding::OpCode;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(60);
const MAX_WIRE: usize = 128 * 1024;
const CHILD: &str = "ALAB_RESPONSES_DEFLATE_CHILD";
const MODEL: &str = "gpt-deflate-contract";

#[derive(Default)]
struct Wire {
    client: Vec<u8>,
    server: Vec<u8>,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Wire>>);

fn append(destination: &mut Vec<u8>, bytes: &[u8]) {
    assert!(
        bytes.len() <= MAX_WIRE - destination.len(),
        "bounded fixture wire capture"
    );
    destination.extend_from_slice(bytes);
}

/// Observe only bytes the real socket read or accepted for writing. Flush,
/// shutdown, Pending and error results retain the underlying socket behavior.
struct WireTap {
    socket: TcpStream,
    capture: Capture,
}

impl AsyncRead for WireTap {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = output.filled().len();
        let result = Pin::new(&mut self.socket).poll_read(context, output);
        if matches!(result, Poll::Ready(Ok(()))) {
            append(
                &mut self.capture.0.lock().unwrap().client,
                &output.filled()[before..],
            );
        }
        result
    }
}

impl AsyncWrite for WireTap {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.socket).poll_write(context, bytes);
        if let Poll::Ready(Ok(written)) = result {
            append(
                &mut self.capture.0.lock().unwrap().server,
                &bytes[..written],
            );
        }
        result
    }

    fn is_write_vectored(&self) -> bool {
        self.socket.is_write_vectored()
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.socket).poll_write_vectored(context, buffers);
        if let Poll::Ready(Ok(written)) = result {
            let mut remaining = written;
            let mut capture = self.capture.0.lock().unwrap();
            for buffer in buffers {
                let take = buffer.len().min(remaining);
                append(&mut capture.server, &buffer[..take]);
                remaining -= take;
                if remaining == 0 {
                    break;
                }
            }
            assert_eq!(remaining, 0, "record exactly the accepted write prefix");
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(context)
    }
}

struct Peer(Option<JoinHandle<()>>);

impl Peer {
    async fn finish(mut self) {
        match tokio::time::timeout(WAIT, self.0.as_mut().unwrap()).await {
            Ok(result) => {
                drop(self.0.take());
                result.expect("two semantic Responses exchanges complete on one peer");
            }
            Err(_) => {
                let task = self.0.take().unwrap();
                task.abort();
                let _ = task.await;
                panic!("compression peer join deadline");
            }
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}

fn input_text(index: usize) -> String {
    format!("入力 {index}: {}", "café Καλημέρα 東京 🌍 ".repeat(24))
}

fn output_text(index: usize) -> String {
    format!("応答 {index}: {}", "naïve Ελληνικά 雪だるま ☃️ ".repeat(24))
}

fn response_id(index: usize) -> String {
    format!("resp_deflate_{index}")
}

async fn serve(listener: TcpListener, capture: Capture, compressed: bool) {
    let (socket, _) = tokio::time::timeout(WAIT, listener.accept())
        .await
        .expect("compression peer accept deadline")
        .unwrap();
    let mut config = WebSocketConfig::default();
    if compressed {
        config.extensions.permessage_deflate = Some(DeflateConfig::default());
    }
    // An explicit default config declines the production client's offer in the
    // baseline. Enabling the ordinary peer codec negotiates it in the positive.
    let mut websocket = tokio::time::timeout(
        WAIT,
        accept_async_with_config(WireTap { socket, capture }, Some(config)),
    )
    .await
    .expect("compression peer Upgrade deadline")
    .expect("ordinary Tungstenite negotiation");
    for index in 0..2 {
        let message = tokio::time::timeout(WAIT, websocket.next())
            .await
            .expect("model request receive deadline")
            .expect("one actual model request")
            .expect("peer decodes the client model request");
        let Message::Text(text) = message else {
            panic!("expected the public client's response.create text message");
        };
        let request: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(request["type"], "response.create");
        assert_eq!(request["model"], MODEL);
        assert_eq!(request["stream"], true);
        assert_eq!(request["input"][0]["content"][0]["text"], input_text(index));
        assert_eq!(
            request.get("previous_response_id").and_then(Value::as_str),
            if index == 0 {
                None
            } else {
                Some("resp_deflate_0")
            }
        );
        let events = [
            json!({"type":"response.created", "response":{"id":response_id(index)}}),
            json!({"type":"response.output_text.delta", "delta":output_text(index)}),
            json!({"type":"response.completed", "response":{
                "id":response_id(index),
                "usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18},
                "end_turn":true
            }}),
        ];
        for event in events {
            tokio::time::timeout(WAIT, websocket.send(Message::text(event.to_string())))
                .await
                .expect("semantic response send deadline")
                .expect("ordinary peer codec writes the response frame");
        }
    }
}

struct NoAuth;

impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

fn request(index: usize) -> codex_api::ResponsesApiRequest {
    codex_api::ResponsesApiRequest {
        model: MODEL.into(),
        stream: true,
        service_tier: None,
        input: vec![
            serde_json::from_value(json!({
                "type":"message", "role":"user",
                "content":[{"type":"input_text", "text":input_text(index)}]
            }))
            .unwrap(),
        ],
        tools: None,
        tool_choice: "auto".into(),
        parallel_tool_calls: true,
        reasoning: None,
        store: false,
        stream_options: None,
        include: vec![],
        prompt_cache_key: None,
        prompt_cache_options: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

fn extension_offered(headers: &http::HeaderMap) -> bool {
    headers
        .get_all("sec-websocket-extensions")
        .iter()
        .any(|header| {
            header.to_str().unwrap().split(',').any(|extension| {
                extension.split(';').next().unwrap().trim() == "permessage-deflate"
            })
        })
}

fn assert_frames(bytes: &[u8], client: bool, compressed: bool, expected_messages: usize) {
    let mut frames = FrameSocket::new(Cursor::new(bytes));
    let mut messages = 0;
    while let Some(frame) = frames
        .read(Some(MAX_WIRE))
        .expect("parse actual captured frame")
    {
        let header = frame.header();
        assert_eq!(
            header.mask.is_some(),
            client,
            "client masks; server does not"
        );
        assert!(!header.rsv2 && !header.rsv3);
        match header.opcode {
            OpCode::Data(Data::Text) => {
                messages += 1;
                assert_eq!(
                    header.rsv1, compressed,
                    "wire text must use the negotiated codec"
                );
            }
            OpCode::Data(Data::Continue) | OpCode::Control(_) => assert!(!header.rsv1),
            other => panic!("unexpected model protocol frame {other:?}"),
        }
    }
    assert_eq!(messages, expected_messages);
}

fn assert_wire(capture: &Capture, compressed: bool) {
    let wire = capture.0.lock().unwrap();
    let (request_length, request) = HandshakeRequest::try_parse(&wire.client)
        .unwrap()
        .expect("captured complete Upgrade request");
    let (response_length, response) = HandshakeResponse::try_parse(&wire.server)
        .unwrap()
        .expect("captured complete Upgrade response");
    assert_eq!(request.uri().path(), "/v1/responses");
    assert!(
        extension_offered(request.headers()),
        "production client offers compression in both cases"
    );
    assert_eq!(response.status().as_u16(), 101);
    assert_eq!(extension_offered(response.headers()), compressed);
    if compressed {
        let agreed = response.headers()["sec-websocket-extensions"]
            .to_str()
            .unwrap();
        assert!(
            !agreed.contains("no_context_takeover"),
            "both exchanges retain the negotiated context"
        );
    } else {
        assert!(!response.headers().contains_key("sec-websocket-extensions"));
    }
    assert_frames(&wire.client[request_length..], true, compressed, 2);
    assert_frames(&wire.server[response_length..], false, compressed, 6);
}

async fn exchange(compressed: bool) {
    let listener = tokio::time::timeout(WAIT, TcpListener::bind("127.0.0.1:0"))
        .await
        .expect("compression listener bind deadline")
        .unwrap();
    let address = listener.local_addr().unwrap();
    let capture = Capture::default();
    let peer = Peer(Some(tokio::spawn(serve(
        listener,
        capture.clone(),
        compressed,
    ))));
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "deflate-contract".into(),
            base_url: format!("http://{address}/v1"),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: WAIT,
        },
        Arc::new(NoAuth),
    );
    let factory = codex_http_client::HttpClientFactory::new(
        codex_http_client::OutboundProxyPolicy::ReqwestDefault,
    );
    assert!(!factory.allows_system_proxy_fallback());
    let connection = tokio::time::timeout(
        WAIT,
        client.connect(
            &factory,
            http::HeaderMap::new(),
            http::HeaderMap::new(),
            None,
            None,
        ),
    )
    .await
    .expect("public Responses Upgrade deadline")
    .expect("real public Responses connection");
    for index in 0..2 {
        let request = request(index);
        let mut create = codex_api::ResponseCreateWsRequest::from(&request);
        if index == 1 {
            create.previous_response_id = Some(response_id(0));
        }
        let mut stream = tokio::time::timeout(
            WAIT,
            connection.stream_request(
                codex_api::ResponsesWsRequest::ResponseCreate(create),
                index == 1,
                None,
            ),
        )
        .await
        .expect("public model request opening deadline")
        .expect("model request opens");
        let events = tokio::time::timeout(WAIT, async {
            let mut events = Vec::new();
            while let Some(event) = stream.rx_event.recv().await {
                events.push(event.expect("semantic response events decode"));
                assert!(events.len() <= 8, "bounded semantic fixture events");
            }
            events
        })
        .await
        .expect("public semantic response completion deadline");
        assert_eq!(events.len(), 3);
        assert!(
            matches!(&events[0], ResponseEvent::Created { response_id: Some(id) } if id == &response_id(index))
        );
        assert!(
            matches!(&events[1], ResponseEvent::OutputTextDelta(text) if text == &output_text(index))
        );
        let ResponseEvent::Completed {
            response_id: id,
            token_usage: Some(usage),
            end_turn: Some(true),
            ..
        } = &events[2]
        else {
            panic!("expected semantic completion with usage");
        };
        assert_eq!(id, &response_id(index));
        assert_eq!(usage.input_tokens, 11);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.total_tokens, 18);
    }
    peer.finish().await;
    assert_wire(&capture, compressed);
}

async fn isolated(name: &str, compressed: bool) {
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name));
        exchange(compressed).await;
        return;
    }
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture"])
        .env(CHILD, name);
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "CODEX_CA_CERTIFICATE",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        command.env_remove(key);
    }
    let report = crate::test_child_process::run(command, CHILD_WAIT)
        .await
        .unwrap_or_else(|failure| panic!("isolated websocket child: {failure}"));
    report.assert_test_success(&exact);
}

#[tokio::test]
async fn negotiated_deflate_preserves_public_responses_across_socket_reuse() {
    isolated(
        "negotiated_deflate_preserves_public_responses_across_socket_reuse",
        true,
    )
    .await;
}

#[tokio::test]
async fn declined_deflate_preserves_public_responses_across_socket_reuse() {
    isolated(
        "declined_deflate_preserves_public_responses_across_socket_reuse",
        false,
    )
    .await;
}
