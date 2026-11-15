// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One prepared endpoint request can own several transport attempts.

use codex_api::ApiError;
use codex_api::RequestTelemetry;
use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::RequestBody;
use codex_http_client::Response;
use codex_http_client::RetryAfter;
use codex_http_client::StreamResponse;
use codex_http_client::TransportError;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tracing::Instrument;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(30);
const TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const OPAQUE: &str = r#"{"detail":"Bad Request"}"#;

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
pub(super) struct Capture(pub(super) Arc<Mutex<Vec<BTreeMap<String, String>>>>);

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
        if !matches!(
            event.metadata().target(),
            TARGET | "codex_http_client::response::alab_error_body_capture"
        ) {
            return;
        }
        if event.metadata().target() == "codex_http_client::response::alab_error_body_capture" {
            assert_eq!(*event.metadata().level(), tracing::Level::INFO);
        }
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

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    attempt: u64,
    status: Option<u16>,
    error: bool,
}

#[derive(Default)]
struct Telemetry(Mutex<Vec<Observation>>);

impl RequestTelemetry for Telemetry {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<http::StatusCode>,
        error: Option<&TransportError>,
        _: Duration,
    ) {
        self.0.lock().unwrap().push(Observation {
            attempt,
            status: status.map(|status| status.as_u16()),
            error: error.is_some(),
        });
    }
}

