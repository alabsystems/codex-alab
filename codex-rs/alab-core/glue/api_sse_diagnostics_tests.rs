// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise semantic delivery through the actual HTTP endpoint and spawned SSE consumer.

use super::*;
use crate::auth::AuthProvider;
use crate::common::ResponseEvent;
use crate::endpoint::ResponsesClient;
use crate::provider::Provider;
use crate::provider::RetryConfig;
use bytes::Bytes;
use codex_client::ByteStream;
use codex_client::HttpTransport;
use codex_client::Request;
use codex_client::Response;
use codex_client::StreamResponse;
use futures::StreamExt;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use tokio::sync::oneshot;
use tracing::Instrument;

type PollResult = Result<
    Option<Result<eventsource_stream::Event, eventsource_stream::EventStreamError<TransportError>>>,
    tokio::time::error::Elapsed,
>;

#[derive(Default)]
struct InnerTelemetry {
    polls: Mutex<Vec<(usize, Duration)>>,
    errors: Mutex<Vec<usize>>,
}

impl SseTelemetry for InnerTelemetry {
    fn on_sse_poll(&self, result: &PollResult, duration: Duration) {
        self.polls
            .lock()
            .unwrap()
            .push((std::ptr::from_ref(result) as usize, duration));
    }

    fn on_sse_error(&self, error: &ApiError) {
        self.errors
            .lock()
            .unwrap()
            .push(std::ptr::from_ref(error) as usize);
    }
}

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

struct SpanRecord {
    parent: Option<u64>,
    fields: BTreeMap<String, String>,
}

#[derive(Default)]
struct CaptureState {
    records: Vec<BTreeMap<String, String>>,
    spans: BTreeMap<u64, SpanRecord>,
    entered: Vec<u64>,
}

#[derive(Clone, Default)]
struct Capture {
    state: Arc<Mutex<CaptureState>>,
    next_span: Arc<AtomicUsize>,
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let id = self.next_span.fetch_add(1, Ordering::Relaxed) as u64 + 1;
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let mut state = self.state.lock().unwrap();
        let parent = attributes
            .parent()
            .map(tracing::span::Id::into_u64)
            .or_else(|| {
                attributes
                    .is_contextual()
                    .then(|| state.entered.last().copied())
                    .flatten()
            });
        state.spans.insert(
            id,
            SpanRecord {
                parent,
                fields: fields.0,
            },
        );
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        self.state
            .lock()
            .unwrap()
            .spans
            .get_mut(&id.into_u64())
            .unwrap()
            .fields
            .extend(fields.0);
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn enter(&self, id: &tracing::span::Id) {
        self.state.lock().unwrap().entered.push(id.into_u64());
    }

    fn exit(&self, id: &tracing::span::Id) {
        let mut state = self.state.lock().unwrap();
        assert_eq!(state.entered.pop(), Some(id.into_u64()));
    }

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        // Avoid logging the fixture's DEBUG/TRACE payload events: production's
        // local diagnostics deliberately contain only the two named INFO events.
        if !fields
            .0
            .get("message")
            .is_some_and(|message| message.starts_with("Responses request"))
        {
            return;
        }
        let mut state = self.state.lock().unwrap();
        let mut cursor = event
            .parent()
            .map(tracing::span::Id::into_u64)
            .or_else(|| state.entered.last().copied());
        while let Some(id) = cursor {
            let span = &state.spans[&id];
            for (name, value) in &span.fields {
                fields
                    .0
                    .entry(name.clone())
                    .or_insert_with(|| value.clone());
            }
            cursor = span.parent;
        }
        state.records.push(fields.0);
    }
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Eof,
    Idle,
    Transport,
    Immediate,
    Deferred,
    Superseded,
}

impl Case {
    fn expected_polls(self) -> usize {
        match self {
            Self::Eof | Self::Idle | Self::Transport | Self::Immediate => 2,
            Self::Deferred | Self::Superseded => 3,
        }
    }

