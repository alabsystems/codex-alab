// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Local request shape evidence, independent of opt-in payload tracing.
//!
//! Capture counts from the request that is actually sent. Never format a request,
//! error message, tool schema, conversation item or arbitrary response header.

use crate::common::ResponseEvent;
use crate::common::ResponsesApiRequest;
use crate::common::ResponsesWsRequest;
use crate::error::ApiError;
use crate::requests::Compression;
use crate::telemetry::SseTelemetry;
use crate::telemetry::WebsocketTelemetry;
use codex_client::TransportError;
use codex_protocol::models::ResponseItem;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

static NEXT_ATTEMPT: AtomicU64 = AtomicU64::new(1);

/// Captured absence is authoritative too: only old responses without the typed
/// extension may parse their advice at this later mapping boundary.
pub(crate) fn websocket_rejection_retry_after(
    response: &tokio_tungstenite::tungstenite::handshake::client::Response,
) -> Option<codex_http_client::RetryAfter> {
    let metadata = response
        .extensions()
        .get::<codex_websocket_client::RejectionMetadata>();
    let advice = metadata.map_or_else(
        || codex_http_client::RetryAfter::from_headers(response.headers()),
        |metadata| metadata.retry_after,
    );
    if codex_alab_core::active() {
        let headers = Some(response.headers());
        tracing::info!(
            transport = "responses_websocket",
            phase = "handshake",
            http_status = response.status().as_u16(),
            body_capture = metadata.map(|metadata| metadata.outcome.as_str()),
            body_wait_ms = metadata.map(|metadata| {
                u64::try_from(metadata.capture_elapsed.as_millis()).unwrap_or(u64::MAX)
            }),
            rejection_body_bytes = metadata.and_then(|metadata| metadata.decoded_body_bytes),
            retry_advised = advice.is_some(),
            upgrade_request_id = correlation(headers, "x-request-id")
                .or_else(|| correlation(headers, "x-oai-request-id")),
            upgrade_cf_ray = correlation(headers, "cf-ray"),
            "Responses WebSocket handshake rejected"
        );
    }
    advice
}

#[derive(Default, Debug)]
struct Items {
    total: usize,
    messages: usize,
    reasoning: usize,
    tool_calls: usize,
    tool_outputs: usize,
    other: usize,
}

impl Items {
    fn add(&mut self, kind: &str) {
        self.total += 1;
        match kind {
            "message" => self.messages += 1,
            "reasoning" => self.reasoning += 1,
            "function_call" | "custom_tool_call" | "local_shell_call" => self.tool_calls += 1,
            "function_call_output" | "custom_tool_call_output" | "local_shell_call_output" => {
                self.tool_outputs += 1;
            }
            _ => self.other += 1,
        }
    }

    fn typed(input: &[ResponseItem]) -> Self {
        let mut items = Self::default();
        for item in input {
            items.add(item.item_type());
        }
        items
    }
}

/// Counts describe an observed input array. A missing field or another JSON
/// representation has no measured item counts, even when it contains no text.
struct InputShape {
    kind: &'static str,
    items: Option<Items>,
}

impl InputShape {
    fn typed(input: &[ResponseItem]) -> Self {
        Self {
            kind: "array",
            items: Some(Items::typed(input)),
        }
    }

    fn generic(input: Option<&Value>) -> Self {
        let kind = match input {
            None => "missing",
            Some(Value::Null) => "null",
            Some(Value::Array(_)) => "array",
            Some(Value::String(_)) => "string",
            Some(Value::Object(_)) => "object",
            Some(Value::Bool(_)) => "boolean",
            Some(Value::Number(_)) => "number",
        };
        let items = match input {
            Some(Value::Array(input)) => {
                let mut items = Items::default();
                for item in input {
                    items.add(
                        item.get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                    );
                }
                Some(items)
            }
            _ => None,
        };
        Self { kind, items }
    }
}

enum Transport {
    Http(Compression),
    Websocket { reused: bool, warmup: bool },
}

#[derive(Clone)]
pub(crate) struct RequestDiagnostic {
    attempt: u64,
    started: Instant,
    transport: &'static str,
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    websocket_ids: Option<websocket_diagnostics::SharedIds>,
}