impl Telemetry {
    fn take(&self) -> Vec<Observation> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

enum Reply {
    Transient(&'static str),
    Opaque,
    Complete(&'static str),
    Network(&'static str),
    Timeout,
    HttpAdvice {
        advice: Option<RetryAfter>,
        wire: Option<&'static str>,
    },
}

#[derive(Default)]
struct Script {
    replies: VecDeque<Reply>,
    sent: Vec<Vec<u8>>,
}

struct ScriptedTransport(Arc<Mutex<Script>>);

fn headers(request_id: &'static str, cf_ray: &'static str) -> http::HeaderMap {
    [
        ("x-request-id", request_id),
        ("cf-ray", cf_ray),
        ("authorization", "Bearer never-log-response-header"),
        ("x-unrelated", "never-log-unrelated-header"),
    ]
    .into_iter()
    .map(|(name, value)| (name.parse().unwrap(), value.parse().unwrap()))
    .collect()
}

impl HttpTransport for ScriptedTransport {
    async fn execute(&self, _: Request) -> Result<Response, TransportError> {
        panic!("Responses must use streaming transport");
    }

    async fn stream(&self, request: Request) -> Result<StreamResponse, TransportError> {
        assert_eq!(request.method, http::Method::POST);
        assert!(request.url.ends_with("/v1/responses"));
        assert_eq!(
            request.headers["authorization"],
            "Bearer never-log-request-header"
        );
        let Some(RequestBody::EncodedJson(body)) = request.body else {
            panic!("expected the endpoint's prepared encoded JSON");
        };
        let reply = {
            let mut script = self.0.lock().unwrap();
            script.sent.push(body.as_bytes().to_vec());
            script
                .replies
                .pop_front()
                .expect("no unexpected extra send")
        };
        match reply {
            Reply::HttpAdvice { advice, wire } => {
                let mut headers = headers("req_advice-1", "ray_advice-1");
                if let Some(wire) = wire {
                    headers.insert("retry-after", wire.parse().unwrap());
                }
                Err(TransportError::Http {
                    status: http::StatusCode::SERVICE_UNAVAILABLE,
                    url: Some("https://never-log-url.invalid/token".into()),
                    headers: Some(headers),
                    body: Some("never-log-provider-advice-content".into()),
                    retry_after: advice,
                })
            }
            Reply::Network(message) => Err(TransportError::Network(message.into())),
            Reply::Timeout => Err(TransportError::Timeout),
            Reply::Transient(id) => Err(TransportError::Http {
                status: http::StatusCode::SERVICE_UNAVAILABLE,
                url: Some("https://never-log-url.invalid/token".into()),
                headers: Some(headers(id, "ray_transient-1")),
                body: Some("never-log-transient-provider-content".into()),
                retry_after: None,
            }),
            Reply::Opaque => Err(TransportError::Http {
                status: http::StatusCode::BAD_REQUEST,
                url: Some("https://never-log-url.invalid/token".into()),
                headers: Some(headers("req_final400-1", "ray_final400-1")),
                body: Some(OPAQUE.into()),
                retry_after: None,
            }),
            Reply::Complete(id) => {
                let body = format!(
                    "data: {}\n\ndata: {}\n\n",
                    json!({"type":"response.created", "response":{"id":id}}),
                    json!({"type":"response.completed", "response":{"id":id}}),
                );
                Ok(StreamResponse {
                    status: http::StatusCode::OK,
                    headers: headers("req_success-1", "ray_success-1"),
                    bytes: futures::stream::once(async move { Ok(body.into_bytes().into()) })
                        .boxed(),
                })
            }
        }
    }
}

struct FixtureAuth;

impl codex_api::AuthProvider for FixtureAuth {
    fn add_auth_headers(&self, headers: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        headers.insert(
            "authorization",
            "Bearer never-log-request-header".parse().unwrap(),
        );
        Ok(())
    }
}

fn input(label: &str) -> Value {
    json!({
        "model":"gpt-retry-fixture",
        "input":[
            {"type":"message", "role":"user", "content":[{
                "type":"input_text", "text":format!("never-log-user-input-{label}")
            }]},
            {"type":"function_call", "call_id":"completed-tool", "name":"never-log-tool-name", "arguments":"{}"},
            {"type":"function_call_output", "call_id":"completed-tool", "output":"never-log-completed-tool-output"}
        ],
        "tools":[{"type":"function", "name":"never-log-schema-name", "parameters":{"type":"object"}}]
    })
}

async fn complete(mut stream: codex_api::ResponseStream, expected: &str) {
    tokio::time::timeout(WAIT, async {
        let mut completions = 0;
        while let Some(event) = stream.next().await {
            if let codex_api::ResponseEvent::Completed { response_id, .. } =
                event.expect("a successful retry must not emit its transient error")
            {
                assert_eq!(response_id, expected);
                completions += 1;
            }
        }
        assert_eq!(completions, 1);
    })
    .await
    .expect("successful response and EOF deadline");
}

fn observation(attempt: u64, status: u16, error: bool) -> Observation {
    Observation {
        attempt,
        status: Some(status),
        error,
    }
}

async fn isolated(name: &str) -> bool {
    const CHILD: &str = "ALAB_HTTP_TRANSPORT_KIND_CHILD";
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name));
        return false;
    }
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env(CHILD, name);
    let report = crate::test_child_process::run(command, CHILD_WAIT)
        .await
        .unwrap_or_else(|failure| panic!("isolated transport diagnostic child: {failure}"));
    report.assert_test_success(&exact);
    true
}

fn transport_client(
    script: Arc<Mutex<Script>>,
    auth: Arc<dyn codex_api::AuthProvider>,
    telemetry: Arc<Telemetry>,
    retry_transport: bool,
) -> codex_api::ResponsesClient<ScriptedTransport> {
    codex_api::ResponsesClient::new(
        ScriptedTransport(script),
        codex_api::Provider {
            name: "transport-kind-fixture".into(),
            base_url: "https://never-log-destination.invalid/v1".into(),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: false,
                retry_transport,
            },
            stream_idle_timeout: WAIT,
        },
        auth,
    )
    .with_telemetry(Some(telemetry), None)
}

async fn terminal_error(
    client: &codex_api::ResponsesClient<ScriptedTransport>,
    request: Value,
) -> ApiError {
    let thread = tracing::info_span!("session_loop", thread_id = "transport-original-thread");
    let turn = tracing::info_span!(
        parent: &thread,
        "turn",
        thread.id = "transport-original-thread",
        turn.id = "transport-original-turn"
    );
    match tokio::time::timeout(
        WAIT,
        client
            .stream(
                request,
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            )
            .instrument(turn),
    )
    .await
    .expect("typed terminal endpoint deadline")
    {
        Err(error) => error,
        Ok(_) => panic!("the endpoint must preserve its selected failure"),
    }
}

