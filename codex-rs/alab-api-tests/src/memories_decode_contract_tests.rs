// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A completed unary HTTP operation is distinct from decoding its output.

use codex_api::ApiError;
use codex_api::MemorySummarizeInput;
use codex_api::MemorySummarizeOutput;
use codex_api::RawMemory;
use codex_api::RawMemoryMetadata;
use codex_api::Reasoning;
use codex_api::ReasoningContext;
use codex_api::RequestTelemetry;
use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::Response;
use codex_http_client::StreamResponse;
use codex_http_client::TransportError;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ReasoningEffort;
use http::HeaderMap;
use http::StatusCode;
use serde_json::json;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Auth(AtomicUsize);

impl codex_api::AuthProvider for Auth {
    fn add_auth_headers(&self, headers: &mut HeaderMap) -> Result<(), codex_api::AuthError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        headers.insert(
            http::header::AUTHORIZATION,
            "Bearer private-memory-auth".parse().unwrap(),
        );
        Ok(())
    }
}

struct Script {
    response: Option<Vec<u8>>,
    requests: Vec<Request>,
}

struct Transport {
    script: Arc<Mutex<Script>>,
    stream_calls: Arc<AtomicUsize>,
}

impl HttpTransport for Transport {
    async fn execute(&self, request: Request) -> Result<Response, TransportError> {
        let mut script = self.script.lock().unwrap();
        script.requests.push(request);
        let body = script
            .response
            .take()
            .expect("a completed Memories POST must not be replayed");
        Ok(Response {
            status: StatusCode::OK,
            headers: [(http::header::RETRY_AFTER, "7".parse().unwrap())]
                .into_iter()
                .collect(),
            body: body.into(),
        })
    }

    async fn stream(&self, _: Request) -> Result<StreamResponse, TransportError> {
        self.stream_calls.fetch_add(1, Ordering::SeqCst);
        panic!("Memories must use the unary transport");
    }
}

#[derive(Default)]
struct Telemetry(Mutex<Vec<(u64, Option<StatusCode>, bool)>>);

impl RequestTelemetry for Telemetry {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<StatusCode>,
        error: Option<&TransportError>,
        _: Duration,
    ) {
        self.0
            .lock()
            .unwrap()
            .push((attempt, status, error.is_some()));
    }
}

async fn cases() {
    for (name, response, expected) in [
        (
            "valid",
            br#"{"output":[{"trace_summary":"private-trace-summary","memory_summary":"private-memory-summary"}]}"#.as_slice(),
            Some(vec![MemorySummarizeOutput {
                raw_memory: "private-trace-summary".into(),
                memory_summary: "private-memory-summary".into(),
            }]),
        ),
        (
            "malformed",
            br#"{"output":[{"trace_summary":"private-trace-summary","memory_summary":42}]}"#.as_slice(),
            None,
        ),
    ] {
        let script = Arc::new(Mutex::new(Script {
            response: Some(response.to_vec()),
            requests: Vec::new(),
        }));
        let stream_calls = Arc::new(AtomicUsize::new(0));
        let auth = Arc::new(Auth::default());
        let telemetry = Arc::new(Telemetry::default());
        let client = codex_api::MemoriesClient::new(
            Transport {
                script: script.clone(),
                stream_calls: stream_calls.clone(),
            },
            codex_api::Provider {
                name: "memories-contract".into(),
                base_url: "https://memory-fixture.invalid/v1".into(),
                query_params: None,
                headers: [("x-provider".parse().unwrap(), "provider-value".parse().unwrap())]
                    .into_iter()
                    .collect(),
                retry: codex_api::RetryConfig {
                    max_attempts: 2,
                    base_delay: Duration::from_millis(1),
                    retry_429: true,
                    retry_5xx: true,
                    retry_transport: true,
                },
                stream_idle_timeout: WAIT,
            },
            auth.clone(),
        )
        .with_telemetry(Some(telemetry.clone()));
        let input = MemorySummarizeInput {
            model: "private-memory-model".into(),
            raw_memories: vec![RawMemory {
                id: "private-trace-id".into(),
                metadata: RawMemoryMetadata {
                    source_path: "/private/memory-fixture.json".into(),
                },
                items: vec![json!({
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "private-memory-input"}]
                })],
            }],
            reasoning: Some(Reasoning {
                effort: Some(ReasoningEffort::High),
                summary: Some(ReasoningSummary::Detailed),
                context: Some(ReasoningContext::CurrentTurn),
            }),
        };
        let extra_headers = [("x-extra".parse().unwrap(), "extra-value".parse().unwrap())]
            .into_iter()
            .collect();
        let result = tokio::time::timeout(WAIT, client.summarize_input(&input, extra_headers))
            .await
            .expect("Memories unary operation deadline");

        assert_eq!(auth.0.load(Ordering::SeqCst), 1, "{name}");
        assert_eq!(stream_calls.load(Ordering::SeqCst), 0, "{name}");
        assert_eq!(
            *telemetry.0.lock().unwrap(),
            [(0, Some(StatusCode::OK), false)],
            "{name}: decoding cannot rewrite the completed HTTP callback"
        );
        let script = script.lock().unwrap();
        assert!(script.response.is_none(), "{name}: response was consumed");
        assert_eq!(script.requests.len(), 1, "{name}: no unary replay");
        let request = &script.requests[0];
        assert_eq!(request.method, http::Method::POST);
        assert_eq!(
            request.url,
            "https://memory-fixture.invalid/v1/memories/trace_summarize"
        );
        for (header, expected) in [
            ("x-provider", "provider-value"),
            ("x-extra", "extra-value"),
            ("authorization", "Bearer private-memory-auth"),
        ] {
            let values = request.headers.get_all(header).iter().collect::<Vec<_>>();
            assert_eq!(values.len(), 1, "{name}: exactly one {header} header");
            assert_eq!(values[0].as_bytes(), expected.as_bytes());
        }
        // Inspect public prepared bytes without imposing Responses' encoded-body
        // representation on this unary endpoint.
        let prepared = request.prepare_body_for_send().unwrap();
        let body: serde_json::Value = serde_json::from_slice(&prepared.body_bytes()).unwrap();
        assert_eq!(body, json!({
            "model": "private-memory-model",
            "traces": [{
                "id": "private-trace-id",
                "metadata": {"source_path": "/private/memory-fixture.json"},
                "items": [{
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "private-memory-input"}]
                }]
            }],
            "reasoning": {"effort": "high", "summary": "detailed", "context": "current_turn"}
        }));
        match (expected, result) {
            (Some(expected), Ok(actual)) => assert_eq!(actual, expected),
            (None, Err(ApiError::Stream(message))) => {
                assert!(!message.trim().is_empty(), "original decode error is retained");
            }
            _ => panic!("{name}: preserve the original Memories decode result"),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn memories_decode_failure_does_not_replay_a_completed_http_operation() {
    const NAME: &str = "memories_decode_failure_does_not_replay_a_completed_http_operation";
    const CHILD: &str = "ALAB_MEMORIES_DECODE_CHILD";
    let exact = format!("{}::{NAME}", module_path!().split_once("::").unwrap().1);
    let marker = format!("ALAB_MEMORIES_DECODE_OK:{NAME}\n");
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(NAME));
        assert_eq!(
            std::env::args().skip(1).collect::<Vec<_>>(),
            ["--exact", &exact, "--nocapture", "--test-threads=1"]
        );
        codex_alab_core::activate();
        cases().await;
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
        .env(CHILD, NAME);
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
    .unwrap_or_else(|failure| panic!("isolated Memories decode child: {failure}"))
    .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
}
