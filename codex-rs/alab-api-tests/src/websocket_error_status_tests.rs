// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Malformed optional error fields cannot erase an unambiguous WebSocket HTTP status.

use codex_api::ApiError;
use codex_http_client::TransportError;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const WAIT: Duration = Duration::from_secs(5);
const CHILD: &str = "ALAB_WS_ERROR_STATUS_CHILD";
const TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const PRIVATE: &str = "private-websocket-error-status-content";
const OUTER_AUTH: &str = "private-outer-authorization";
const NESTED_AUTH: &str = "private-nested-authorization";
const NESTED_ONLY: &str = "private-nested-only-header";

#[derive(Clone, Copy, Debug)]
enum StatusField {
    Primary,
    Alias,
    Equal,
}

#[derive(Clone, Copy, Debug)]
enum ErrorShape {
    CodeNumber,
    MessageNumber,
    Scalar,
    FullArray,
    BadHeaders,
}

#[derive(Clone, Copy)]
enum Expected {
    Http {
        status: u16,
        delay: u64,
        ids: (&'static str, &'static str),
    },
    Closed,
    Retryable,
}

fn positive(status: u16, field: StatusField, shape: ErrorShape) -> (String, Expected) {
    let nested = json!({"retry-after":"7", "x-request-id":"req_status-nested-1",
        "cf-ray":"ray_status-nested-1", "authorization":NESTED_AUTH,
        "x-error-json":NESTED_ONLY, "x-openai-authorization-error":NESTED_ONLY,
        "x-unrelated":NESTED_ONLY});
    let error = match shape {
        ErrorShape::CodeNumber => json!({"code":42, "message":PRIVATE, "headers":nested}),
        ErrorShape::MessageNumber => {
            json!({"code":"invalid_prompt", "message":42, "headers":nested})
        }
        ErrorShape::Scalar => json!(42),
        ErrorShape::FullArray => json!([null, "invalid_prompt", PRIVATE, null, null, null]),
        ErrorShape::BadHeaders => json!({"code":42, "message":PRIVATE, "headers":42}),
    };
    let mut outer = json!({"retry-after":"11", "authorization":OUTER_AUTH, "x-unrelated":PRIVATE});
    let ids = if matches!(shape, ErrorShape::MessageNumber) {
        outer.as_object_mut().unwrap().remove("x-unrelated");
        ("req_status-nested-1", "ray_status-nested-1")
    } else {
        outer["x-request-id"] = json!("private invalid outer primary");
        outer["x-oai-request-id"] = json!("req_status-outer-alias-1");
        outer["cf-ray"] = json!("ray_status-outer-1");
        ("req_status-outer-alias-1", "ray_status-outer-1")
    };
    let mut frame = json!({"type":"error", "headers":outer, "error":error, "private-body":PRIVATE});
    match field {
        StatusField::Primary => frame["status"] = json!(status),
        StatusField::Alias => frame["status_code"] = json!(status),
        StatusField::Equal => {
            frame["status"] = json!(status);
            frame["status_code"] = json!(status);
        }
    }
    let delay = if matches!(shape, ErrorShape::CodeNumber | ErrorShape::MessageNumber) {
        7
    } else {
        11
    };
    (frame.to_string(), Expected::Http { status, delay, ids })
}

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        cx: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        cx.span(id).unwrap().extensions_mut().insert(fields);
    }

    fn on_event(&self, event: &tracing::Event<'_>, cx: Context<'_, S>) {
        if event.metadata().target() != TARGET {
            return;
        }
        assert_eq!(*event.metadata().level(), tracing::Level::INFO);
        let mut fields = Fields::default();
        event.record(&mut fields);
        for span in cx.event_scope(event).unwrap().from_root() {
            let extensions = span.extensions();
            for (name, value) in &extensions.get::<Fields>().unwrap().0 {
                fields
                    .0
                    .entry(name.clone())
                    .or_insert_with(|| value.clone());
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

fn request() -> codex_api::ResponsesApiRequest {
    codex_api::ResponsesApiRequest {
        model: "gpt-ws-error-status".into(),
        stream: true,
        service_tier: None,
        input: vec![
            serde_json::from_value(json!({"type":"message", "role":"user",
            "content":[{"type":"input_text", "text":PRIVATE}]}))
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

async fn exchange(payload: String, expected: Expected) {
    let mut peer = tokio::time::timeout(
        WAIT,
        crate::websocket_fixture::WebsocketFixture::start_payloads(vec![payload.clone()]),
    )
    .await
    .expect("error-status fixture startup deadline");
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "websocket-error-status".into(),
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
    .expect("error-status Upgrade deadline")
    .unwrap();
    let capture = Capture::default();
    let mut stream = {
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        let thread = tracing::info_span!("status_fixture_thread", thread_id = "status-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!("status_fixture_turn", turn_id = "status-turn");
        let _turn = turn.enter();
        let stream = tokio::time::timeout(
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
        .expect("error-status request deadline")
        .unwrap();
        // Observe the fixture's first actual request, without claiming a full wire count.
        let observed = peer.observed_request().await;
        assert_eq!(observed["type"], "response.create");
        assert_eq!(observed["model"], "gpt-ws-error-status");
        assert_eq!(observed["input"][0]["content"][0]["text"], PRIVATE);
        stream
    };
    assert_eq!(
        capture.0.lock().unwrap().len(),
        usize::from(codex_alab_core::active())
    );
    let before = Instant::now();
    peer.release_failure();
    let error = match tokio::time::timeout(WAIT, stream.rx_event.recv())
        .await
        .expect("error-status selected outcome deadline")
    {
        Some(Err(error)) => error,
        other => panic!("expected selected error, got {other:?}"),
    };
    let after = Instant::now();
    assert!(
        tokio::time::timeout(WAIT, stream.rx_event.recv())
            .await
            .expect("error-status closes channel")
            .is_none()
    );
    peer.finish().await;
    let (kind, status, ids) = match expected {
        Expected::Http { status, delay, ids } => {
            let ApiError::Transport(TransportError::Http {
                status: actual,
                body,
                headers,
                retry_after,
                ..
            }) = &error
            else {
                panic!(
                    "explicit HTTP {status} must survive malformed optional error fields: {error:?}"
                );
            };
            assert_eq!(actual.as_u16(), status);
            assert_eq!(
                body.as_deref(),
                Some(payload.as_str()),
                "retain original raw body"
            );
            let advice = retry_after.expect("captured original header advice");
            assert!(advice.deadline() >= before + Duration::from_secs(delay));
            assert!(advice.deadline() <= after + Duration::from_secs(delay));
            let headers = headers.as_ref().expect("original HTTP metadata");
            assert!(
                ["x-request-id", "x-oai-request-id"]
                    .into_iter()
                    .any(|name| headers.get(name).is_some_and(|value| value == ids.0))
            );
            assert_eq!(headers.get("cf-ray").unwrap(), ids.1);
            let original: Value = serde_json::from_str(&payload).unwrap();
            for name in ["authorization", "x-unrelated", "retry-after"] {
                assert_eq!(
                    headers.get(name).and_then(|value| value.to_str().ok()),
                    original["headers"].get(name).and_then(Value::as_str),
                    "preserve only the original outer value for {name}"
                );
            }
            for name in ["x-error-json", "x-openai-authorization-error"] {
                assert!(
                    !headers.contains_key(name),
                    "nested-only {name} cannot enter HTTP metadata"
                );
            }
            let mapped = codex_api::map_api_error(error);
            if matches!(status, 401 | 403) {
                assert_eq!(mapped.http_status_code_value(), Some(status));
                assert_eq!(
                    serde_json::to_value(mapped.to_codex_protocol_error()).unwrap(),
                    json!({"http_connection_failed":{"http_status_code":status}})
                );
            }
            assert_eq!(mapped.retry_after(), Some(advice));
            ("http", Some(status), Some(ids))
        }
        Expected::Closed => {
            assert!(
                matches!(&error, ApiError::Stream(message) if message == "websocket closed by server before response.completed"),
                "ambiguous/ignored frame cannot invent HTTP identity: {error:?}"
            );
            assert!(
                codex_api::map_api_error(error)
                    .http_status_code_value()
                    .is_none()
            );
            ("stream", None, None)
        }
        Expected::Retryable => {
            let ApiError::Retryable { retry_after, .. } = &error else {
                panic!("keep existing bounded retryable route: {error:?}");
            };
            let advice = retry_after.expect("existing nested advice");
            assert!(advice.deadline() >= before + Duration::from_secs(7));
            assert!(advice.deadline() <= after + Duration::from_secs(7));
            let mapped = codex_api::map_api_error(error);
            assert!(mapped.http_status_code_value().is_none());
            assert_eq!(mapped.retry_after(), Some(advice));
            assert!(mapped.retry_delay(1).is_some());
            ("retryable", None, None)
        }
    };
    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), if codex_alab_core::active() { 2 } else { 0 });
    if codex_alab_core::active() {
        assert_eq!(records[0]["message"], "Responses request shape");
        assert_eq!(records[1]["message"], "Responses request failed");
        assert_eq!(records[0]["local_attempt"], records[1]["local_attempt"]);
        for record in records.iter() {
            assert_eq!(record["thread_id"], "\"status-thread\"");
            assert_eq!(record["turn_id"], "\"status-turn\"");
            assert_eq!(record["transport"], "\"responses_websocket\"");
        }
        let failed = &records[1];
        assert_eq!(failed["failure_kind"], format!("{kind:?}"));
        assert_eq!(
            failed.get("http_status").map(String::as_str),
            status.map(|value| value.to_string()).as_deref()
        );
        if let Some((request, ray)) = ids {
            assert_eq!(failed.get("request_id"), Some(&format!("{request:?}")));
            assert_eq!(failed.get("cf_ray"), Some(&format!("{ray:?}")));
        } else {
            assert!(!failed.contains_key("request_id"));
            assert!(!failed.contains_key("cf_ray"));
        }
    }
    let text = format!("{records:?}");
    for forbidden in [
        "private",
        "authorization",
        "x-unrelated",
        "retry-after",
        "x-error-json",
        "x-openai-authorization-error",
    ] {
        assert!(
            !text.contains(forbidden),
            "diagnostics exclude payload and arbitrary headers"
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
    let marker = format!("ALAB_WS_ERROR_STATUS_OK:{name}\n");
    crate::test_child_process::run_with_stdout_marker(
        command,
        Duration::from_secs(60),
        Some(&marker),
    )
    .await
    .unwrap_or_else(|failure| panic!("isolated error-status child: {failure}"))
    .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
    true
}

fn complete(name: &str) {
    std::io::stdout()
        .lock()
        .write_all(format!("ALAB_WS_ERROR_STATUS_OK:{name}\n").as_bytes())
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_websocket_error_fields_preserve_explicit_http_status() {
    const NAME: &str = "malformed_websocket_error_fields_preserve_explicit_http_status";
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    for status in [401, 400, 403, 503] {
        for field in [StatusField::Primary, StatusField::Alias, StatusField::Equal] {
            for shape in [
                ErrorShape::CodeNumber,
                ErrorShape::MessageNumber,
                ErrorShape::Scalar,
                ErrorShape::FullArray,
                ErrorShape::BadHeaders,
            ] {
                let (payload, expected) = positive(status, field, shape);
                exchange(payload, expected).await;
            }
        }
        println!("ALAB_WS_ERROR_STATUS_CASE:{status}");
    }
    let (mut frame, expected) = positive(401, StatusField::Primary, ErrorShape::CodeNumber);
    let mut value: Value = serde_json::from_str(&frame).unwrap();
    value["error"]["code"] = json!("websocket_connection_limit_reached");
    value["error"]["message"] = json!(42);
    frame = value.to_string();
    exchange(frame, expected).await;
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn ambiguous_websocket_error_status_keeps_existing_bounded_routes() {
    const NAME: &str = "ambiguous_websocket_error_status_keeps_existing_bounded_routes";
    if isolated(NAME).await {
        return;
    }
    codex_alab_core::activate();
    for payload in [
        r#"{"type":"error","error":{"code":42}}"#,
        r#"{"type":"error","status":200,"error":{"code":42}}"#,
        r#"{"type":"error","status":"bad","error":{"code":42}}"#,
        r#"{"type":"error","status":401,"status_code":503,"error":{"code":42}}"#,
        r#"{"type":"error","status":401,"status_code":null,"error":{"code":42}}"#,
        r#"{"type":"error","status":null,"status_code":401,"error":{"code":42}}"#,
        r#"{"type":"error","status":401,"status":401,"error":{"code":42}}"#,
        r#"{"type":"error","type":"error","status":401,"error":{"code":42}}"#,
        r#"{"type":"error","status":401,"error":{"code":42,"code":42}}"#,
        r#"{"type":"error","status":401,"error":{"code":42,"headers":{},"headers":{}}}"#,
        r#"{"type":"error","status":401,"error":42,"error":42}"#,
    ] {
        exchange(payload.into(), Expected::Closed).await;
    }
    exchange(r#"{"type":"error","status":403,"status_code":503,"headers":{"retry-after":"11"},"error":{"code":"server_is_overloaded","headers":{"retry-after":"3","retry-after":"7"}}}"#.into(), Expected::Retryable).await;
    for code in [
        "websocket_connection_limit_reached",
        "previous_response_not_found",
    ] {
        exchange(
            json!({"type":"error", "status":403, "error":{"code":code,
            "message":PRIVATE, "headers":{"retry-after":"7"}}})
            .to_string(),
            Expected::Retryable,
        )
        .await;
    }
    exchange(json!({"type":"error", "status":403, "error":["websocket_connection_limit_reached", PRIVATE, {"retry-after":"7"}]}).to_string(), Expected::Retryable).await;
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn inactive_websocket_error_status_keeps_strict_decoding() {
    const NAME: &str = "inactive_websocket_error_status_keeps_strict_decoding";
    if isolated(NAME).await {
        return;
    }
    assert!(!codex_alab_core::active());
    let (payload, _) = positive(401, StatusField::Primary, ErrorShape::CodeNumber);
    exchange(payload, Expected::Closed).await;
    complete(NAME);
}