fn assert_transport_records(capture: &Capture, expected_kind: &str) {
    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), 2, "one shape and one selected failure");
    let shape = &records[0];
    let failed = &records[1];
    assert_eq!(shape["message"], "Responses request shape");
    assert_eq!(failed["message"], "Responses request failed");
    assert_eq!(shape["local_attempt"], failed["local_attempt"]);
    for record in [shape, failed] {
        assert_eq!(record["transport"], "\"responses_http\"");
        assert_eq!(record["thread_id"], "\"transport-original-thread\"");
        assert_eq!(record["thread.id"], "\"transport-original-thread\"");
        assert_eq!(record["turn.id"], "\"transport-original-turn\"");
    }
    assert_eq!(shape["input_items"], "3");
    assert_eq!(shape["tool_calls"], "1");
    assert_eq!(shape["tool_outputs"], "1");
    assert_eq!(failed["opaque_bad_request"], "false");
    for absent in ["http_status", "request_id", "cf_ray"] {
        assert!(!failed.contains_key(absent), "{failed:?}");
    }
    let evidence = format!("{records:?}");
    for forbidden in [
        "never-log",
        "authorization",
        "x-unrelated",
        "completed-tool",
    ] {
        assert!(!evidence.contains(forbidden), "{evidence}");
    }
    // Check the classification after all behavioral/context/privacy controls,
    // so the old fallback proves precisely the missing typed evidence.
    assert_eq!(failed["failure_kind"], format!("{expected_kind:?}"));
}

