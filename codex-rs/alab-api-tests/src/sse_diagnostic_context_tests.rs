// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The real Registry must retain request ancestors after the endpoint future returns.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const CHILD_WAIT: Duration = Duration::from_secs(60);
const DIAGNOSTIC_TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";

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
        if event.metadata().target() != DIAGNOSTIC_TARGET
            || *event.metadata().level() != tracing::Level::INFO
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        if !matches!(
            fields.0.get("message").map(String::as_str),
            Some("Responses request shape" | "Responses request failed")
        ) {
            return;
        }
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

struct NoAuth;

impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_sse_failure_retains_real_parent_spans_after_caller_context_exits() {
    const CHILD: &str = "ALAB_SSE_CONTEXT_REGRESSION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Activation is process-wide; isolate this native endpoint regression.
        let name = format!(
            "{}::native_sse_failure_retains_real_parent_spans_after_caller_context_exits",
            module_path!().split_once("::").unwrap().1
        );
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1");
        let report = crate::test_child_process::run(command, CHILD_WAIT)
            .await
            .unwrap_or_else(|failure| panic!("isolated context regression: {failure}"));
        report.assert_test_success(&name);
        return;
    }

    codex_alab_core::activate();
    let mut server = crate::sse_fixture::SseFixture::start().await;

    let transport = codex_api::ReqwestTransport::from_http_client(
        codex_http_client::HttpClientBuilder::new()
            .without_request_logging()
            .build_direct()
            .expect("direct loopback client"),
    );
    let client = codex_api::ResponsesClient::new(
        transport,
        codex_api::Provider {
            name: "context-fixture".into(),
            base_url: format!("{}/v1", server.uri()),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_secs(5),
        },
        Arc::new(NoAuth),
    );
    let capture = Capture::default();
    let mut stream = {
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let thread = tracing::info_span!("fixture_thread", thread_id = "original-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!("fixture_turn", turn_id = "original-turn");
        let _turn = turn.enter();
        tokio::time::timeout(
            Duration::from_secs(5),
            client.stream(
                serde_json::json!({
                    "model":"gpt-context-fixture",
                    "input":[{"type":"message","role":"user","content":"never-log-input"}]
                }),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            ),
        )
        .await
        .expect("HTTP opening deadline")
        .expect("HTTP 200 opens a stream before the provider fails")
    };
    // Both entered parent spans, their handles, and the caller's subscriber
    // guard have dropped before the spawned SSE task can observe the failure.
    assert_eq!(
        capture.0.lock().unwrap().len(),
        1,
        "shape only before release"
    );
    server.release_failure();
    let error = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.rx_event.recv().await {
                Some(Err(error)) => break error,
                Some(Ok(_)) => {}
                None => panic!("SSE stream closed without the provider error"),
            }
        }
    })
    .await
    .expect("terminal SSE delivery");
    assert!(matches!(error, codex_api::ApiError::InvalidRequest { .. }));
    server
        .finish()
        .await
        .expect("bounded SSE server completion");

    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), 2, "one shape and one semantic failure");
    let shape = &records[0];
    let failed = &records[1];
    assert_eq!(shape["message"], "Responses request shape");
    assert_eq!(failed["message"], "Responses request failed");
    assert_eq!(shape["local_attempt"], failed["local_attempt"]);
    for event in [shape, failed] {
        assert_eq!(event["thread_id"], "\"original-thread\"");
        assert_eq!(event["turn_id"], "\"original-turn\"");
    }
    assert_eq!(failed["request_id"], "\"req_context-1\"");
    assert_eq!(failed["cf_ray"], "\"ray_context-1\"");
    assert_eq!(failed["failure_kind"], "\"invalid_request\"");
    assert!(
        !failed.contains_key("http_status"),
        "opening 200 is not an error status"
    );
    let text = format!("{records:?}");
    for forbidden in [
        "never-log",
        "invalid identifier",
        "authorization",
        "invalid_access_program",
    ] {
        assert!(!text.contains(forbidden), "{text}");
    }
}

// The request diagnostic must restore only its own event context, rather than
// changing the subscriber seen by the existing websocket telemetry callbacks.
type ScopedRequestSubscriber =
    tracing_subscriber::layer::Layered<Capture, tracing_subscriber::Registry>;

fn request_subscriber_is_current() -> bool {
    tracing::dispatcher::get_default(|dispatch| {
        dispatch.downcast_ref::<ScopedRequestSubscriber>().is_some()
    })
}