    fn body(self) -> Vec<Result<Bytes, TransportError>> {
        match self {
            Self::Eof | Self::Idle => vec![],
            Self::Transport => vec![Err(TransportError::Network(
                "never-log-transport-content".into(),
            ))],
            Self::Immediate => vec![Ok(sse(json!({
                "type":"response.failed",
                "response":{"error":{"code":"invalid_access_program","message":"never-log-provider-content"}}
            })))],
            Self::Deferred => vec![Ok(deferred())],
            Self::Superseded => vec![
                Ok(deferred()),
                Ok(sse(
                    json!({"type":"response.completed","response":{"id":"resp_complete"}}),
                )),
            ],
        }
    }
}

fn sse(event: Value) -> Bytes {
    Bytes::from(format!("data: {event}\n\n"))
}

fn deferred() -> Bytes {
    sse(json!({
        "type":"response.failed",
        "response":{"error":{"code":"unknown_failure","message":"never-log-deferred-content"}}
    }))
}

struct StreamingTransport {
    case: Case,
    gate: Mutex<Option<oneshot::Receiver<()>>>,
}

impl HttpTransport for StreamingTransport {
    async fn execute(&self, _: Request) -> Result<Response, TransportError> {
        panic!("SSE fixture must use streaming HTTP");
    }

    async fn stream(&self, _: Request) -> Result<StreamResponse, TransportError> {
        let gate = self.gate.lock().unwrap().take().expect("one HTTP request");
        let first = futures::stream::once(async move {
            gate.await.expect("release SSE fixture");
            Ok(sse(
                json!({"type":"response.created","response":{"id":"resp_created"}}),
            ))
        });
        let mut bytes: ByteStream = first.chain(futures::stream::iter(self.case.body())).boxed();
        if matches!(self.case, Case::Idle | Case::Immediate) {
            // Immediate errors must be delivered while the underlying stream is
            // still open; the idle case must wait for the actual timeout.
            bytes = bytes.chain(futures::stream::pending()).boxed();
        }
        let mut headers = http::HeaderMap::new();
        if matches!(self.case, Case::Idle) {
            headers.insert("x-request-id", "invalid identifier".parse().unwrap());
            headers.insert("x-oai-request-id", "req_fallback-2".parse().unwrap());
            headers.insert("cf-ray", "invalid correlation data".parse().unwrap());
        } else {
            headers.insert("x-request-id", "req_sse-1".parse().unwrap());
            headers.insert("cf-ray", "ray_sse-1".parse().unwrap());
        }
        headers.insert("authorization", "Bearer never-log-header".parse().unwrap());
        Ok(StreamResponse {
            status: http::StatusCode::OK,
            headers,
            bytes,
        })
    }
}

struct NoAuth;
impl AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), crate::AuthError> {
        Ok(())
    }
}