impl RequestDiagnostic {
    fn record(
        transport: Transport,
        model: Option<&str>,
        incremental: bool,
        input: InputShape,
        tools: Option<usize>,
        tool_bytes: Option<usize>,
        request_bytes: usize,
    ) -> Self {
        let (transport, compression, connection_reused, warmup) = match transport {
            Transport::Http(compression) => (
                "responses_http",
                Some(match compression {
                    Compression::None => "none",
                    Compression::Zstd => "zstd",
                }),
                None,
                None,
            ),
            Transport::Websocket { reused, warmup } => {
                ("responses_websocket", None, Some(reused), Some(warmup))
            }
        };
        let diagnostic = Self {
            attempt: NEXT_ATTEMPT.fetch_add(1, Ordering::Relaxed),
            started: Instant::now(),
            transport,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            span: tracing::Span::current(),
            websocket_ids: None,
        };
        tracing::info!(
            local_attempt = diagnostic.attempt,
            transport,
            compression,
            connection_reused,
            warmup,
            model = model.filter(|model| token(model, 128)),
            mode = if incremental { "incremental" } else { "full" },
            request_bytes,
            input_kind = input.kind,
            input_items = input.items.as_ref().map(|items| items.total),
            messages = input.items.as_ref().map(|items| items.messages),
            reasoning_items = input.items.as_ref().map(|items| items.reasoning),
            tool_calls = input.items.as_ref().map(|items| items.tool_calls),
            tool_outputs = input.items.as_ref().map(|items| items.tool_outputs),
            other_items = input.items.as_ref().map(|items| items.other),
            tools,
            tool_bytes,
            "Responses request shape"
        );
        diagnostic
    }
}

pub(crate) fn http(
    request: &ResponsesApiRequest,
    bytes: usize,
    compression: Compression,
) -> Option<RequestDiagnostic> {
    if !codex_alab_core::active() {
        return None;
    }
    let tools = request
        .tools
        .as_ref()
        .map(|tools| tools.as_raw_value().get());
    Some(RequestDiagnostic::record(
        Transport::Http(compression),
        Some(&request.model),
        false,
        InputShape::typed(&request.input),
        tools.is_none().then_some(0),
        Some(tools.map_or(0, str::len)),
        bytes,
    ))
}

pub(crate) fn http_value(
    request: &Value,
    bytes: usize,
    compression: Compression,
) -> Option<RequestDiagnostic> {
    if !codex_alab_core::active() {
        return None;
    }
    Some(RequestDiagnostic::record(
        Transport::Http(compression),
        request.get("model").and_then(Value::as_str),
        request
            .get("previous_response_id")
            .is_some_and(Value::is_string),
        InputShape::generic(request.get("input")),
        match request.get("tools") {
            None => Some(0),
            Some(Value::Array(tools)) => Some(tools.len()),
            Some(_) => None,
        },
        // Present generic values have no retained encoded tool allocation.
        // Omit the unknown size instead of reporting a measured zero.
        request.get("tools").is_none().then_some(0),
        bytes,
    ))
}

pub(crate) fn websocket(
    request: &ResponsesWsRequest<'_>,
    bytes: usize,
    connection_reused: bool,
    inner: Option<Arc<dyn WebsocketTelemetry>>,
) -> (
    Option<RequestDiagnostic>,
    Option<Arc<dyn WebsocketTelemetry>>,
) {
    if !codex_alab_core::active() {
        return (None, inner);
    }
    let ResponsesWsRequest::ResponseCreate(request) = request;
    let tools = request.tools.map(serde_json::value::RawValue::get);
    let diagnostic = RequestDiagnostic::record(
        Transport::Websocket {
            reused: connection_reused,
            warmup: request.generate == Some(false),
        },
        Some(request.model),
        request.previous_response_id.is_some(),
        InputShape::typed(request.input),
        tools.is_none().then_some(0),
        Some(tools.map_or(0, str::len)),
        bytes,
    );
    websocket_diagnostics::wrap(Some(diagnostic), inner)
}

/// Observe the selected terminal result without changing completion-preserving
/// cancellation cleanup or the response consumer's original error value.
pub(crate) fn websocket_result(
    diagnostic: Option<&RequestDiagnostic>,
    result: Result<ResponseEvent, ApiError>,
) -> Result<ResponseEvent, ApiError> {
    result.inspect_err(|error| {
        if matches!(
            error,
            ApiError::InvalidRequest { .. } | ApiError::InvalidPrompt { .. }
        ) {
            let ids = websocket_diagnostics::selected_ids(diagnostic);
            failed_with_response_ids(
                diagnostic,
                error,
                ids.request_id.as_deref(),
                ids.cf_ray.as_deref(),
            );
        } else {
            failed(diagnostic, error);
        }
    })
}