#[derive(Clone, Default)]
struct WebsocketCallbackContext(Arc<Mutex<Vec<bool>>>);

impl codex_api::WebsocketTelemetry for WebsocketCallbackContext {
    fn on_ws_request(&self, _: Duration, _: Option<&codex_api::ApiError>, _: bool) {}

    fn on_ws_receive_failure(&self, _: Duration) {}

    fn on_ws_text_event(&self, _: &str, _: Option<&str>, _: Duration) {
        self.0.lock().unwrap().push(request_subscriber_is_current());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_websocket_failure_retains_request_dispatch_after_caller_context_exits() {
    const CHILD: &str = "ALAB_WS_CONTEXT_REGRESSION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let name = format!(
            "{}::native_websocket_failure_retains_request_dispatch_after_caller_context_exits",
            module_path!().split_once("::").unwrap().1
        );
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1");
        let report = crate::test_child_process::run(command, CHILD_WAIT)
            .await
            .unwrap_or_else(|failure| panic!("isolated websocket context regression: {failure}"));
        report.assert_test_success(&name);
        return;
    }

    codex_alab_core::activate();
    let failure = serde_json::json!({
        "type":"error",
        "error":{
            "code":"invalid_access_program",
            "message":"never-log-websocket-provider-content"
        },
        "headers":{"authorization":"Bearer never-log-websocket-header"}
    });
    let mut server = crate::websocket_fixture::WebsocketFixture::start(failure).await;
    let callback_context = WebsocketCallbackContext::default();
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "context-fixture".into(),
            base_url: format!("{}/v1", server.uri()),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_secs(5),
        },
        Arc::new(NoAuth),
    );
    let factory = codex_http_client::HttpClientFactory::new(
        codex_http_client::OutboundProxyPolicy::ReqwestDefault,
    );
    let connection = tokio::time::timeout(
        Duration::from_secs(5),
        client.connect(
            &factory,
            http::HeaderMap::new(),
            http::HeaderMap::new(),
            None,
            Some(Arc::new(callback_context.clone())),
        ),
    )
    .await
    .expect("websocket connection deadline")
    .expect("loopback websocket connection");
    let request = crate::streaming_request("gpt-context-fixture", "never-log-websocket-input");
    let capture = Capture::default();
    let mut stream = {
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let _subscriber = tracing::subscriber::set_default(subscriber);
        assert!(request_subscriber_is_current(), "positive context control");
        let thread = tracing::info_span!("fixture_thread", thread_id = "original-ws-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!("fixture_turn", turn_id = "original-ws-turn");
        let _turn = turn.enter();
        let stream = tokio::time::timeout(
            Duration::from_secs(5),
            connection.stream_request(
                codex_api::ResponsesWsRequest::ResponseCreate(
                    codex_api::ResponseCreateWsRequest::from(&request),
                ),
                false,
                None,
            ),
        )
        .await
        .expect("websocket request opening deadline")
        .expect("request opens before its gated semantic error");
        let observed = server.observed_request().await;
        assert_eq!(observed["type"], "response.create");
        stream
    };
    // The request reached the server, but its failure remains gated until both
    // parent span handles and the caller's thread-local subscriber have dropped.
    assert!(!request_subscriber_is_current());
    assert_eq!(
        capture.0.lock().unwrap().len(),
        1,
        "shape only before release"
    );
    assert!(callback_context.0.lock().unwrap().is_empty());
    server.release_failure();
    let error = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.rx_event.recv().await {
                Some(Err(error)) => break error,
                Some(Ok(_)) => {}
                None => panic!("websocket closed without its gated error"),
            }
        }
    })
    .await
    .expect("terminal websocket error delivery");
    assert!(matches!(error, codex_api::ApiError::InvalidRequest { .. }));
    server.finish().await;

    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), 2, "one shape and one correlated failure");
    let shape = &records[0];
    let failed = &records[1];
    assert_eq!(shape["message"], "Responses request shape");
    assert_eq!(failed["message"], "Responses request failed");
    assert_eq!(shape["local_attempt"], failed["local_attempt"]);
    for event in [shape, failed] {
        assert_eq!(event["transport"], "\"responses_websocket\"");
        assert_eq!(event["thread_id"], "\"original-ws-thread\"");
        assert_eq!(event["turn_id"], "\"original-ws-turn\"");
    }
    assert_eq!(failed["failure_kind"], "\"invalid_request\"");
    assert!(!failed.contains_key("http_status"));
    assert_eq!(
        *callback_context.0.lock().unwrap(),
        [false],
        "diagnostic restoration must not change the telemetry callback's subscriber"
    );
    let text = format!("{records:?}");
    for forbidden in ["never-log", "authorization", "invalid_access_program"] {
        assert!(!text.contains(forbidden), "{text}");
    }
}