#[tokio::test(flavor = "current_thread")]
async fn transport_retry_logs_only_selected_timeout_and_preserves_encoded_input() {
    if isolated("transport_retry_logs_only_selected_timeout_and_preserves_encoded_input").await {
        return;
    }
    codex_alab_core::activate();
    let capture = Capture::default();
    let _subscriber =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
    let script = Arc::new(Mutex::new(Script {
        replies: VecDeque::from([
            Reply::Network("never-log-transient-network-content"),
            Reply::Timeout,
        ]),
        sent: Vec::new(),
    }));
    let telemetry = Arc::new(Telemetry::default());
    let client = transport_client(
        script.clone(),
        Arc::new(FixtureAuth),
        telemetry.clone(),
        true,
    );
    let request = input("terminal-timeout");
    let expected = serde_json::to_vec(&request).unwrap();
    assert!(matches!(
        terminal_error(&client, request).await,
        ApiError::Transport(TransportError::Timeout)
    ));
    assert_eq!(
        telemetry.take(),
        [
            Observation {
                attempt: 0,
                status: None,
                error: true,
            },
            Observation {
                attempt: 1,
                status: None,
                error: true,
            },
        ]
    );
    let script = script.lock().unwrap();
    assert!(script.replies.is_empty());
    assert_eq!(script.sent, [expected.clone(), expected]);
    assert_transport_records(&capture, "timeout");
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_network_failure_preserves_private_error_without_logging_it() {
    if isolated("terminal_network_failure_preserves_private_error_without_logging_it").await {
        return;
    }
    const PRIVATE: &str = "never-log-terminal-network-content";
    codex_alab_core::activate();
    let capture = Capture::default();
    let _subscriber =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
    let script = Arc::new(Mutex::new(Script {
        replies: VecDeque::from([Reply::Network(PRIVATE)]),
        sent: Vec::new(),
    }));
    let telemetry = Arc::new(Telemetry::default());
    let client = transport_client(
        script.clone(),
        Arc::new(FixtureAuth),
        telemetry.clone(),
        false,
    );
    let request = input("terminal-network");
    let expected = serde_json::to_vec(&request).unwrap();
    match terminal_error(&client, request).await {
        ApiError::Transport(TransportError::Network(message)) => assert_eq!(message, PRIVATE),
        _ => panic!("the original typed network error must survive"),
    }
    assert_eq!(
        telemetry.take(),
        [Observation {
            attempt: 0,
            status: None,
            error: true,
        }]
    );
    let script = script.lock().unwrap();
    assert!(script.replies.is_empty());
    assert_eq!(script.sent, [expected]);
    assert_transport_records(&capture, "network");
}

#[tokio::test(flavor = "current_thread")]
async fn selected_http_advice_presence_uses_typed_state_even_when_expired() {
    if isolated("selected_http_advice_presence_uses_typed_state_even_when_expired").await {
        return;
    }
    tokio::time::pause();
    codex_alab_core::activate();
    let mut observations = Vec::new();
    for (delay, wire, elapsed) in [
        (None, None, Duration::ZERO),
        (None, Some("7"), Duration::ZERO),
        (Some(Duration::from_secs(7)), None, Duration::ZERO),
        (Some(Duration::ZERO), None, Duration::ZERO),
        (Some(Duration::from_secs(1)), None, Duration::from_secs(2)),
    ] {
        let capture = Capture::default();
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        let captured_at = tokio::time::Instant::now();
        let advice = delay.map(|delay| RetryAfter::from_delay(delay).unwrap());
        if !elapsed.is_zero() {
            tokio::time::advance(elapsed).await;
            assert!(
                tokio::time::Instant::now() > captured_at + delay.unwrap(),
                "this retained Some is genuinely expired before the endpoint sees it"
            );
        }
        let script = Arc::new(Mutex::new(Script {
            replies: VecDeque::from([Reply::HttpAdvice { advice, wire }]),
            sent: Vec::new(),
        }));
        let telemetry = Arc::new(Telemetry::default());
        let client = transport_client(
            script.clone(),
            Arc::new(FixtureAuth),
            telemetry.clone(),
            false,
        );
        let request = input("http-advice");
        let expected_bytes = serde_json::to_vec(&request).unwrap();
        let error = terminal_error(&client, request).await;
        match &error {
            ApiError::Transport(TransportError::Http {
                status,
                url,
                headers: Some(original),
                body,
                retry_after,
            }) => {
                assert_eq!(*status, http::StatusCode::SERVICE_UNAVAILABLE);
                assert_eq!(url.as_deref(), Some("https://never-log-url.invalid/token"));
                assert_eq!(body.as_deref(), Some("never-log-provider-advice-content"));
                assert_eq!(
                    *retry_after, advice,
                    "the original retained advice is unchanged"
                );
                assert_eq!(
                    original
                        .get("retry-after")
                        .and_then(|value| value.to_str().ok()),
                    wire
                );
                assert_eq!(original["x-request-id"], "req_advice-1");
                assert_eq!(original["x-unrelated"], "never-log-unrelated-header");
            }
            _ => panic!("the original HTTP failure and headers must survive"),
        }
        let mapped = codex_api::map_api_error(error);
        assert_eq!(
            mapped.retry_after(),
            advice,
            "mapping preserves Some/None without reparsing the raw header"
        );
        assert!(mapped.retry_delay(1).is_some());
        assert_eq!(telemetry.take(), [observation(0, 503, true)]);
        let script = script.lock().unwrap();
        assert!(script.replies.is_empty());
        assert_eq!(script.sent, [expected_bytes]);
        let records = capture.0.lock().unwrap();
        assert_eq!(records.len(), 2);
        let shape = &records[0];
        let failed = &records[1];
        assert_eq!(shape["message"], "Responses request shape");
        assert!(!shape.contains_key("retry_advised"));
        assert_eq!(failed["message"], "Responses request failed");
        assert_eq!(shape["local_attempt"], failed["local_attempt"]);
        assert_eq!(failed["failure_kind"], "\"http\"");
        assert_eq!(failed["http_status"], "503");
        assert_eq!(failed["request_id"], "\"req_advice-1\"");
        assert_eq!(failed["cf_ray"], "\"ray_advice-1\"");
        for record in [shape, failed] {
            assert_eq!(record["thread_id"], "\"transport-original-thread\"");
            assert_eq!(record["thread.id"], "\"transport-original-thread\"");
            assert_eq!(record["turn.id"], "\"transport-original-turn\"");
        }
        let text = format!("{records:?}");
        for private in [
            "never-log",
            "authorization",
            "x-unrelated",
            "retry-after",
            "completed-tool",
        ] {
            assert!(
                !text.contains(private),
                "private content is absent from metadata"
            );
        }
        observations.push((delay, wire, failed.get("retry_advised").cloned()));
    }
    // All future, zero, expired, absent and contradictory-header rows have
    // passed the original HTTP/error/recovery/correlation/privacy controls.
    for (delay, wire, advised) in observations {
        assert_eq!(
            advised.as_deref(),
            Some(if delay.is_some() { "true" } else { "false" }),
            "typed={delay:?}, raw={wire:?}"
        );
    }
}

#[derive(Clone, Copy)]
enum AuthFailure {
    Policy,
    Build,
}

struct RejectingAuth {
    failure: AuthFailure,
    calls: Mutex<usize>,
}

impl codex_api::AuthProvider for RejectingAuth {
    fn auth_header_telemetry(
        &self,
    ) -> Result<codex_api::AuthHeaderTelemetry, codex_api::AuthError> {
        // Observing header metadata must not perform a credential application.
        Ok(codex_api::AuthHeaderTelemetry::default())
    }

    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        *self.calls.lock().unwrap() += 1;
        Err(match self.failure {
            AuthFailure::Policy => {
                codex_api::AuthError::Policy(codex_http_client::NetworkPolicyDenied::Destination)
            }
            AuthFailure::Build => {
                codex_api::AuthError::Build("never-log-auth-build-content".into())
            }
        })
    }
}

