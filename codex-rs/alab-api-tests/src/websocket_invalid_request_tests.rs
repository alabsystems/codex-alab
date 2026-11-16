// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Statusless request refusals share the parser without replacing wrapped HTTP status.

use codex_api::ApiError;
use codex_http_client::TransportError;
use serde_json::json;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(5);
const CHILD: &str = "ALAB_WS_INVALID_REQUEST_CHILD";
const COMPLETE: &str = "ALAB_WS_INVALID_REQUEST_OK\n";
const PRIVATE: &str = "private-websocket-invalid-request-message";

struct NoAuth;

impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

fn request() -> codex_api::ResponsesApiRequest {
    codex_api::ResponsesApiRequest {
        model: "gpt-ws-invalid-request".into(),
        stream: true,
        service_tier: None,
        input: vec![
            serde_json::from_value(json!({
                "type":"message", "role":"user",
                "content":[{"type":"input_text", "text":PRIVATE}]
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

async fn exchange(status: Option<u16>) {
    exchange_error("invalid_request", status, None).await;
}

async fn exchange_error(code: &str, status: Option<u16>, status_alias: Option<&str>) {
    let mut frame = json!({"type":"error", "error":{
        "code":code, "message":PRIVATE,
        "headers":{"retry-after":"7"}
    }});
    if let Some(status) = status {
        frame["status"] = json!(status);
    }
    if let Some(alias) = status_alias {
        frame["status_code"] = json!(alias);
    }
    let original = frame.to_string();
    let mut peer = crate::websocket_fixture::WebsocketFixture::start(frame).await;
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "websocket-invalid-request".into(),
            base_url: format!("{}/v1", peer.uri()),
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
    .expect("WebSocket connection deadline")
    .expect("one loopback WebSocket connection");
    let mut stream = tokio::time::timeout(
        WAIT,
        connection.stream_request(
            codex_api::ResponsesWsRequest::ResponseCreate(
                codex_api::ResponseCreateWsRequest::from(&request()),
            ),
            false,
            None,
        ),
    )
    .await
    .expect("WebSocket request deadline")
    .expect("one public stream request");
    let observed = peer.observed_request().await;
    assert_eq!(observed["type"], "response.create");
    assert_eq!(observed["model"], "gpt-ws-invalid-request");
    assert_eq!(observed["input"][0]["content"][0]["text"], PRIVATE);
    let before = Instant::now();
    peer.release_failure();
    let error = match tokio::time::timeout(WAIT, stream.rx_event.recv()).await {
        Ok(Some(Err(error))) => error,
        other => panic!("expected selected WebSocket error: {other:?}"),
    };
    let after = Instant::now();
    match status.filter(|_| status_alias.is_none()) {
        None => {
            assert!(
                matches!((code, &error),
                    ("invalid_request", ApiError::InvalidRequest { message })
                    | ("invalid_prompt", ApiError::InvalidPrompt { message }) if message == PRIVATE),
                "statusless frame must retain its exact refusal: {error:?}"
            );
            let mapped = codex_api::map_api_error(error);
            assert_eq!(mapped.to_string(), PRIVATE);
            assert!(mapped.retry_delay(1).is_none());
            assert!(mapped.retry_after().is_none());
        }
        Some(expected) => {
            let ApiError::Transport(TransportError::Http {
                status,
                body,
                retry_after,
                ..
            }) = error
            else {
                panic!("explicit HTTP status must retain its transport route");
            };
            assert_eq!(status.as_u16(), expected);
            assert_eq!(body.as_deref(), Some(original.as_str()));
            let deadline = retry_after.expect("nested provider advice").deadline();
            assert!(deadline >= before + Duration::from_secs(7));
            assert!(deadline <= after + Duration::from_secs(7));
        }
    }
    assert!(
        tokio::time::timeout(WAIT, stream.rx_event.recv())
            .await
            .expect("selected error closes the event channel")
            .is_none()
    );
    peer.finish().await;
    if code == "invalid_request" {
        println!("ALAB_WS_INVALID_REQUEST_CASE:{status:?}");
    } else {
        println!("ALAB_WS_INVALID_PROMPT_CASE:{status:?}:{status_alias:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_invalid_request_preserves_websocket_status_precedence() {
    let name = "standalone_invalid_request_preserves_websocket_status_precedence";
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name));
        assert_eq!(
            std::env::args().skip(1).collect::<Vec<_>>(),
            ["--exact", &exact, "--nocapture", "--test-threads=1"]
        );
        codex_alab_core::activate();
        // Run unchanged status routes first so the RED also exercises them.
        for status in [Some(400), Some(503), None] {
            exchange(status).await;
        }
        std::io::stdout()
            .lock()
            .write_all(COMPLETE.as_bytes())
            .unwrap();
        return;
    }
    assert!(!codex_alab_core::active());
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
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
    crate::test_child_process::run_with_stdout_marker(
        command,
        Duration::from_secs(30),
        Some(COMPLETE),
    )
    .await
    .unwrap_or_else(|failure| panic!("isolated WebSocket invalid-request child: {failure}"))
    .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
}

async fn isolated_prompt_cases(name: &str, cases: &[(Option<u16>, Option<&str>)]) {
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let marker = format!("ALAB_WS_INVALID_PROMPT_OK:{name}\n");
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name));
        assert_eq!(
            std::env::args().skip(1).collect::<Vec<_>>(),
            ["--exact", &exact, "--nocapture", "--test-threads=1"]
        );
        codex_alab_core::activate();
        for &(status, alias) in cases {
            exchange_error("invalid_prompt", status, alias).await;
        }
        std::io::stdout()
            .lock()
            .write_all(marker.as_bytes())
            .unwrap();
        return;
    }
    assert!(!codex_alab_core::active());
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD, name);
    for key in ["DYLD_LIBRARY_PATH", "LD_LIBRARY_PATH"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    crate::test_child_process::run_with_stdout_marker(
        command,
        Duration::from_secs(30),
        Some(&marker),
    )
    .await
    .unwrap_or_else(|failure| panic!("isolated WebSocket prompt child: {failure}"))
    .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_invalid_prompt_preserves_websocket_status_precedence() {
    isolated_prompt_cases(
        "standalone_invalid_prompt_preserves_websocket_status_precedence",
        &[
            (Some(400), None),
            (Some(401), None),
            (Some(503), None),
            (None, None),
        ],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_websocket_401_keeps_invalid_prompt_request_only() {
    // Ambiguous status metadata cannot invent an HTTP identity or authorize a
    // login/replay; only the independently validated request refusal is retained.
    isolated_prompt_cases(
        "malformed_websocket_401_keeps_invalid_prompt_request_only",
        &[(Some(401), Some("bad"))],
    )
    .await;
}