const REPEATED_WAIT: Duration = Duration::from_secs(5);
const REPEATED_CHILD: &str = "ALAB_SSE_REPEATED_HEADERS_CHILD";
const REPEATED_COMPLETE: &str = "ALAB_SSE_REPEATED_HEADERS_OK";
const REPEATED_PRIVATE: &str = "never-log-provider-content";
const REPEATED_PROGRESS_ID: &str = "never-log-private-created-progress-id";

#[derive(Clone, Copy, Debug)]
enum RepeatedHeaders {
    InvalidFirstThenPrimary,
    FirstPrimaryStaysFirst,
    LaterAlias,
    AllInvalid,
    RequestOnly,
    RayOnly,
}

impl RepeatedHeaders {
    fn headers(self) -> Vec<u8> {
        let fields: &[(&str, &str)] = match self {
            Self::InvalidFirstThenPrimary => &[
                ("x-request-id", "never-log-invalid id"),
                ("X-ReQuEsT-Id", "req_sse-first-1"),
                ("x-request-id", "req_sse-second-2"),
                ("x-oai-request-id", "req_sse-alias-1"),
                ("cf-ray", "never-log-invalid ray"),
                ("Cf-RaY", "ray_sse-first-1"),
                ("cf-ray", "ray_sse-second-2"),
            ],
            Self::FirstPrimaryStaysFirst => &[
                ("x-request-id", "req_sse-first-1"),
                ("x-request-id", "never-log-invalid id"),
                ("x-request-id", "req_sse-second-2"),
                ("x-oai-request-id", "req_sse-alias-1"),
                ("cf-ray", "ray_sse-first-1"),
                ("cf-ray", "ray_sse-second-2"),
            ],
            Self::LaterAlias => &[
                ("x-request-id", "never-log-invalid id"),
                ("x-oai-request-id", "never-log-invalid alias"),
                ("X-OaI-ReQuEsT-Id", "req_sse-alias-1"),
                ("cf-ray", "ray_sse-first-1"),
            ],
            Self::AllInvalid => &[
                ("x-request-id", "never-log-invalid id"),
                ("x-request-id", "id_pair,other"),
                ("x-oai-request-id", "never-log-invalid alias"),
                ("cf-ray", "never-log-invalid ray"),
                ("cf-ray", "ray_pair,other"),
            ],
            Self::RequestOnly => &[
                ("x-request-id", "never-log-invalid id"),
                ("x-request-id", "req_sse-first-1"),
                ("cf-ray", "never-log-invalid ray"),
            ],
            Self::RayOnly => &[
                ("x-request-id", "never-log-invalid id"),
                ("x-oai-request-id", "never-log-invalid alias"),
                ("cf-ray", "never-log-invalid ray"),
                ("cf-ray", "ray_sse-first-1"),
            ],
        };
        let mut headers = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\nauthorization: Bearer never-log-header\r\nx-unrelated: never-log-unrelated-header\r\n".to_vec();
        for (name, value) in fields {
            headers.extend_from_slice(name.as_bytes());
            headers.extend_from_slice(b": ");
            headers.extend_from_slice(value.as_bytes());
            headers.extend_from_slice(b"\r\n");
        }
        headers.extend_from_slice(b"\r\n");
        headers
    }

    fn ids(self) -> (Option<&'static str>, Option<&'static str>) {
        match self {
            Self::InvalidFirstThenPrimary | Self::FirstPrimaryStaysFirst => {
                (Some("req_sse-first-1"), Some("ray_sse-first-1"))
            }
            Self::LaterAlias => (Some("req_sse-alias-1"), Some("ray_sse-first-1")),
            Self::AllInvalid => (None, None),
            Self::RequestOnly => (Some("req_sse-first-1"), None),
            Self::RayOnly => (None, Some("ray_sse-first-1")),
        }
    }
}

