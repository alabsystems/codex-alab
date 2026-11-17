// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real HTTP retry dispatch, prepared wire bytes and endpoint diagnostics.

use futures::FutureExt;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Read;
use std::io::Write;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tracing::Instrument;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(60);
const MIN_REMAINING: Duration = Duration::from_millis(500);
const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;
const TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const RESPONSE_ID: &str = "wire-retry-complete";

#[derive(Clone, Copy)]
struct Stamp {
    physical: std::time::Instant,
    logical: tokio::time::Instant,
}

impl Stamp {
    fn now() -> Self {
        Self {
            physical: std::time::Instant::now(),
            logical: tokio::time::Instant::now(),
        }
    }
}

struct Advice {
    deadline: tokio::time::Instant,
    physical_before_read: std::time::Instant,
    remaining: Duration,
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    attempt: u64,
    status: Option<u16>,
    error: bool,
}

#[derive(Default)]
struct Attempts {
    starts: Vec<Stamp>,
    observations: Vec<Observation>,
    advice: Option<Advice>,
}

struct FixtureAuth(Arc<Mutex<Attempts>>);

impl codex_api::AuthProvider for FixtureAuth {
    fn add_auth_headers(&self, headers: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        headers.insert(
            "authorization",
            "Bearer never-log-wire-auth".parse().unwrap(),
        );
        Ok(())
    }

    fn apply_auth(
        &self,
        mut request: codex_http_client::Request,
    ) -> codex_api::AuthProviderFuture<'_> {
        Box::pin(async move {
            self.add_auth_headers(&mut request.headers)?;
            // No await or fixture gate: this is initiation before the actual
            // Reqwest transport, rather than request-completion telemetry.
            self.0.lock().unwrap().starts.push(Stamp::now());
            Ok(request)
        })
    }
}

struct Telemetry(Arc<Mutex<Attempts>>);

impl codex_api::RequestTelemetry for Telemetry {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<http::StatusCode>,
        error: Option<&codex_http_client::TransportError>,
        _: Duration,
    ) {
        let advice = match error {
            Some(codex_http_client::TransportError::Http {
                status: http::StatusCode::SERVICE_UNAVAILABLE,
                retry_after: Some(advice),
                ..
            }) => {
                // Sampling physical time first makes its required interval
                // conservative. No virtual clock is advanced in this test.
                let physical_before_read = std::time::Instant::now();
                let deadline = advice.deadline();
                Some(Advice {
                    deadline,
                    physical_before_read,
                    remaining: deadline.saturating_duration_since(tokio::time::Instant::now()),
                })
            }
            _ => None,
        };
        let mut state = self.0.lock().unwrap();
        state.observations.push(Observation {
            attempt,
            status: status.map(|status| status.as_u16()),
            error: error.is_some(),
        });
        if let Some(advice) = advice {
            assert!(
                state.advice.replace(advice).is_none(),
                "one advised response"
            );
        }
    }
}

#[derive(Default)]
struct Fields {
    values: BTreeMap<String, String>,
    strict: bool,
}

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let allowed = matches!(
            field.name(),
            "message"
                | "local_attempt"
                | "transport"
                | "compression"
                | "connection_reused"
                | "warmup"
                | "model"
                | "mode"
                | "request_bytes"
                | "input_kind"
                | "input_items"
                | "messages"
                | "reasoning_items"
                | "tool_calls"
                | "tool_outputs"
                | "other_items"
                | "tools"
                | "tool_bytes"
                | "elapsed_ms"
                | "failure_kind"
                | "http_status"
                | "opaque_bad_request"
                | "retry_advised"
                | "request_id"
                | "cf_ray"
                | "thread_id"
                | "turn_id"
                | "http.method"
                | "api.path"
        );
        if !allowed {
            assert!(
                !self.strict,
                "unexpected diagnostic field name: {}",
                field.name()
            );
            return;
        }
        self.values
            .insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