#[path = "api_websocket_diagnostics.rs"]
mod websocket_diagnostics;

pub(crate) fn failed(diagnostic: Option<&RequestDiagnostic>, error: &ApiError) {
    failed_with_response_ids(diagnostic, error, None, None);
}

/// Mirror the advice-bearing types in the API bridge. Other representations do
/// not establish absence. Some includes expired advice: sampling its deadline
/// would read the shared wake observer, and logging must not do that.
fn retry_advice_presence(error: &ApiError) -> Option<bool> {
    match error {
        ApiError::Transport(TransportError::Http { retry_after, .. })
        | ApiError::Retryable { retry_after, .. }
        | ApiError::RateLimitExceeded { retry_after, .. }
        | ApiError::ServerOverloaded { retry_after }
        | ApiError::PolicyCapacityUnavailable { retry_after } => Some(retry_after.is_some()),
        _ => None,
    }
}

fn failed_with_response_ids(
    diagnostic: Option<&RequestDiagnostic>,
    error: &ApiError,
    response_request_id: Option<&str>,
    response_cf_ray: Option<&str>,
) {
    let Some(diagnostic) = diagnostic else {
        return;
    };
    // Fixed typed labels never inspect private messages. Exhaustive matches also
    // require a deliberate category when upstream adds another error variant.
    let (kind, status, headers, opaque) = match error {
        ApiError::Transport(transport) => match transport {
            TransportError::Http {
                status,
                headers,
                body,
                ..
            } => (
                "http",
                Some(status.as_u16()),
                headers.as_ref(),
                *status == http::StatusCode::BAD_REQUEST
                    && body
                        .as_deref()
                        .is_some_and(codex_alab_core::request_error::opaque_bad_request),
            ),
            TransportError::Policy(_) => ("network_policy", None, None, false),
            TransportError::Timeout => ("timeout", None, None, false),
            TransportError::Connection(_) => ("connection", None, None, false),
            TransportError::Network(_) => ("network", None, None, false),
            TransportError::Build(_) => ("request_build", None, None, false),
            TransportError::RetryLimit => ("retry_limit", None, None, false),
            TransportError::ResponseTooLarge { .. } => ("response_too_large", None, None, false),
        },
        ApiError::Api { status, .. } => ("api", Some(status.as_u16()), None, false),
        ApiError::InvalidRequest { .. } => ("invalid_request", None, None, false),
        ApiError::InvalidPrompt { .. } => ("invalid_prompt", None, None, false),
        ApiError::ContextWindowExceeded => ("context_window", None, None, false),
        ApiError::ResponseIncomplete(_) => ("incomplete", None, None, false),
        ApiError::Stream(_) => ("stream", None, None, false),
        ApiError::Retryable { .. } => ("retryable", None, None, false),
        ApiError::WebsocketMessageTooLarge => ("websocket_message_too_large", None, None, false),
        ApiError::ContentFilter => ("content_filter", None, None, false),
        ApiError::QuotaExceeded => ("quota_exceeded", None, None, false),
        ApiError::UsageNotIncluded => ("usage_not_included", None, None, false),
        ApiError::SessionBudgetExceeded => ("session_budget_exceeded", None, None, false),
        ApiError::RateLimitExceeded { .. } => ("rate_limit_exceeded", None, None, false),
        ApiError::RateLimit(_) => ("rate_limit", None, None, false),
        ApiError::AccessProgramNotEnabled { .. } => {
            ("access_program_not_enabled", None, None, false)
        }
        ApiError::InvalidAudioInput { .. } => ("invalid_audio_input", None, None, false),
        ApiError::ModelNotFound { .. } => ("model_not_found", None, None, false),
        ApiError::InferenceHookDenied { .. } => ("inference_hook_denied", None, None, false),
        ApiError::ContentPolicyViolation { .. } => ("content_policy_violation", None, None, false),
        ApiError::CyberPolicy { .. } => ("cyber_policy", None, None, false),
        ApiError::BioPolicy { .. } => ("bio_policy", None, None, false),
        ApiError::MisalignmentPolicyViolation { .. } => {
            ("misalignment_policy_violation", None, None, false)
        }
        ApiError::FlexUnavailable => ("flex_unavailable", None, None, false),
        ApiError::PolicyCapacityUnavailable { .. } => {
            ("policy_capacity_unavailable", None, None, false)
        }
        ApiError::ServerOverloaded { .. } => ("server_overloaded", None, None, false),
    };
    // Spawned transport tasks do not inherit a thread-local subscriber. Restore
    // the original request context only for this local diagnostic emission.
    tracing::dispatcher::with_default(&diagnostic.dispatch, || {
        let _entered = diagnostic.span.enter();
        tracing::info!(
            local_attempt = diagnostic.attempt,
            transport = diagnostic.transport,
            elapsed_ms =
                u64::try_from(diagnostic.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            failure_kind = kind,
            http_status = status,
            opaque_bad_request = opaque,
            retry_advised = retry_advice_presence(error),
            request_id = correlation(headers, "x-request-id")
                .or_else(|| correlation(headers, "x-oai-request-id"))
                .or(response_request_id),
            cf_ray = correlation(headers, "cf-ray").or(response_cf_ray),
            "Responses request failed"
        );
    });
}

/// A semantic error hook on the existing SSE consumer avoids proxying its channel,
/// retaining raw events, or treating a deferred error superseded by completion as a failure.
struct SseDiagnostic {
    diagnostic: RequestDiagnostic,
    inner: Option<Arc<dyn SseTelemetry>>,
    request_id: Option<String>,
    cf_ray: Option<String>,
}

pub(crate) fn sse_telemetry(
    diagnostic: Option<&RequestDiagnostic>,
    inner: Option<Arc<dyn SseTelemetry>>,
    headers: &http::HeaderMap,
) -> Option<Arc<dyn SseTelemetry>> {
    let Some(diagnostic) = diagnostic else {
        // Inactive mode returns the same optional observer with no allocation.
        return inner;
    };
    Some(Arc::new(SseDiagnostic {
        diagnostic: diagnostic.clone(),
        inner,
        request_id: correlation(Some(headers), "x-request-id")
            .or_else(|| correlation(Some(headers), "x-oai-request-id"))
            .map(str::to_owned),
        cf_ray: correlation(Some(headers), "cf-ray").map(str::to_owned),
    }))
}

impl SseTelemetry for SseDiagnostic {
    fn on_sse_poll(
        &self,
        result: &Result<
            Option<
                Result<
                    eventsource_stream::Event,
                    eventsource_stream::EventStreamError<TransportError>,
                >,
            >,
            tokio::time::error::Elapsed,
        >,
        duration: Duration,
    ) {
        if let Some(inner) = self.inner.as_ref() {
            inner.on_sse_poll(result, duration);
        }
    }

    fn on_sse_error(&self, error: &ApiError) {
        if let Some(inner) = self.inner.as_ref() {
            inner.on_sse_error(error);
        }
        failed_with_response_ids(
            Some(&self.diagnostic),
            error,
            self.request_id.as_deref(),
            self.cf_ray.as_deref(),
        );
    }
}

fn token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
        })
}