async fn repeated_header_exchange(case: RepeatedHeaders, deferred: bool) {
    let code = if deferred {
        "unknown_failure"
    } else {
        "invalid_request"
    };
    let frame = serde_json::json!({
        "type":"response.failed", "response":{"error":{
            "code":code, "message":REPEATED_PRIVATE,
            "private-body-field":"never-log-extra-body"
        }}
    });
    let mut events = format!("data: {frame}\n\n");
    if deferred {
        let progress = serde_json::json!({
            "type":"response.created", "response":{"id":REPEATED_PROGRESS_ID}
        });
        events.push_str(&format!("data: {progress}\n\n"));
    }
    let mut server = crate::sse_fixture::SseFixture::start_scripted_response(
        case.headers(),
        events.into_bytes(),
    )
    .await;
    let transport = codex_api::ReqwestTransport::from_http_client(
        codex_http_client::HttpClientBuilder::new()
            .without_request_logging()
            .build_direct()
            .expect("direct repeated-header loopback client"),
    );
    let client = codex_api::ResponsesClient::new(
        transport,
        codex_api::Provider {
            name: "repeated-sse-fixture".into(),
            base_url: format!("{}/v1", server.uri()),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: REPEATED_WAIT,
        },
        Arc::new(NoAuth),
    );
    let capture = Capture::default();
    let mut stream = {
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        assert!(request_subscriber_is_current());
        let thread = tracing::info_span!("repeated_sse_thread", thread_id = "repeated-sse-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!("repeated_sse_turn", turn_id = "repeated-sse-turn");
        let _turn = turn.enter();
        tokio::time::timeout(
            REPEATED_WAIT,
            client.stream(
                serde_json::json!({"model":"gpt-repeated-sse-fixture", "input":[{
                    "type":"message", "role":"user", "content":"never-log-repeated-input"
                }]}),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            ),
        )
        .await
        .expect("repeated-header HTTP opening deadline")
        .expect("real HTTP200 opens before the gated SSE event")
    };
    assert!(!request_subscriber_is_current(), "caller subscriber exited");
    assert!(
        tracing::Span::current().is_disabled(),
        "caller span handles exited"
    );
    assert_eq!(
        capture.0.lock().unwrap().len(),
        1,
        "shape only before release"
    );
    assert!(server.eof_is_held());
    let prelude = tokio::time::timeout(REPEATED_WAIT, stream.rx_event.recv())
        .await
        .expect("opening rate-limit snapshot deadline");
    assert!(
        matches!(prelude, Some(Ok(codex_api::ResponseEvent::RateLimits(_)))),
        "exactly the expected opening snapshot: {prelude:?}"
    );
    assert!(matches!(
        stream.rx_event.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    server.release_failure();
    server.wait_for_event_written().await;
    assert!(server.eof_is_held());
    if deferred {
        // The ordered public witness establishes processing beyond the failed
        // frame. EOF must still select that original buffered error afterward.
        // This does not measure scheduler timing or a complete wire inventory.
        let progress = tokio::time::timeout(REPEATED_WAIT, stream.rx_event.recv())
            .await
            .expect("ordered public SSE progress deadline");
        assert!(
            matches!(&progress,
                Some(Ok(codex_api::ResponseEvent::Created { response_id: Some(id) }))
                    if id == REPEATED_PROGRESS_ID),
            "expected only the exact public progress witness, got {progress:?}"
        );
        assert!(
            server.eof_is_held(),
            "consumer progresses while EOF is withheld"
        );
        assert_eq!(capture.0.lock().unwrap().len(), 1);
        server.release_eof();
    }
    let outcome = tokio::time::timeout(REPEATED_WAIT, stream.rx_event.recv())
        .await
        .expect("selected repeated-header SSE outcome deadline");
    let error = match outcome {
        Some(Err(error)) => error,
        other => panic!("expected the selected original SSE failure, got {other:?}"),
    };
    if deferred {
        let codex_api::ApiError::Retryable {
            message,
            retry_after,
        } = &error
        else {
            panic!("EOF must select the original buffered Retryable, got {error:?}");
        };
        assert_eq!(message, REPEATED_PRIVATE);
        assert!(retry_after.is_none());
    } else {
        let codex_api::ApiError::InvalidRequest { message } = &error else {
            panic!("expected immediate InvalidRequest, got {error:?}");
        };
        assert_eq!(message, REPEATED_PRIVATE);
        assert!(
            server.eof_is_held(),
            "immediate outcome precedes fixture EOF"
        );
    }
    let mapped = codex_api::map_api_error(error);
    assert_eq!(
        mapped.to_string(),
        if deferred {
            format!("stream disconnected before completion: {REPEATED_PRIVATE}")
        } else {
            REPEATED_PRIVATE.into()
        }
    );
    assert_eq!(mapped.retry_delay(1).is_some(), deferred);
    assert!(mapped.retry_after().is_none());
    assert!(!mapped.requires_http_fallback());
    assert!(
        tokio::time::timeout(REPEATED_WAIT, stream.rx_event.recv())
            .await
            .expect("selected SSE task closes its channel")
            .is_none()
    );
    if !deferred {
        assert!(
            server.eof_is_held(),
            "channel closes while EOF is still withheld"
        );
        server.release_eof();
    }
    server
        .finish()
        .await
        .expect("bounded joined repeated-header fixture cleanup");

    let records = capture.0.lock().unwrap();
    assert_eq!(
        records.len(),
        2,
        "one shape plus one selected semantic failure"
    );
    let shape = &records[0];
    let failed = &records[1];
    assert_eq!(shape["message"], "Responses request shape");
    assert_eq!(failed["message"], "Responses request failed");
    assert!(shape["local_attempt"].parse::<u64>().unwrap() > 0);
    assert_eq!(failed["local_attempt"], shape["local_attempt"]);
    assert_eq!(
        failed["failure_kind"],
        if deferred {
            "\"retryable\""
        } else {
            "\"invalid_request\""
        }
    );
    assert!(!shape.contains_key("retry_advised"));
    if deferred {
        assert_eq!(
            failed.get("retry_advised").map(String::as_str),
            Some("false")
        );
    } else {
        assert!(!failed.contains_key("retry_advised"));
    }
    for record in records.iter() {
        assert_eq!(record["transport"], "\"responses_http\"");
        assert_eq!(record["thread_id"], "\"repeated-sse-thread\"");
        assert_eq!(record["turn_id"], "\"repeated-sse-turn\"");
        for field in [
            "http_status",
            "code",
            "error_code",
            "error_message",
            "error.message",
            "body",
            "payload",
            "headers",
        ] {
            assert!(
                !record.contains_key(field),
                "opening200 is not an error or payload record"
            );
        }
    }
    let logged = format!("{records:?}");
    for forbidden in [
        "never-log",
        REPEATED_PROGRESS_ID,
        "authorization",
        "x-unrelated",
        "unknown_failure",
        "id_pair,other",
        "ray_pair,other",
        "req_sse-second-2",
        "ray_sse-second-2",
    ] {
        assert!(
            !logged.contains(forbidden),
            "private/unselected values stay outside diagnostics"
        );
    }
    // Correlation checks are last, after selection, joined cleanup, context and privacy.
    let (request_id, cf_ray) = case.ids();
    let request_id = request_id.map(|value| format!("{value:?}"));
    let cf_ray = cf_ray.map(|value| format!("{value:?}"));
    assert_eq!(failed.get("request_id"), request_id.as_ref());
    assert_eq!(failed.get("cf_ray"), cf_ray.as_ref());
    println!(
        "ALAB_SSE_REPEATED_HEADERS:selection={}:case={case:?}:stage=complete",
        if deferred {
            "deferred_eof"
        } else {
            "immediate"
        }
    );
}

async fn repeated_header_cases(name: &str, deferred: bool) {
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    if let Some(child) = std::env::var_os(REPEATED_CHILD) {
        assert_eq!(child.to_str(), Some(name));
        assert_eq!(
            std::env::args().skip(1).collect::<Vec<_>>(),
            ["--exact", &exact, "--nocapture", "--test-threads=1"]
        );
        codex_alab_core::activate();
        for case in [
            RepeatedHeaders::InvalidFirstThenPrimary,
            RepeatedHeaders::FirstPrimaryStaysFirst,
            RepeatedHeaders::LaterAlias,
            RepeatedHeaders::AllInvalid,
            RepeatedHeaders::RequestOnly,
            RepeatedHeaders::RayOnly,
        ] {
            repeated_header_exchange(case, deferred).await;
        }
        std::io::stdout()
            .lock()
            .write_all(format!("{REPEATED_COMPLETE}:{name}\n").as_bytes())
            .unwrap();
        return;
    }
    assert!(!codex_alab_core::active());
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(REPEATED_CHILD, name);
    for key in ["DYLD_LIBRARY_PATH", "LD_LIBRARY_PATH"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let marker = format!("{REPEATED_COMPLETE}:{name}\n");
    crate::test_child_process::run_with_stdout_marker(command, CHILD_WAIT, Some(&marker))
        .await
        .unwrap_or_else(|failure| panic!("isolated repeated-header SSE child: {failure}"))
        .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
}

#[tokio::test(flavor = "current_thread")]
async fn native_sse_repeated_correlation_fields_survive_immediate_semantic_failure() {
    repeated_header_cases(
        "native_sse_repeated_correlation_fields_survive_immediate_semantic_failure",
        false,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn native_sse_repeated_correlation_fields_survive_deferred_eof_failure() {
    repeated_header_cases(
        "native_sse_repeated_correlation_fields_survive_deferred_eof_failure",
        true,
    )
    .await;
}

async fn buffered_cancellation_exchange(cancel_consumer: bool) {
    let frame = serde_json::json!({
        "type":"response.failed", "response":{"error":{
            "code":"unknown_failure", "message":REPEATED_PRIVATE,
            "private-body-field":"never-log-extra-body"
        }}
    });
    let progress = serde_json::json!({
        "type":"response.created", "response":{"id":REPEATED_PROGRESS_ID}
    });
    let events = format!("data: {frame}\n\ndata: {progress}\n\n").into_bytes();
    let headers = RepeatedHeaders::FirstPrimaryStaysFirst.headers();
    let mut server = if cancel_consumer {
        crate::sse_fixture::SseFixture::start_consumer_close_response(headers, events).await
    } else {
        crate::sse_fixture::SseFixture::start_scripted_response(headers, events).await
    };
    let transport = codex_api::ReqwestTransport::from_http_client(
        codex_http_client::HttpClientBuilder::new()
            .without_request_logging()
            .build_direct()
            .expect("direct buffered-cancellation loopback client"),
    );
    let client = codex_api::ResponsesClient::new(
        transport,
        codex_api::Provider {
            name: "buffered-cancellation-fixture".into(),
            base_url: format!("{}/v1", server.uri()),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            // The real peer-read and join budgets are both five seconds;
            // neither may be satisfied by this provider idle timer.
            stream_idle_timeout: Duration::from_secs(10),
        },
        Arc::new(NoAuth),
    );
    let capture = Capture::default();
    let mut stream = {
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        assert!(request_subscriber_is_current());
        let thread = tracing::info_span!(
            "buffered_cancel_thread",
            thread_id = "buffered-cancel-thread"
        );
        let _thread = thread.enter();
        let turn = tracing::info_span!("buffered_cancel_turn", turn_id = "buffered-cancel-turn");
        let _turn = turn.enter();
        tokio::time::timeout(
            REPEATED_WAIT,
            client.stream(
                serde_json::json!({"model":"gpt-buffered-cancellation", "input":[{
                    "type":"message", "role":"user", "content":"never-log-cancel-input"
                }]}),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            ),
        )
        .await
        .expect("buffered-cancellation opening deadline")
        .expect("real HTTP200 opens before the gated buffered failure")
    };
    assert!(!request_subscriber_is_current(), "caller subscriber exited");
    assert!(
        tracing::Span::current().is_disabled(),
        "caller spans exited"
    );
    let prelude = tokio::time::timeout(REPEATED_WAIT, stream.rx_event.recv())
        .await
        .expect("buffered-cancellation opening snapshot deadline");
    assert!(
        matches!(prelude, Some(Ok(codex_api::ResponseEvent::RateLimits(_)))),
        "only the expected opening snapshot precedes body release: {prelude:?}"
    );
    assert!(matches!(
        stream.rx_event.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(capture.0.lock().unwrap().len(), 1);
    server.release_failure();
    server.wait_for_event_written().await;
    let witness = tokio::time::timeout(REPEATED_WAIT, stream.rx_event.recv())
        .await
        .expect("buffered-cancellation ordered Created deadline");
    assert!(
        matches!(&witness,
            Some(Ok(codex_api::ResponseEvent::Created { response_id: Some(id) }))
                if id == REPEATED_PROGRESS_ID),
        "exact public progress past the buffered failure: {witness:?}"
    );
    assert!(server.eof_is_held());
    assert_eq!(capture.0.lock().unwrap().len(), 1);
    assert!(matches!(
        stream.rx_event.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    if cancel_consumer {
        server.wait_for_consumer_read_pending().await;
        assert!(
            server.eof_is_held(),
            "peer read is pending without server EOF"
        );
        // Drop the actual public receiver owner after consumer progress and
        // the peer's pending read; the client stays alive beyond observation.
        drop(stream);
    } else {
        server.release_eof();
        let outcome = tokio::time::timeout(REPEATED_WAIT, stream.rx_event.recv())
            .await
            .expect("positive EOF selects the buffered failure");
        let error = match outcome {
            Some(Err(error)) => error,
            other => panic!("expected the original buffered failure, got {other:?}"),
        };
        let codex_api::ApiError::Retryable {
            message,
            retry_after,
        } = &error
        else {
            panic!("EOF must retain the original Retryable, got {error:?}");
        };
        assert_eq!(message, REPEATED_PRIVATE);
        assert!(retry_after.is_none());
        let mapped = codex_api::map_api_error(error);
        assert_eq!(
            mapped.to_string(),
            format!("stream disconnected before completion: {REPEATED_PRIVATE}")
        );
        assert!(mapped.retry_delay(1).is_some());
        assert!(mapped.retry_after().is_none());
        assert!(!mapped.requires_http_fallback());
        assert!(
            tokio::time::timeout(REPEATED_WAIT, stream.rx_event.recv())
                .await
                .expect("positive EOF closes the public channel")
                .is_none()
        );
        drop(stream);
    }
    // This joins the server fixture, not the unexposed SSE consumer task.
    // Cancellation success requires an actual peer EOF/reset, never gate release.
    server
        .finish()
        .await
        .expect("bounded joined SSE fixture cleanup");
    {
        let records = capture.0.lock().unwrap();
        assert_eq!(records.len(), if cancel_consumer { 1 } else { 2 });
        let shape = &records[0];
        assert_eq!(shape["message"], "Responses request shape");
        assert!(shape["local_attempt"].parse::<u64>().unwrap() > 0);
        for record in records.iter() {
            assert_eq!(record["transport"], "\"responses_http\"");
            assert_eq!(record["thread_id"], "\"buffered-cancel-thread\"");
            assert_eq!(record["turn_id"], "\"buffered-cancel-turn\"");
            for field in [
                "http_status",
                "code",
                "error_code",
                "error_message",
                "error.message",
                "body",
                "payload",
                "headers",
            ] {
                assert!(!record.contains_key(field));
            }
        }
        let logged = format!("{records:?}");
        for private in [
            "never-log",
            REPEATED_PROGRESS_ID,
            "authorization",
            "x-unrelated",
            "unknown_failure",
            "req_sse-second-2",
            "ray_sse-second-2",
        ] {
            assert!(
                !logged.contains(private),
                "private/unselected value stays outside diagnostics"
            );
        }
        assert!(!shape.contains_key("request_id"));
        assert!(!shape.contains_key("cf_ray"));
        if !cancel_consumer {
            let failed = &records[1];
            assert_eq!(failed["message"], "Responses request failed");
            assert_eq!(failed["local_attempt"], shape["local_attempt"]);
            assert_eq!(failed["failure_kind"], "\"retryable\"");
            assert_eq!(failed["retry_advised"], "false");
            // Safe IDs are checked after error, cleanup, context and privacy.
            assert_eq!(failed["request_id"], "\"req_sse-first-1\"");
            assert_eq!(failed["cf_ray"], "\"ray_sse-first-1\"");
        }
    }
    drop(client);
}

#[tokio::test(flavor = "current_thread")]
async fn native_sse_buffered_failure_cancellation_retires_http_body() {
    const NAME: &str = "native_sse_buffered_failure_cancellation_retires_http_body";
    const CHILD: &str = "ALAB_SSE_BUFFERED_CANCELLATION_CHILD";
    let exact = format!("{}::{NAME}", module_path!().split_once("::").unwrap().1);
    let marker = format!("ALAB_SSE_BUFFERED_CANCELLATION_OK:{NAME}\n");
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(NAME));
        assert_eq!(
            std::env::args().skip(1).collect::<Vec<_>>(),
            ["--exact", &exact, "--nocapture", "--test-threads=1"]
        );
        codex_alab_core::activate();
        buffered_cancellation_exchange(false).await;
        buffered_cancellation_exchange(true).await;
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
        .unwrap_or_else(|failure| panic!("isolated buffered-cancellation SSE child: {failure}"))
        .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
}