async fn auth_failure_case(failure: AuthFailure) {
    codex_alab_core::activate();
    let capture = Capture::default();
    let _subscriber =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
    let script = Arc::new(Mutex::new(Script::default()));
    let auth = Arc::new(RejectingAuth {
        failure,
        calls: Mutex::new(0),
    });
    let telemetry = Arc::new(Telemetry::default());
    // Enable transport retries: these auth failures must still stop before send.
    let client = transport_client(script.clone(), auth.clone(), telemetry.clone(), true);
    let error = terminal_error(&client, input("auth-rejection")).await;
    match (failure, error) {
        (
            AuthFailure::Policy,
            ApiError::Transport(TransportError::Policy(
                codex_http_client::NetworkPolicyDenied::Destination,
            )),
        ) => {}
        (AuthFailure::Build, ApiError::Transport(TransportError::Build(message))) => {
            assert_eq!(message, "never-log-auth-build-content");
        }
        _ => panic!("the original typed auth failure must survive"),
    }
    assert_eq!(*auth.calls.lock().unwrap(), 1);
    assert!(script.lock().unwrap().sent.is_empty());
    assert_eq!(
        telemetry.take(),
        [Observation {
            attempt: 0,
            status: None,
            error: true,
        }]
    );
    assert_transport_records(
        &capture,
        match failure {
            AuthFailure::Policy => "network_policy",
            AuthFailure::Build => "request_build",
        },
    );
}

#[tokio::test(flavor = "current_thread")]
async fn policy_auth_rejection_is_not_retried_or_sent_and_is_classified() {
    if isolated("policy_auth_rejection_is_not_retried_or_sent_and_is_classified").await {
        return;
    }
    auth_failure_case(AuthFailure::Policy).await;
}

#[tokio::test(flavor = "current_thread")]
async fn build_auth_rejection_is_not_retried_or_sent_and_is_classified() {
    if isolated("build_auth_rejection_is_not_retried_or_sent_and_is_classified").await {
        return;
    }
    auth_failure_case(AuthFailure::Build).await;
}