pub(super) fn correlation<'a>(headers: Option<&'a http::HeaderMap>, name: &str) -> Option<&'a str> {
    let value = headers?.get(name)?.to_str().ok()?;
    (token(value, 128)
        && value
            .bytes()
            .all(|byte| !matches!(byte, b'.' | b'/' | b':')))
    .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Events(Arc<Mutex<Vec<BTreeMap<String, String>>>>);
    #[derive(Default)]
    struct Fields(BTreeMap<String, String>);
    impl tracing::field::Visit for Fields {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.insert(field.name().to_owned(), format!("{value:?}"));
        }
    }
    impl tracing::Subscriber for Events {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut fields = Fields::default();
            event.record(&mut fields);
            self.0.lock().unwrap().push(fields.0);
        }
    }

    fn assert_input_metrics(
        shape: &BTreeMap<String, String>,
        kind: &str,
        counts: Option<[usize; 6]>,
    ) {
        assert_eq!(shape["input_kind"], format!("{kind:?}"));
        for (index, field) in [
            "input_items",
            "messages",
            "reasoning_items",
            "tool_calls",
            "tool_outputs",
            "other_items",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                shape.get(field),
                counts.map(|counts| counts[index].to_string()).as_ref(),
                "{kind}: {shape:?}"
            );
        }
        if let Some(counts) = counts {
            assert_eq!(counts[0], counts[1..].iter().sum::<usize>());
        }
    }

    #[test]
    fn diagnostics_correlate_shape_and_failure_without_content_or_arbitrary_headers() {
        // Tracing's callsite interests are shared across parallel tests. Keep
        // this recorder's process-wide default isolated from other subscribers.
        const CHILD: &str = "ALAB_REQUEST_SHAPE_DIAGNOSTIC_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let name = format!(
                "{}::diagnostics_correlate_shape_and_failure_without_content_or_arbitrary_headers",
                module_path!().split_once("::").unwrap().1
            );
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", &name, "--nocapture"])
                .env(CHILD, "1");
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test child parent runtime")
                .block_on(async {
                    crate::test_child_process::run(command, std::time::Duration::from_secs(30))
                        .await
                        .unwrap_or_else(|failure| panic!("{name}: {failure}"))
                        .assert_test_success(&name);
                });
            return;
        }
        let events = Events::default();
        tracing::subscriber::set_global_default(events.clone()).unwrap();
        let mut items = Items::default();
        for kind in [
            "message",
            "reasoning",
            "custom_tool_call",
            "custom_tool_call_output",
            "future",
        ] {
            items.add(kind);
        }
        let diagnostic = RequestDiagnostic::record(
            Transport::Websocket {
                reused: true,
                warmup: false,
            },
            Some("gpt-example"),
            true,
            InputShape {
                kind: "array",
                items: Some(items),
            },
            Some(2),
            Some(180),
            1234,
        );
        let mut headers = http::HeaderMap::new();
        headers.insert("x-request-id", "req_example-1".parse().unwrap());
        headers.insert("cf-ray", "contains private content".parse().unwrap());
        headers.insert("authorization", "Bearer never-log-this".parse().unwrap());
        failed(
            Some(&diagnostic),
            &ApiError::Transport(TransportError::Http {
                status: http::StatusCode::BAD_REQUEST,
                url: Some("https://private.example/token".into()),
                headers: Some(headers),
                body: Some(r#"{"detail":"Bad Request"}"#.into()),
                retry_after: None,
            }),
        );
        let records = events.0.lock().unwrap();
        assert_eq!(records.len(), 2);
        let shape = &records[0];
        let failure = &records[1];
        assert_eq!(shape["local_attempt"], failure["local_attempt"]);
        assert_eq!(shape["transport"], "\"responses_websocket\"");
        assert_eq!(shape["mode"], "\"incremental\"");
        assert_eq!(shape["connection_reused"], "true");
        assert_eq!(shape["warmup"], "false");
        assert_eq!(shape["request_bytes"], "1234");
        assert_eq!(shape["input_kind"], "\"array\"");
        assert_eq!(shape["input_items"], "5");
        assert_eq!(shape["messages"], "1");
        assert_eq!(shape["reasoning_items"], "1");
        assert_eq!(shape["tool_calls"], "1");
        assert_eq!(shape["tool_outputs"], "1");
        assert_eq!(shape["other_items"], "1");
        assert_eq!(shape["tools"], "2");
        assert_eq!(shape["tool_bytes"], "180");
        assert_eq!(failure["http_status"], "400");
        assert_eq!(failure["request_id"], "\"req_example-1\"");
        assert_eq!(failure["opaque_bad_request"], "true");
        let text = format!("{records:?}");
        for secret in [
            "private",
            "never-log-this",
            "authorization",
            "token",
            "detail",
            "Bad Request",
        ] {
            assert!(!text.contains(secret), "{text}");
        }
    }

    #[test]
    fn identifiers_never_log_an_unbounded_or_content_like_model() {
        for model in ["gpt-example", "org/model:v2", "gpt_example.1"] {
            assert!(token(model, 128));
        }
        for model in ["", "model\nprivate", "model with content", &"x".repeat(129)] {
            assert!(!token(model, 128));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn real_http_endpoints_emit_correlated_diagnostics_only_when_active() {
        const CHILD: &str = "ALAB_REQUEST_DIAGNOSTIC_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let name = format!(
                "{}::real_http_endpoints_emit_correlated_diagnostics_only_when_active",
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
        use crate::auth::AuthProvider;
        use crate::common::ResponsesApiTools;
        use crate::endpoint::ResponsesClient;
        use crate::endpoint::ResponsesOptions;
        use crate::provider::Provider;
        use crate::provider::RetryConfig;
        use codex_client::HttpTransport;
        use codex_client::Request;
        use codex_client::Response;
        use codex_client::StreamResponse;
        use serde_json::json;
        use std::time::Duration;

        struct RejectingTransport(http::StatusCode);
        impl HttpTransport for RejectingTransport {
            async fn execute(&self, _: Request) -> Result<Response, TransportError> {
                panic!("expected streaming transport");
            }
            async fn stream(&self, _: Request) -> Result<StreamResponse, TransportError> {
                let mut headers = http::HeaderMap::new();
                headers.insert("x-request-id", "req_endpoint-1".parse().unwrap());
                headers.insert("authorization", "Bearer never-log-this".parse().unwrap());
                Err(TransportError::Http {
                    status: self.0,
                    url: Some("https://private.example/token".into()),
                    headers: Some(headers),
                    body: Some(r#"{"detail":"Bad Request"}"#.into()),
                    retry_after: None,
                })
            }
        }
        struct NoAuth;
        impl AuthProvider for NoAuth {
            fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), crate::AuthError> {
                Ok(())
            }
        }
        let client = |status| {
            ResponsesClient::new(
                RejectingTransport(status),
                Provider {
                    name: "test".into(),
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
                    stream_idle_timeout: Duration::from_secs(1),
                },
                Arc::new(NoAuth),
            )
        };
        let input = json!({"type":"message", "role":"user", "content":[{"type":"input_text", "text":"never-log-conversation"}]});
        let request = json!({"model":"gpt-example", "input":[input.clone()], "tools":[{"name":"never-log-schema"}]});
        const TOOL_JSON: &str = r#"[{"name":"never-log-schema"}]"#;
        let typed_request = |tools| ResponsesApiRequest {
            model: "gpt-example".into(),
            stream: true,
            service_tier: None,
            input: vec![serde_json::from_value(input.clone()).unwrap()],
            tools,
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
        };
        let events = Events::default();
        let _subscriber = tracing::subscriber::set_default(events.clone());
        assert!(!codex_alab_core::active());
        assert!(
            client(http::StatusCode::BAD_REQUEST)
                .stream(
                    request.clone(),
                    http::HeaderMap::new(),
                    Compression::None,
                    None
                )
                .await
                .is_err()
        );
        assert!(events.0.lock().unwrap().iter().all(|event| {
            !event
                .get("message")
                .is_some_and(|message| message.starts_with("Responses request"))
        }));
        events.0.lock().unwrap().clear();
        codex_alab_core::activate();
        for status in [http::StatusCode::BAD_REQUEST, http::StatusCode::FORBIDDEN] {
            assert!(
                client(status)
                    .stream(
                        request.clone(),
                        http::HeaderMap::new(),
                        Compression::Zstd,
                        None
                    )
                    .await
                    .is_err()
            );
            let typed = typed_request(Some(ResponsesApiTools::from(Arc::from(
                serde_json::value::RawValue::from_string(TOOL_JSON.into()).unwrap(),
            ))));
            assert!(
                client(status)
                    .stream_request(typed, ResponsesOptions::default())
                    .await
                    .is_err()
            );
        }
        let mut absent = request.clone();
        absent.as_object_mut().unwrap().remove("tools");
        assert!(
            client(http::StatusCode::BAD_REQUEST)
                .stream(absent, http::HeaderMap::new(), Compression::None, None)
                .await
                .is_err()
        );
        assert!(
            client(http::StatusCode::BAD_REQUEST)
                .stream_request(typed_request(None), ResponsesOptions::default())
                .await
                .is_err()
        );
        let mut empty = request.clone();
        empty["tools"] = json!([]);
        assert!(
            client(http::StatusCode::BAD_REQUEST)
                .stream(empty, http::HeaderMap::new(), Compression::None, None)
                .await
                .is_err()
        );
        let mut null = request.clone();
        null["tools"] = Value::Null;
        assert!(
            client(http::StatusCode::BAD_REQUEST)
                .stream(null, http::HeaderMap::new(), Compression::None, None)
                .await
                .is_err()
        );
        let all = std::mem::take(&mut *events.0.lock().unwrap());
        let records: Vec<_> = all
            .iter()
            .filter(|event| {
                event
                    .get("message")
                    .is_some_and(|message| message.starts_with("Responses request"))
            })
            .collect();
        assert_eq!(records.len(), 16, "{records:?}");
        for (index, pair) in records.as_chunks::<2>().0.iter().enumerate() {
            let shape = pair[0];
            let failure = pair[1];
            assert_eq!(shape["local_attempt"], failure["local_attempt"]);
            assert_eq!(shape["input_kind"], "\"array\"");
            assert_eq!(shape["input_items"], "1");
            assert_eq!(shape["messages"], "1");
            assert_eq!(shape["mode"], "\"full\"");
            assert_eq!(
                shape["compression"],
                if matches!(index, 0 | 2) {
                    "\"zstd\""
                } else {
                    "\"none\""
                }
            );
            assert_eq!(failure["request_id"], "\"req_endpoint-1\"");
            assert_eq!(
                failure["opaque_bad_request"],
                if matches!(index, 2 | 3) {
                    "false"
                } else {
                    "true"
                }
            );
            let (tools, tool_bytes) = match index {
                0 | 2 => (Some(1), None),
                1 | 3 => (None, Some(TOOL_JSON.len())),
                4 | 5 => (Some(0), Some(0)),
                6 => (Some(0), None),
                7 => (None, None),
                _ => unreachable!(),
            };
            assert_eq!(
                shape.get("tools"),
                tools.map(|value| value.to_string()).as_ref()
            );
            assert_eq!(
                shape.get("tool_bytes"),
                tool_bytes.map(|value| value.to_string()).as_ref()
            );
            let text = format!("{shape:?}{failure:?}");
            for secret in [
                "never-log-conversation",
                "never-log-schema",
                "never-log-this",
                "private.example",
                "authorization",
            ] {
                assert!(!text.contains(secret), "{text}");
            }
        }
        type InputCase = (&'static str, Option<Value>, Option<[usize; 6]>);
        let input_cases: [InputCase; 8] = [
            ("missing", None, None),
            ("null", Some(Value::Null), None),
            ("string", Some(json!("never-log-string-input")), None),
            (
                "object",
                Some(json!({"never-log-object-key":"never-log-object-value"})),
                None,
            ),
            ("boolean", Some(json!(true)), None),
            ("number", Some(json!(123456789)), None),
            ("array", Some(json!([])), Some([0; 6])),
            (
                "array",
                Some(json!([
                    input.clone(),
                    {"type":"reasoning"},
                    {"type":"function_call"},
                    {"type":"function_call_output"},
                    {"type":"never-log-unknown-kind"},
                    null,
                    "never-log-scalar-input",
                    {"type":42},
                    false,
                    {}
                ])),
                Some([10, 1, 1, 1, 1, 6]),
            ),
        ];
        for (kind, input, counts) in input_cases {
            let mut request = json!({"model":"gpt-example"});
            if let Some(input) = input {
                request["input"] = input;
            }
            assert!(
                client(http::StatusCode::BAD_REQUEST)
                    .stream(request, http::HeaderMap::new(), Compression::None, None)
                    .await
                    .is_err()
            );
            let all = std::mem::take(&mut *events.0.lock().unwrap());
            let records: Vec<_> = all
                .iter()
                .filter(|event| {
                    event
                        .get("message")
                        .is_some_and(|message| message.starts_with("Responses request"))
                })
                .collect();
            assert_eq!(records.len(), 2, "{kind}: {records:?}");
            let shape = records[0];
            let failure = records[1];
            assert_input_metrics(shape, kind, counts);
            assert_eq!(shape["local_attempt"], failure["local_attempt"]);
            assert_eq!(failure["request_id"], "\"req_endpoint-1\"");
            let text = format!("{records:?}");
            for private in ["never-log", "123456789", "private.example", "authorization"] {
                assert!(!text.contains(private), "{kind}: {text}");
            }
        }
        let mut typed_empty = typed_request(None);
        typed_empty.input.clear();
        assert!(
            client(http::StatusCode::BAD_REQUEST)
                .stream_request(typed_empty, ResponsesOptions::default())
                .await
                .is_err()
        );
        let all = std::mem::take(&mut *events.0.lock().unwrap());
        let records: Vec<_> = all
            .iter()
            .filter(|event| {
                event
                    .get("message")
                    .is_some_and(|message| message.starts_with("Responses request"))
            })
            .collect();
        assert_eq!(records.len(), 2, "{records:?}");
        assert_input_metrics(records[0], "array", Some([0; 6]));
        assert_eq!(records[0]["local_attempt"], records[1]["local_attempt"]);
    }
}

#[cfg(test)]
#[path = "api_sse_diagnostics_tests.rs"]
mod sse_tests;