impl<S> Layer<S> for Capture
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        context: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        context.span(id).unwrap().extensions_mut().insert(fields);
    }

    fn on_event(&self, event: &tracing::Event<'_>, context: Context<'_, S>) {
        if event.metadata().target() != TARGET || *event.metadata().level() != tracing::Level::INFO
        {
            return;
        }
        let mut fields = Fields {
            strict: true,
            ..Fields::default()
        };
        event.record(&mut fields);
        if let Some(scope) = context.event_scope(event) {
            for span in scope.from_root() {
                if let Some(parent) = span.extensions().get::<Fields>() {
                    for (name, value) in &parent.values {
                        fields
                            .values
                            .entry(name.clone())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
        }
        self.0.lock().unwrap().push(fields.values);
    }
}

struct WirePost {
    headers: BTreeMap<String, Vec<String>>,
    body: Vec<u8>,
    read_completed: Stamp,
}

async fn read_post(socket: &mut TcpStream) -> Result<WirePost, &'static str> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = tokio::time::timeout(WAIT, async {
        loop {
            let read = socket
                .read(&mut chunk)
                .await
                .map_err(|_| "request header read failed")?;
            if read == 0 {
                return Err("request ended before headers");
            }
            bytes.extend_from_slice(&chunk[..read]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                if end + 4 > MAX_HEADERS {
                    return Err("request headers exceed bound");
                }
                return Ok(end + 4);
            }
            if bytes.len() > MAX_HEADERS {
                return Err("request headers exceed bound");
            }
        }
    })
    .await
    .map_err(|_| "request header deadline")??;
    let text =
        std::str::from_utf8(&bytes[..header_end - 4]).map_err(|_| "non-UTF8 request headers")?;
    let mut lines = text.split("\r\n");
    if lines.next() != Some("POST /v1/responses HTTP/1.1") {
        return Err("unexpected request line");
    }
    let mut headers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("malformed request header")?;
        let name = http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| "invalid request header name")?;
        headers
            .entry(name.as_str().to_owned())
            .or_default()
            .push(value.trim_matches([' ', '\t']).to_owned());
    }
    if headers.contains_key("transfer-encoding") {
        return Err("unexpected transfer encoding");
    }
    let lengths = headers
        .get("content-length")
        .ok_or("missing content length")?;
    if lengths.len() != 1
        || lengths[0].is_empty()
        || !lengths[0].bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("ambiguous content length");
    }
    let length: usize = lengths[0].parse().map_err(|_| "invalid content length")?;
    if length == 0 || length > MAX_BODY {
        return Err("request body exceeds bound");
    }
    let end = header_end + length;
    if bytes.len() > end {
        return Err("request exceeds declared length");
    }
    tokio::time::timeout(WAIT, async {
        while bytes.len() < end {
            let remaining = (end - bytes.len()).min(chunk.len());
            let read = socket
                .read(&mut chunk[..remaining])
                .await
                .map_err(|_| "request body read failed")?;
            if read == 0 {
                return Err("request ended before body");
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        Ok(())
    })
    .await
    .map_err(|_| "request body deadline")??;
    Ok(WirePost {
        headers,
        body: bytes[header_end..].to_vec(),
        read_completed: Stamp::now(),
    })
}

struct WireReport {
    posts: Vec<WirePost>,
    first_response_started: Stamp,
}

struct WireServer {
    uri: String,
    task: Option<JoinHandle<Result<WireReport, &'static str>>>,
}

impl WireServer {
    async fn start() -> Self {
        let listener = tokio::time::timeout(WAIT, TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("wire listener deadline")
            .expect("wire listener");
        let uri = format!("http://{}/v1", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut posts = Vec::new();
            let mut first_response_started = None;
            for index in 0..2 {
                let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
                    .await
                    .map_err(|_| "wire accept deadline")?
                    .map_err(|_| "wire accept failed")?;
                posts.push(read_post(&mut socket).await?);
                let (status, fields, body) = if index == 0 {
                    first_response_started = Some(Stamp::now());
                    (
                        "503 Service Unavailable",
                        "retry-after: 1\r\nx-request-id: req_wire-transient\r\ncf-ray: ray_wire-transient\r\ncontent-type: application/json\r\nauthorization: Bearer never-log-response-auth\r\nx-unrelated: never-log-response-header\r\n",
                        "{\"error\":{\"message\":\"never-log-transient-body\"}}".to_owned(),
                    )
                } else {
                    (
                        "200 OK",
                        "content-type: text/event-stream\r\nx-request-id: req_wire-success\r\n",
                        format!(
                            "data: {}\n\ndata: {}\n\n",
                            json!({"type":"response.created","response":{"id":RESPONSE_ID}}),
                            json!({"type":"response.completed","response":{"id":RESPONSE_ID}})
                        ),
                    )
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\n{fields}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                tokio::time::timeout(WAIT, socket.write_all(response.as_bytes()))
                    .await
                    .map_err(|_| "wire response deadline")?
                    .map_err(|_| "wire response failed")?;
                tokio::time::timeout(WAIT, socket.shutdown())
                    .await
                    .map_err(|_| "wire shutdown deadline")?
                    .map_err(|_| "wire shutdown failed")?;
            }
            Ok(WireReport {
                posts,
                first_response_started: first_response_started.expect("first response"),
            })
        });
        Self {
            uri,
            task: Some(task),
        }
    }

    async fn abort_and_join(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }

    async fn finish(mut self) -> Result<WireReport, &'static str> {
        let result = tokio::time::timeout(WAIT, self.task.as_mut().expect("one wire server")).await;
        match result {
            Ok(result) => {
                drop(self.task.take());
                result.map_err(|_| "wire server task failed")?
            }
            Err(_) => {
                self.abort_and_join().await;
                Err("wire server join deadline")
            }
        }
    }
}

impl Drop for WireServer {
    fn drop(&mut self) {
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
    }
}

async fn exchange(compression: codex_api::Compression) -> u64 {
    let transport = codex_api::ReqwestTransport::from_http_client(
        codex_http_client::HttpClientBuilder::new()
            .without_request_logging()
            .build_direct()
            .expect("direct wire client"),
    );
    let mut server = WireServer::start().await;
    let attempts = Arc::new(Mutex::new(Attempts::default()));
    let capture = Capture::default();
    let _subscriber =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
    let client = codex_api::ResponsesClient::new(
        transport,
        codex_api::Provider {
            name: "wire-retry-fixture".into(),
            base_url: server.uri.clone(),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: true,
                retry_transport: false,
            },
            stream_idle_timeout: WAIT,
        },
        Arc::new(FixtureAuth(attempts.clone())),
    )
    .with_telemetry(Some(Arc::new(Telemetry(attempts.clone()))), None);
    let input = json!({"model":"gpt-wire-retry", "input":[
        {"type":"message","role":"user","content":"never-log-wire-input"},
        {"type":"function_call","name":"fixture_tool","call_id":"completed-tool","arguments":"{\"private\":\"never-log-tool-args\"}"},
        {"type":"function_call_output","call_id":"completed-tool","output":"never-log-completed-output"}
    ], "tools":[{"type":"function","name":"fixture_tool","parameters":{"never-log-tool-schema":true}}]});
    let original = serde_json::to_vec(&input).unwrap();
    let outcome = AssertUnwindSafe(async {
        let mut stream = tokio::time::timeout(WAIT, client.stream(input, http::HeaderMap::new(), compression, None)
            .instrument(tracing::info_span!("wire_retry", thread_id = "wire-thread", turn_id = "wire-turn")))
            .await.expect("wire retry endpoint deadline").expect("503 recovers to HTTP200");
        tokio::time::timeout(WAIT, async {
            assert!(matches!(stream.rx_event.recv().await, Some(Ok(codex_api::ResponseEvent::RateLimits(_)))));
            assert!(matches!(stream.rx_event.recv().await, Some(Ok(codex_api::ResponseEvent::Created { response_id: Some(id) })) if id == RESPONSE_ID));
            assert!(matches!(stream.rx_event.recv().await, Some(Ok(codex_api::ResponseEvent::Completed { response_id, .. })) if response_id == RESPONSE_ID));
            assert!(stream.rx_event.recv().await.is_none());
        }).await.expect("wire completion and closed channel deadline");
    }).catch_unwind().await;
    if let Err(payload) = outcome {
        server.abort_and_join().await;
        std::panic::resume_unwind(payload);
    }
    let report = server.finish().await.expect("bounded joined wire server");
    let attempts = attempts.lock().unwrap();
    assert_eq!(
        attempts.observations,
        [
            Observation {
                attempt: 0,
                status: Some(503),
                error: true
            },
            Observation {
                attempt: 1,
                status: Some(200),
                error: false
            }
        ]
    );
    assert_eq!(attempts.starts.len(), 2);
    let advice = attempts
        .advice
        .as_ref()
        .expect("actual first HTTP error retained advice");
    assert!(
        advice.remaining >= MIN_REMAINING,
        "immediate response leaves a meaningful positive advised interval"
    );
    assert!(advice.remaining <= Duration::from_secs(1));
    assert!(advice.deadline >= report.first_response_started.logical + Duration::from_secs(1));
    assert!(
        attempts.starts[1].logical >= advice.deadline,
        "actual retry initiation respects captured advice"
    );
    assert!(
        attempts.starts[1]
            .physical
            .duration_since(advice.physical_before_read)
            >= advice.remaining,
        "physical monotonic elapsed corroborates unpaused provider pacing"
    );
    assert_eq!(report.posts.len(), 2);
    assert!(report.posts[1].read_completed.logical >= attempts.starts[1].logical);
    assert_eq!(report.posts[0].headers, report.posts[1].headers);
    assert_eq!(report.posts[0].body, report.posts[1].body);
    for post in &report.posts {
        assert!(!post.headers.contains_key("transfer-encoding"));
        assert_eq!(
            post.headers.get("content-length"),
            Some(&vec![post.body.len().to_string()])
        );
        assert_eq!(
            post.headers.get("content-type"),
            Some(&vec!["application/json".to_owned()])
        );
        assert_eq!(
            post.headers.get("authorization"),
            Some(&vec!["Bearer never-log-wire-auth".to_owned()])
        );
        match compression {
            codex_api::Compression::None => {
                assert!(!post.headers.contains_key("content-encoding"));
                assert_eq!(post.body, original);
            }
            codex_api::Compression::Zstd => {
                assert_eq!(
                    post.headers.get("content-encoding"),
                    Some(&vec!["zstd".to_owned()])
                );
                let decoder = zstd::stream::read::Decoder::new(post.body.as_slice())
                    .expect("actual wire zstd frame");
                let mut decoded = Vec::new();
                decoder
                    .take((MAX_BODY + 1) as u64)
                    .read_to_end(&mut decoded)
                    .expect("bounded independent zstd decode");
                assert!(decoded.len() <= MAX_BODY);
                assert_eq!(decoded, original);
            }
        }
    }
    let records = capture.0.lock().unwrap();
    assert_eq!(
        records.len(),
        1,
        "one endpoint shape and no selected failure for recovered success"
    );
    let shape = &records[0];
    assert_eq!(shape["message"], "Responses request shape");
    assert_eq!(shape["transport"], "\"responses_http\"");
    assert_eq!(
        shape["compression"],
        match compression {
            codex_api::Compression::None => "\"none\"",
            codex_api::Compression::Zstd => "\"zstd\"",
        }
    );
    assert_eq!(shape["request_bytes"], original.len().to_string());
    assert_eq!(shape["input_items"], "3");
    assert_eq!(shape["messages"], "1");
    assert_eq!(shape["tool_calls"], "1");
    assert_eq!(shape["tool_outputs"], "1");
    assert_eq!(shape["thread_id"], "\"wire-thread\"");
    assert_eq!(shape["turn_id"], "\"wire-turn\"");
    for absent in [
        "failure_kind",
        "http_status",
        "request_id",
        "cf_ray",
        "retry_advised",
    ] {
        assert!(!shape.contains_key(absent));
    }
    let logged = format!("{records:?}");
    for private in [
        "never-log",
        "completed-tool",
        "fixture_tool",
        RESPONSE_ID,
        "req_wire-transient",
        "ray_wire-transient",
        "req_wire-success",
        "authorization",
        "x-unrelated",
    ] {
        assert!(
            !logged.contains(private),
            "private/unselected content stays outside endpoint diagnostics"
        );
    }
    let local_attempt = shape["local_attempt"].parse::<u64>().unwrap();
    assert!(local_attempt > 0);
    drop(records);
    drop(attempts);
    drop(client);
    local_attempt
}

#[tokio::test(flavor = "current_thread")]
async fn real_http_retries_preserve_wire_body_and_provider_pacing() {
    const NAME: &str = "real_http_retries_preserve_wire_body_and_provider_pacing";
    const CHILD: &str = "ALAB_HTTP_WIRE_RETRY_CHILD";
    let exact = format!("{}::{NAME}", module_path!().split_once("::").unwrap().1);
    let marker = format!("ALAB_HTTP_WIRE_RETRY_OK:{NAME}\n");
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(NAME));
        assert_eq!(
            std::env::args().skip(1).collect::<Vec<_>>(),
            ["--exact", &exact, "--nocapture", "--test-threads=1"]
        );
        codex_alab_core::activate();
        let plain = exchange(codex_api::Compression::None).await;
        let compressed = exchange(codex_api::Compression::Zstd).await;
        assert_ne!(
            plain, compressed,
            "logical endpoint calls have distinct attempt IDs"
        );
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
    crate::test_child_process::run_with_stdout_marker(command, CHILD_WAIT, Some(&marker))
        .await
        .unwrap_or_else(|failure| panic!("isolated real HTTP retry child: {failure}"))
        .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
}
