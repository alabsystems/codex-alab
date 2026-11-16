// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An exact, validated invalid_request refusal must not become a retryable stream error.

use codex_api::ApiError;
use codex_http_client::ByteStream;
use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::RequestBody;
use codex_http_client::Response;
use codex_http_client::StreamResponse;
use codex_http_client::TransportError;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const WAIT: Duration = Duration::from_secs(5);
const CHILD: &str = "ALAB_SSE_INVALID_REQUEST_CHILD";
const COMPLETE: &str = "ALAB_SSE_INVALID_REQUEST_OK";
const TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const PRIVATE: &str = "never-log-private-invalid-request";
const FALLBACK: &str = "Invalid request.";

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
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
        if event.metadata().target() == TARGET {
            assert_eq!(*event.metadata().level(), tracing::Level::INFO);
            let mut fields = Fields::default();
            event.record(&mut fields);
            if let Some(scope) = context.event_scope(event) {
                for span in scope.from_root() {
                    if let Some(parent) = span.extensions().get::<Fields>() {
                        for (name, value) in &parent.0 {
                            fields
                                .0
                                .entry(name.clone())
                                .or_insert_with(|| value.clone());
                        }
                    }
                }
            }
            self.0.lock().unwrap().push(fields.0);
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Envelope {
    Failed,
    Error,
}

#[derive(Clone, Copy, Debug)]
enum Tail {
    Pending,
    Completed,
    Eof,
}

#[derive(Clone, Copy, Debug)]
enum Expected {
    InvalidRequest(&'static str),
    InvalidPrompt(&'static str),
    Completed,
    Retryable(&'static str, bool),
    Stream(&'static str),
    Quota,
    Overloaded,
    ProviderMarker(&'static str),
}

#[derive(Default)]
struct Observed {
    sent: Mutex<Vec<Vec<u8>>>,
    tail_polls: AtomicUsize,
    dropped: AtomicBool,
}

struct TailProbe(Arc<Observed>);

impl Drop for TailProbe {
    fn drop(&mut self) {
        self.0.dropped.store(true, Ordering::SeqCst);
    }
}

struct GatedTransport {
    envelope: Envelope,
    detail: Value,
    tail: Tail,
    gate: Mutex<Option<oneshot::Receiver<()>>>,
    observed: Arc<Observed>,
}

fn sse(value: Value) -> Vec<u8> {
    format!("data: {value}\n\n").into_bytes()
}

impl HttpTransport for GatedTransport {
    async fn execute(&self, _: Request) -> Result<Response, TransportError> {
        panic!("Responses uses its streaming transport");
    }

    async fn stream(&self, request: Request) -> Result<StreamResponse, TransportError> {
        assert_eq!(request.method, http::Method::POST);
        assert!(request.url.ends_with("/v1/responses"));
        let Some(RequestBody::EncodedJson(body)) = request.body else {
            panic!("the endpoint sends encoded input");
        };
        self.observed
            .sent
            .lock()
            .unwrap()
            .push(body.as_bytes().to_vec());
        let gate = self.gate.lock().unwrap().take().expect("one actual send");
        let envelope = self.envelope;
        let detail = self.detail.clone();
        let head = futures::stream::once(async move {
            gate.await.expect("release after caller context exits");
            let frame = match envelope {
                Envelope::Failed => json!({"type":"response.failed", "response":{"error":detail}}),
                Envelope::Error => json!({"type":"error", "error":detail,
                    "headers":{"retry-after":"11", "authorization":PRIVATE}}),
            };
            Ok(sse(frame).into())
        });
        let probe = TailProbe(self.observed.clone());
        let tail = self.tail;
        let mut emitted = false;
        let tail: ByteStream = futures::stream::poll_fn(move |_| {
            probe.0.tail_polls.fetch_add(1, Ordering::SeqCst);
            match tail {
                Tail::Pending => Poll::Pending,
                Tail::Eof => Poll::Ready(None),
                Tail::Completed if emitted => Poll::Ready(None),
                Tail::Completed => {
                    emitted = true;
                    Poll::Ready(Some(Ok(sse(json!({
                        "type":"response.completed", "response":{"id":"contradictory-completion"}
                    }))
                    .into())))
                }
            }
        })
        .boxed();
        Ok(StreamResponse {
            status: http::StatusCode::OK,
            headers: [
                ("x-request-id", "invalid identifier"),
                ("x-oai-request-id", "req_invalid-1"),
                ("cf-ray", "ray_invalid-1"),
                ("authorization", PRIVATE),
                ("x-unrelated", PRIVATE),
            ]
            .into_iter()
            .map(|(name, value)| (name.parse().unwrap(), value.parse().unwrap()))
            .collect(),
            bytes: head.chain(tail).boxed(),
        })
    }
}

struct NoAuth;

impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

async fn run_case(detail: Value, tail: Tail, expected: Expected) {
    run_envelope_case(Envelope::Failed, detail, tail, expected).await;
}

async fn run_envelope_case(envelope: Envelope, detail: Value, tail: Tail, expected: Expected) {
    let (release, gate) = oneshot::channel();
    let observed = Arc::new(Observed::default());
    let client = codex_api::ResponsesClient::new(
        GatedTransport {
            envelope,
            detail,
            tail,
            gate: Mutex::new(Some(gate)),
            observed: observed.clone(),
        },
        codex_api::Provider {
            name: "invalid-request-fixture".into(),
            base_url: "https://never-log-private-destination.invalid/v1".into(),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 3,
                base_delay: Duration::ZERO,
                retry_429: true,
                retry_5xx: true,
                retry_transport: true,
            },
            stream_idle_timeout: Duration::from_secs(1),
        },
        Arc::new(NoAuth),
    );
    let request = json!({"model":"gpt-invalid-fixture", "input":[
        {"type":"message", "role":"user", "content":PRIVATE}
    ]});
    let encoded = serde_json::to_vec(&request).unwrap();
    let capture = Capture::default();
    let mut stream = {
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        let thread = if matches!(expected, Expected::ProviderMarker(_)) {
            tracing::info_span!("opaque_marker_thread", thread_id = "opaque-marker-thread")
        } else {
            tracing::Span::none()
        };
        let _thread = thread.enter();
        let turn = if matches!(expected, Expected::ProviderMarker(_)) {
            tracing::info_span!("opaque_marker_turn", turn_id = "opaque-marker-turn")
        } else {
            tracing::Span::none()
        };
        let _turn = turn.enter();
        tokio::time::timeout(
            WAIT,
            client.stream(
                request,
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            ),
        )
        .await
        .expect("HTTP opening deadline")
        .expect("HTTP 200 opens before the semantic failure")
    };
    assert_eq!(
        capture.0.lock().unwrap().len(),
        usize::from(codex_alab_core::active())
    );
    assert_eq!(*observed.sent.lock().unwrap(), [encoded]);
    assert_eq!(observed.tail_polls.load(Ordering::SeqCst), 0);
    assert!(!observed.dropped.load(Ordering::SeqCst));
    if matches!(expected, Expected::ProviderMarker(_)) {
        assert!(
            tracing::Span::current().is_disabled(),
            "caller context exited"
        );
    }
    let started = Instant::now();
    release.send(()).unwrap();
    let event = tokio::time::timeout(WAIT, async {
        loop {
            match stream.rx_event.recv().await {
                Some(Ok(codex_api::ResponseEvent::RateLimits(_))) => {}
                outcome => break outcome,
            }
        }
    })
    .await
    .expect("selected semantic outcome deadline");
    let mut provider_display = None;
    match expected {
        Expected::InvalidRequest(message) | Expected::InvalidPrompt(message) => {
            let error = match (expected, event) {
                (
                    Expected::InvalidRequest(_),
                    Some(Err(error @ ApiError::InvalidRequest { .. })),
                )
                | (Expected::InvalidPrompt(_), Some(Err(error @ ApiError::InvalidPrompt { .. }))) => {
                    error
                }
                (_, other) => {
                    panic!("{tail:?}: expected exact {expected:?} refusal, got {other:?}")
                }
            };
            let (ApiError::InvalidRequest { message: actual }
            | ApiError::InvalidPrompt { message: actual }) = &error
            else {
                unreachable!()
            };
            assert_eq!(
                actual, message,
                "preserve the message or its defined fallback"
            );
            assert_eq!(Instant::now(), started, "refusal precedes the idle timeout");
            let mapped = codex_api::map_api_error(error);
            assert_eq!(mapped.to_string(), message);
            assert!(
                mapped.retry_delay(1).is_none(),
                "validation refusal is never retried"
            );
            assert!(
                mapped.retry_after().is_none(),
                "wire retry advice cannot make validation retryable"
            );
        }
        Expected::Completed => assert!(matches!(event,
            Some(Ok(codex_api::ResponseEvent::Completed { response_id, .. }))
                if response_id == "contradictory-completion"
        )),
        Expected::Retryable(message, advised) => {
            let Some(Err(ApiError::Retryable {
                message: actual,
                retry_after,
            })) = event
            else {
                panic!("expected unchanged retryable fallback, got {event:?}");
            };
            assert_eq!(actual, message);
            assert_eq!(retry_after.is_some(), advised);
            if let Some(advice) = retry_after {
                assert_eq!(advice.deadline(), started + Duration::from_secs(7));
            }
        }
        Expected::Stream(message) => {
            assert!(matches!(event, Some(Err(ApiError::Stream(actual))) if actual == message))
        }
        Expected::Quota => {
            assert!(matches!(event, Some(Err(ApiError::QuotaExceeded))));
            let mapped = codex_api::map_api_error(ApiError::QuotaExceeded);
            assert!(mapped.retry_delay(1).is_none());
            assert!(mapped.retry_after().is_none());
        }
        Expected::Overloaded => {
            let Some(Err(error @ ApiError::ServerOverloaded { .. })) = event else {
                panic!("overload keeps its existing classification: {event:?}");
            };
            let mapped = codex_api::map_api_error(error);
            assert_eq!(
                mapped.retry_after().unwrap().deadline(),
                started + Duration::from_secs(7)
            );
            assert!(mapped.retry_delay(1).is_some());
        }
        Expected::ProviderMarker(message) => {
            let Some(Err(error @ ApiError::Retryable { .. })) = event else {
                panic!("unknown provider error retains Retryable identity: {event:?}");
            };
            let ApiError::Retryable {
                message: actual,
                retry_after,
            } = &error
            else {
                unreachable!()
            };
            assert_eq!(actual, message, "preserve provider text before the bridge");
            let advice = retry_after.expect("nested retry advice");
            assert_eq!(advice.deadline(), started + Duration::from_secs(7));
            assert_eq!(
                Instant::now(),
                started,
                "EOF does not wait for the idle timer"
            );
            let mapped = codex_api::map_api_error(error);
            assert!(mapped.retry_delay(1).is_some());
            assert_eq!(mapped.retry_after(), Some(advice));
            assert_eq!(mapped.http_status_code_value(), None);
            assert_eq!(
                serde_json::to_value(mapped.to_codex_protocol_error()).unwrap(),
                json!("other")
            );
            provider_display = Some(mapped.to_string());
        }
    }
    assert!(
        tokio::time::timeout(WAIT, stream.rx_event.recv())
            .await
            .expect("selected task closes channel")
            .is_none()
    );
    assert!(observed.dropped.load(Ordering::SeqCst));
    if matches!(
        expected,
        Expected::InvalidRequest(_)
            | Expected::InvalidPrompt(_)
            | Expected::Quota
            | Expected::Overloaded
    ) {
        assert_eq!(
            observed.tail_polls.load(Ordering::SeqCst),
            0,
            "typed refusal ends reading before the tail"
        );
    } else {
        assert!(observed.tail_polls.load(Ordering::SeqCst) > 0);
    }
    assert_eq!(
        observed.sent.lock().unwrap().len(),
        1,
        "one actual endpoint send"
    );
    let records = capture.0.lock().unwrap();
    let active = codex_alab_core::active();
    assert_eq!(
        records.len(),
        if active {
            1 + usize::from(!matches!(expected, Expected::Completed))
        } else {
            0
        }
    );
    if active {
        assert_eq!(records[0]["message"], "Responses request shape");
        if !matches!(expected, Expected::Completed) {
            let failed = &records[1];
            assert_eq!(failed["message"], "Responses request failed");
            assert_eq!(failed["local_attempt"], records[0]["local_attempt"]);
            assert_eq!(failed["request_id"], "\"req_invalid-1\"");
            assert_eq!(failed["cf_ray"], "\"ray_invalid-1\"");
            assert!(
                !failed.contains_key("http_status"),
                "opening 200 is not an error status"
            );
            if matches!(
                expected,
                Expected::InvalidRequest(_) | Expected::InvalidPrompt(_)
            ) {
                assert_eq!(
                    failed["failure_kind"],
                    if matches!(expected, Expected::InvalidPrompt(_)) {
                        "\"invalid_prompt\""
                    } else {
                        "\"invalid_request\""
                    }
                );
                assert!(!failed.contains_key("retry_advised"));
            }
            if matches!(expected, Expected::ProviderMarker(_)) {
                assert_eq!(failed["failure_kind"], "\"retryable\"");
                assert_eq!(failed["retry_advised"], "true");
                assert_eq!(failed["opaque_bad_request"], "false");
                for record in records.iter() {
                    assert_eq!(record["thread_id"], "\"opaque-marker-thread\"");
                    assert_eq!(record["turn_id"], "\"opaque-marker-turn\"");
                }
            }
        }
    }
    for record in records.iter() {
        for field in ["code", "error_code", "error_message", "body", "payload"] {
            assert!(
                !record.contains_key(field),
                "semantic payload is never diagnostic metadata"
            );
        }
    }
    let text = format!("{records:?}");
    for private in [
        "never-log",
        "invalid identifier",
        "authorization",
        "x-unrelated",
        "{\\\"detail\\\":",
    ] {
        assert!(
            !text.contains(private),
            "diagnostics exclude private provider and request content"
        );
    }
    if let Expected::ProviderMarker(message) = expected {
        assert!(!text.contains(message));
        assert_eq!(
            provider_display.as_deref(),
            Some(
                format!("stream disconnected before completion: provider stream: {message}")
                    .as_str()
            ),
            "provider text cannot impersonate the bridge's normalized HTTP400 marker"
        );
    }
}

async fn isolated(name: &str) -> bool {
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name));
        assert_eq!(
            std::env::args().skip(1).collect::<Vec<_>>(),
            ["--exact", &exact, "--nocapture", "--test-threads=1"]
        );
        tokio::time::pause();
        return false;
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
    let marker = format!("{COMPLETE}:{name}\n");
    crate::test_child_process::run_with_stdout_marker(
        command,
        Duration::from_secs(30),
        Some(&marker),
    )
    .await
    .unwrap_or_else(|failure| panic!("isolated invalid-request child: {failure}"))
    .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
    true
}

fn complete(name: &str) {
    std::io::stdout()
        .lock()
        .write_all(format!("{COMPLETE}:{name}\n").as_bytes())
        .expect("child completion marker");
}

#[tokio::test(flavor = "current_thread")]
async fn exact_invalid_request_owns_pending_and_contradictory_streams() {
    const NAME: &str = "exact_invalid_request_owns_pending_and_contradictory_streams";
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    for tail in [Tail::Pending, Tail::Completed] {
        for message in [
            PRIVATE,
            "  original refusal\nwith whitespace  ",
            r#"{"detail":"Bad Request"}"#,
        ] {
            run_case(
                json!({"code":"invalid_request", "type":"insufficient_quota", "message":message,
                "headers":{"retry-after":"7", "authorization":PRIVATE}}),
                tail,
                Expected::InvalidRequest(message),
            )
            .await;
        }
    }
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_request_uses_fallback_only_for_absent_or_blank_message() {
    const NAME: &str = "invalid_request_uses_fallback_only_for_absent_or_blank_message";
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    for detail in [
        json!({"code":"invalid_request"}),
        json!({"code":"invalid_request", "message":null}),
        json!({"code":"invalid_request", "message":""}),
        json!({"code":"invalid_request", "message":" \t\n "}),
    ] {
        run_case(detail, Tail::Eof, Expected::InvalidRequest(FALLBACK)).await;
    }
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn only_exact_validated_code_changes_existing_failure_classification() {
    const NAME: &str = "only_exact_validated_code_changes_existing_failure_classification";
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    for field in ["type", "code", "message", "plan_type", "resets_at"] {
        let mut detail = json!({"code":"invalid_request", "message":PRIVATE});
        detail[field] = if field == "resets_at" {
            json!("invalid")
        } else {
            json!(42)
        };
        run_case(
            detail.clone(),
            Tail::Eof,
            Expected::Stream("response.failed event received"),
        )
        .await;
        run_case(detail.clone(), Tail::Completed, Expected::Completed).await;
        detail["headers"] = json!({"retry-after":"7"});
        run_case(
            detail,
            Tail::Eof,
            Expected::Retryable("response.failed event received", true),
        )
        .await;
    }
    for detail in [
        json!({"type":"invalid_request_error", "message":PRIVATE}),
        json!({"code":"unknown_error", "type":"invalid_request_error", "message":PRIVATE}),
        json!({"code":"INVALID_REQUEST", "message":PRIVATE}),
        json!({"code":"invalid_request_extra", "message":PRIVATE}),
    ] {
        run_case(
            detail.clone(),
            Tail::Eof,
            Expected::Retryable(PRIVATE, false),
        )
        .await;
        run_case(detail, Tail::Completed, Expected::Completed).await;
    }
    for code in ["insufficient_quota", "organization_usage_limit_exceeded"] {
        run_case(
            json!({"code":code, "type":"invalid_request_error", "message":PRIVATE,
            "headers":{"retry-after":"7"}}),
            Tail::Pending,
            Expected::Quota,
        )
        .await;
    }
    run_case(
        json!({"code":"server_is_overloaded", "type":"invalid_request_error", "message":PRIVATE,
        "headers":{"retry-after":"7"}}),
        Tail::Pending,
        Expected::Overloaded,
    )
    .await;
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn inactive_invalid_request_keeps_upstream_retryable_and_completion_paths() {
    const NAME: &str = "inactive_invalid_request_keeps_upstream_retryable_and_completion_paths";
    if isolated(NAME).await {
        return;
    }
    assert!(!codex_alab_core::active());
    let detail =
        json!({"code":"invalid_request", "message":PRIVATE, "headers":{"retry-after":"7"}});
    run_case(
        detail.clone(),
        Tail::Eof,
        Expected::Retryable(PRIVATE, true),
    )
    .await;
    run_case(detail, Tail::Completed, Expected::Completed).await;
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_invalid_request_owns_pending_completion_and_eof() {
    const NAME: &str = "standalone_invalid_request_owns_pending_completion_and_eof";
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    for tail in [Tail::Pending, Tail::Completed, Tail::Eof] {
        for message in [PRIVATE, r#"{"detail":"Bad Request"}"#] {
            run_envelope_case(
                Envelope::Error,
                json!({"code":"invalid_request", "type":"insufficient_quota", "message":message,
                    "headers":{"retry-after":"7", "authorization":PRIVATE}}),
                tail,
                Expected::InvalidRequest(message),
            )
            .await;
        }
        for detail in [
            json!({"code":"invalid_request"}),
            json!({"code":"invalid_request", "message":null}),
            json!({"code":"invalid_request", "message":""}),
            json!({"code":"invalid_request", "message":" \t\n "}),
        ] {
            run_envelope_case(
                Envelope::Error,
                detail,
                tail,
                Expected::InvalidRequest(FALLBACK),
            )
            .await;
        }
    }
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_errors_require_exact_validated_invalid_request_code() {
    const NAME: &str = "standalone_errors_require_exact_validated_invalid_request_code";
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    let mut controls = Vec::new();
    for field in ["type", "code", "message", "plan_type", "resets_at"] {
        let mut detail = json!({"code":"invalid_request", "message":PRIVATE});
        detail[field] = if field == "resets_at" {
            json!("invalid")
        } else {
            json!(42)
        };
        controls.push(detail);
    }
    controls.extend([
        json!({"type":"invalid_request_error", "message":PRIVATE}),
        json!({"code":"unknown_error", "type":"invalid_request_error", "message":PRIVATE}),
        json!({"code":"INVALID_REQUEST", "message":PRIVATE}),
        json!({"code":"invalid_request_extra", "message":PRIVATE}),
    ]);
    for mut detail in controls {
        // The standalone upstream path ignores these events even with valid
        // nested advice; sharing the failed-response parser must not promote them.
        detail["headers"] = json!({"retry-after":"7"});
        for (tail, expected) in [
            (
                Tail::Pending,
                Expected::Stream("idle timeout waiting for SSE"),
            ),
            (
                Tail::Eof,
                Expected::Stream("stream closed before response.completed"),
            ),
            (Tail::Completed, Expected::Completed),
        ] {
            run_envelope_case(Envelope::Error, detail.clone(), tail, expected).await;
        }
    }
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_inactive_invalid_request_keeps_upstream_ignored_path() {
    const NAME: &str = "standalone_inactive_invalid_request_keeps_upstream_ignored_path";
    if isolated(NAME).await {
        return;
    }
    assert!(!codex_alab_core::active());
    let detail =
        json!({"code":"invalid_request", "message":PRIVATE, "headers":{"retry-after":"7"}});
    for (tail, expected) in [
        (
            Tail::Pending,
            Expected::Stream("idle timeout waiting for SSE"),
        ),
        (
            Tail::Eof,
            Expected::Stream("stream closed before response.completed"),
        ),
        (Tail::Completed, Expected::Completed),
    ] {
        run_envelope_case(Envelope::Error, detail.clone(), tail, expected).await;
    }
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_invalid_prompt_matches_failed_response_without_retry() {
    const NAME: &str = "standalone_invalid_prompt_matches_failed_response_without_retry";
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    // Run the established failed-response route first. Unlike invalid_request,
    // an invalid_prompt preserves present empty and whitespace-only messages.
    for envelope in [Envelope::Failed, Envelope::Error] {
        for tail in [Tail::Pending, Tail::Completed, Tail::Eof] {
            for (mut detail, message) in [
                (json!({"code":"invalid_prompt", "message":PRIVATE}), PRIVATE),
                (json!({"code":"invalid_prompt", "message":""}), ""),
                (
                    json!({"code":"invalid_prompt", "message":" \t\n "}),
                    " \t\n ",
                ),
                (
                    json!({"code":"invalid_prompt", "message":r#"{"detail":"Bad Request"}"#}),
                    r#"{"detail":"Bad Request"}"#,
                ),
                (json!({"code":"invalid_prompt"}), FALLBACK),
                (json!({"code":"invalid_prompt", "message":null}), FALLBACK),
            ] {
                detail["headers"] = json!({"retry-after":"7", "authorization":PRIVATE});
                run_envelope_case(envelope, detail, tail, Expected::InvalidPrompt(message)).await;
            }
            std::io::stdout()
                .lock()
                .write_all(format!("ALAB_PROMPT_SSE_CASE:{envelope:?}:{tail:?}\n").as_bytes())
                .unwrap();
        }
    }
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_prompt_refusal_requires_exact_validated_object_code() {
    const NAME: &str = "standalone_prompt_refusal_requires_exact_validated_object_code";
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    let mut controls = Vec::new();
    for field in ["type", "code", "message", "plan_type", "resets_at"] {
        let mut detail = json!({"code":"invalid_prompt", "message":PRIVATE});
        detail[field] = if field == "resets_at" {
            json!("invalid")
        } else {
            json!(42)
        };
        controls.push(detail);
    }
    controls.extend([
        json!({"type":"invalid_prompt", "message":PRIVATE}),
        json!({"type":"invalid_request_error", "message":PRIVATE}),
        json!({"code":"unknown_error", "type":"invalid_prompt", "message":PRIVATE}),
        json!({"code":"INVALID_PROMPT", "message":PRIVATE}),
        json!({"code":"invalid_prompt_extra", "message":PRIVATE}),
        // Error's derived decoder can accept a positional sequence on the
        // failed-response route; a standalone event still requires object code.
        json!([null, "invalid_prompt", PRIVATE, null, null, null]),
    ]);
    for code in ["insufficient_quota", "rate_limit_exceeded", "cyber_policy"] {
        controls.push(json!({"code":code, "type":"invalid_prompt", "message":PRIVATE}));
    }
    for mut detail in controls {
        if let Some(fields) = detail.as_object_mut() {
            fields.insert("headers".into(), json!({"retry-after":"7"}));
        }
        for (tail, expected) in [
            (
                Tail::Pending,
                Expected::Stream("idle timeout waiting for SSE"),
            ),
            (
                Tail::Eof,
                Expected::Stream("stream closed before response.completed"),
            ),
            (Tail::Completed, Expected::Completed),
        ] {
            run_envelope_case(Envelope::Error, detail.clone(), tail, expected).await;
        }
    }
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_inactive_invalid_prompt_keeps_upstream_ignored_path() {
    const NAME: &str = "standalone_inactive_invalid_prompt_keeps_upstream_ignored_path";
    if isolated(NAME).await {
        return;
    }
    assert!(!codex_alab_core::active());
    let detail = json!({"code":"invalid_prompt", "message":PRIVATE, "headers":{"retry-after":"7"}});
    for (tail, expected) in [
        (
            Tail::Pending,
            Expected::Stream("idle timeout waiting for SSE"),
        ),
        (
            Tail::Eof,
            Expected::Stream("stream closed before response.completed"),
        ),
        (Tail::Completed, Expected::Completed),
    ] {
        run_envelope_case(Envelope::Error, detail.clone(), tail, expected).await;
    }
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn provider_failed_message_cannot_forge_opaque_http_400() {
    const NAME: &str = "provider_failed_message_cannot_forge_opaque_http_400";
    const MESSAGE: &str = r#"opaque backend HTTP 400: {"detail":"Bad Request"}"#;
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    run_case(
        json!({"code":"unknown_error", "message":MESSAGE,
            "headers":{"retry-after":"7", "authorization":PRIVATE}}),
        Tail::Eof,
        Expected::ProviderMarker(MESSAGE),
    )
    .await;
    complete(NAME);
}