async fn endpoint_case(case: Case, active: bool) {
    let (release, gate) = oneshot::channel();
    let inner = Arc::new(InnerTelemetry::default());
    let client = ResponsesClient::new(
        StreamingTransport {
            case,
            gate: Mutex::new(Some(gate)),
        },
        Provider {
            name: "fixture".into(),
            base_url: "https://example.com/v1".into(),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_millis(2),
        },
        Arc::new(NoAuth),
    )
    .with_telemetry(None, Some(inner.clone()));
    let capture = Capture::default();
    let mut stream = {
        let _subscriber = tracing::subscriber::set_default(capture.clone());
        let span = tracing::info_span!(
            "fixture_request",
            thread_id = "sse-thread",
            turn_id = "sse-turn"
        );
        client.stream(
            json!({"model":"gpt-example","input":[{"type":"message","content":"never-log-input"}]}),
            http::HeaderMap::new(),
            Compression::None,
            None,
        ).instrument(span).await.expect("HTTP response opens successfully")
    };
    // The spawned SSE task now runs without the request's thread-local subscriber.
    // Its diagnostic must restore the captured Dispatch explicitly. This minimal
    // subscriber does not implement current_span; it tests dispatch retention
    // without adding a span-registry dependency to the API crate.
    release.send(()).unwrap();
    let mut errors = 0;
    let mut completed = 0;
    while let Some(event) = stream.next().await {
        match event {
            Err(_) => errors += 1,
            Ok(ResponseEvent::Completed { .. }) => completed += 1,
            _ => {}
        }
    }
    let expected_errors = usize::from(!matches!(case, Case::Superseded));
    assert_eq!(errors, expected_errors, "{case:?}");
    assert_eq!(
        completed,
        usize::from(matches!(case, Case::Superseded)),
        "{case:?}"
    );
    assert_eq!(
        inner.errors.lock().unwrap().len(),
        expected_errors,
        "{case:?}"
    );
    assert_eq!(
        inner.polls.lock().unwrap().len(),
        case.expected_polls(),
        "{case:?}"
    );
    let state = capture.state.lock().unwrap();
    if !active {
        assert!(state.records.is_empty(), "{case:?}: {:?}", state.records);
        return;
    }
    assert_eq!(
        state.records.len(),
        1 + expected_errors,
        "{case:?}: {:?}",
        state.records
    );
    let shape = &state.records[0];
    assert_eq!(shape["message"], "Responses request shape");
    if expected_errors > 0 {
        let failure = &state.records[1];
        assert_eq!(failure["message"], "Responses request failed");
        assert_eq!(shape["local_attempt"], failure["local_attempt"]);
        assert_eq!(shape["thread_id"], "\"sse-thread\"");
        assert_eq!(shape["turn_id"], "\"sse-turn\"");
        assert_eq!(
            failure["request_id"],
            if matches!(case, Case::Idle) {
                "\"req_fallback-2\""
            } else {
                "\"req_sse-1\""
            }
        );
        assert_eq!(
            failure.get("cf_ray").map(String::as_str),
            if matches!(case, Case::Idle) {
                None
            } else {
                Some("\"ray_sse-1\"")
            }
        );
        // A successful opening HTTP 200 is not the status of a later stream error.
        assert!(!failure.contains_key("http_status"), "{failure:?}");
        assert_eq!(
            failure["failure_kind"],
            match case {
                Case::Immediate => "\"invalid_request\"",
                Case::Deferred => "\"retryable\"",
                _ => "\"stream\"",
            }
        );
    }
    let text = format!("{:?}", state.records);
    for forbidden in [
        "never-log",
        "invalid identifier",
        "invalid correlation",
        "authorization",
        "unknown_failure",
        "response.failed",
    ] {
        assert!(!text.contains(forbidden), "{case:?}: {text}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn http_sse_semantic_failures_retain_attempt_ids_and_captured_context() {
    const CHILD: &str = "ALAB_HTTP_SSE_DIAGNOSTIC_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let name = format!(
            "{}::http_sse_semantic_failures_retain_attempt_ids_and_captured_context",
            module_path!().split_once("::").unwrap().1
        );
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1");
        crate::test_child_process::run(command, std::time::Duration::from_secs(30))
            .await
            .unwrap_or_else(|failure| panic!("{name}: {failure}"))
            .assert_test_success(&name);
        return;
    }
    assert!(!codex_alab_core::active());
    for active in [false, true] {
        if active {
            codex_alab_core::activate();
        }
        for case in [
            Case::Eof,
            Case::Idle,
            Case::Transport,
            Case::Immediate,
            Case::Deferred,
            Case::Superseded,
        ] {
            endpoint_case(case, active).await;
        }
    }
}

#[test]
fn sse_wrapper_forwards_original_references_and_inactive_arc_without_replacement() {
    let inner = Arc::new(InnerTelemetry::default());
    let observer: Arc<dyn SseTelemetry> = inner.clone();
    let headers = http::HeaderMap::new();
    let unchanged = sse_telemetry(None, Some(observer.clone()), &headers).unwrap();
    assert!(Arc::ptr_eq(&observer, &unchanged));
    assert!(sse_telemetry(None, None, &headers).is_none());
    let diagnostic = RequestDiagnostic::record(
        Transport::Http(Compression::None),
        Some("gpt-example"),
        false,
        InputShape::typed(&[]),
        Some(0),
        Some(0),
        10,
    );
    let wrapped = sse_telemetry(Some(&diagnostic), Some(observer), &headers).unwrap();
    let poll: PollResult = Ok(None);
    let duration = Duration::from_nanos(37);
    wrapped.on_sse_poll(&poll, duration);
    let error = ApiError::Stream("never-log-this".into());
    wrapped.on_sse_error(&error);
    assert_eq!(
        *inner.polls.lock().unwrap(),
        [(std::ptr::from_ref(&poll) as usize, duration)]
    );
    assert_eq!(
        *inner.errors.lock().unwrap(),
        [std::ptr::from_ref(&error) as usize]
    );
}