#[tokio::test(flavor = "current_thread")]
async fn internal_http_retries_share_one_endpoint_diagnostic_and_preserve_input() {
    const CHILD: &str = "ALAB_HTTP_RETRY_DIAGNOSTIC_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let name = format!(
            "{}::internal_http_retries_share_one_endpoint_diagnostic_and_preserve_input",
            module_path!().split_once("::").unwrap().1
        );
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1");
        let report = crate::test_child_process::run(command, CHILD_WAIT)
            .await
            .unwrap_or_else(|failure| {
                panic!("isolated HTTP retry diagnostic regression: {failure}")
            });
        report.assert_test_success(&name);
        return;
    }

    codex_alab_core::activate();
    let capture = Capture::default();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(capture.clone()))
        .unwrap();
    let script = Arc::new(Mutex::new(Script {
        replies: VecDeque::from([
            Reply::Transient("req_transient-a"),
            Reply::Opaque,
            Reply::Transient("req_transient-b"),
            Reply::Complete("retried-success"),
            Reply::Complete("next-endpoint-success"),
        ]),
        sent: Vec::new(),
    }));
    let telemetry = Arc::new(Telemetry::default());
    let client = codex_api::ResponsesClient::new(
        ScriptedTransport(script.clone()),
        codex_api::Provider {
            name: "retry-fixture".into(),
            base_url: "https://example.invalid/v1".into(),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                // This is one internal retry after the initial transport send.
                max_attempts: 1,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: true,
                retry_transport: false,
            },
            stream_idle_timeout: WAIT,
        },
        Arc::new(FixtureAuth),
    )
    .with_telemetry(Some(telemetry.clone()), None);

    let rejected = input("rejected");
    let first = tokio::time::timeout(
        WAIT,
        client
            .stream(
                rejected.clone(),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            )
            .instrument(tracing::info_span!(
                "logical_request",
                logical_call = "rejected"
            )),
    )
    .await
    .expect("terminal endpoint deadline");
    match first {
        Err(ApiError::Transport(TransportError::Http {
            status,
            url,
            headers: Some(original_headers),
            body,
            retry_after,
        })) => {
            assert_eq!(status, http::StatusCode::BAD_REQUEST);
            assert_eq!(url.as_deref(), Some("https://never-log-url.invalid/token"));
            assert_eq!(original_headers["x-request-id"], "req_final400-1");
            assert_eq!(
                original_headers["x-unrelated"],
                "never-log-unrelated-header"
            );
            assert_eq!(body.as_deref(), Some(OPAQUE));
            assert!(retry_after.is_none());
        }
        _ => panic!("endpoint must retain its original terminal HTTP error"),
    }
    assert_eq!(
        telemetry.take(),
        [observation(0, 503, true), observation(1, 400, true)]
    );
    assert_eq!(capture.0.lock().unwrap().len(), 2);

    let recovered = input("recovered");
    let second = tokio::time::timeout(
        WAIT,
        client
            .stream(
                recovered.clone(),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            )
            .instrument(tracing::info_span!(
                "logical_request",
                logical_call = "recovered"
            )),
    )
    .await
    .expect("retrying endpoint deadline")
    .expect("the second logical request recovers its transient HTTP error");
    complete(second, "retried-success").await;
    assert_eq!(
        telemetry.take(),
        [observation(0, 503, true), observation(1, 200, false)]
    );
    assert_eq!(capture.0.lock().unwrap().len(), 3);

    let next = input("next");
    let third = tokio::time::timeout(
        WAIT,
        client
            .stream(
                next.clone(),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            )
            .instrument(tracing::info_span!(
                "logical_request",
                logical_call = "next"
            )),
    )
    .await
    .expect("subsequent endpoint deadline")
    .expect("subsequent logical request starts with a fresh transport attempt counter");
    complete(third, "next-endpoint-success").await;
    assert_eq!(telemetry.take(), [observation(0, 200, false)]);

    let script = script.lock().unwrap();
    assert!(script.replies.is_empty());
    let rejected_bytes = serde_json::to_vec(&rejected).unwrap();
    let recovered_bytes = serde_json::to_vec(&recovered).unwrap();
    let next_bytes = serde_json::to_vec(&next).unwrap();
    assert_eq!(
        script.sent,
        [
            rejected_bytes.clone(),
            rejected_bytes,
            recovered_bytes.clone(),
            recovered_bytes,
            next_bytes,
        ],
        "each retry retains the original encoded prompt and completed tool pair"
    );

    let records = capture.0.lock().unwrap();
    assert_eq!(
        records.len(),
        4,
        "three shapes and only the selected final failure"
    );
    let shapes: Vec<_> = records
        .iter()
        .filter(|record| record["message"] == "Responses request shape")
        .collect();
    let failures: Vec<_> = records
        .iter()
        .filter(|record| record["message"] == "Responses request failed")
        .collect();
    assert_eq!(shapes.len(), 3);
    assert_eq!(failures.len(), 1);
    let ids: BTreeSet<_> = shapes
        .iter()
        .map(|record| &record["local_attempt"])
        .collect();
    assert_eq!(ids.len(), 3, "new logical calls receive distinct local IDs");
    let failed = failures[0];
    assert_eq!(failed["local_attempt"], shapes[0]["local_attempt"]);
    assert_eq!(failed["logical_call"], "\"rejected\"");
    assert_eq!(failed["http_status"], "400");
    assert_eq!(failed["opaque_bad_request"], "true");
    assert_eq!(failed["failure_kind"], "\"http\"");
    assert_eq!(failed["request_id"], "\"req_final400-1\"");
    assert_eq!(failed["cf_ray"], "\"ray_final400-1\"");
    for (shape, label) in shapes.iter().zip(["rejected", "recovered", "next"]) {
        assert_eq!(shape["logical_call"], format!("{label:?}"));
        assert_eq!(shape["input_items"], "3");
        assert_eq!(shape["tool_calls"], "1");
        assert_eq!(shape["tool_outputs"], "1");
    }
    let evidence = format!("{records:?}");
    for forbidden in [
        "never-log",
        "req_transient-a",
        "req_transient-b",
        "ray_transient-1",
        "req_success-1",
        "ray_success-1",
        "authorization",
        "x-unrelated",
        "Bad Request",
    ] {
        assert!(!evidence.contains(forbidden), "{evidence}");
    }
}
