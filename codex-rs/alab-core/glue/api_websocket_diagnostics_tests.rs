// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::common::ResponseEvent;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Instant;

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Default)]
struct CaptureState {
    records: Vec<BTreeMap<String, String>>,
    spans: BTreeMap<u64, BTreeMap<String, String>>,
    entered: Vec<u64>,
}

#[derive(Clone, Default)]
struct Capture {
    state: Arc<Mutex<CaptureState>>,
    next: Arc<AtomicU64>,
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        self.state.lock().unwrap().spans.insert(id, fields.0);
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
            .extend(fields.0);
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn enter(&self, id: &tracing::span::Id) {
        self.state.lock().unwrap().entered.push(id.into_u64());
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(
            self.state.lock().unwrap().entered.pop(),
            Some(id.into_u64())
        );
    }

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut state = self.state.lock().unwrap();
        for id in &state.entered {
            fields.0.extend(state.spans[id].clone());
        }
        state.records.push(fields.0);
    }
}

fn diagnostic(span: tracing::Span) -> RequestDiagnostic {
    RequestDiagnostic {
        attempt: 1,
        started: Instant::now(),
        transport: "responses_websocket",
        dispatch: tracing::dispatcher::get_default(Clone::clone),
        span,
        websocket_ids: None,
    }
}

fn wrapped() -> (RequestDiagnostic, Arc<dyn WebsocketTelemetry>) {
    let (diagnostic, observer) = wrap(Some(diagnostic(tracing::Span::none())), None);
    (diagnostic.unwrap(), observer.unwrap())
}

fn frame(headers: Value) -> String {
    json!({"type":"error", "error":{"code":"invalid_request", "headers":headers}}).to_string()
}

fn expected(request: &str, ray: &str) -> SafeIds {
    SafeIds {
        request_id: Some(request.into()),
        cf_ray: Some(ray.into()),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Request(Duration, Option<usize>, bool),
    Receive(Duration),
    Text(usize, usize, Option<String>, Duration),
}

#[derive(Default)]
struct Inner {
    calls: Mutex<Vec<Call>>,
    ids: Mutex<Option<SharedIds>>,
}

impl Inner {
    fn observe(&self, call: Call) {
        if let Some(ids) = self.ids.lock().unwrap().as_ref() {
            assert!(ids.try_lock().is_ok(), "the wrapper holds no state lock");
        }
        self.calls.lock().unwrap().push(call);
        tracing::info!("delegated telemetry callback");
    }
}

impl WebsocketTelemetry for Inner {
    fn on_ws_request(&self, duration: Duration, error: Option<&ApiError>, reused: bool) {
        self.observe(Call::Request(
            duration,
            error.map(|error| std::ptr::from_ref(error) as usize),
            reused,
        ));
    }

    fn on_ws_receive_failure(&self, duration: Duration) {
        self.observe(Call::Receive(duration));
    }

    fn on_ws_text_event(&self, text: &str, kind: Option<&str>, duration: Duration) {
        self.observe(Call::Text(
            text.as_ptr() as usize,
            text.len(),
            kind.map(str::to_owned),
            duration,
        ));
    }
}

#[test]
fn inactive_wrapper_preserves_optional_observer_identity() {
    let inner: Arc<dyn WebsocketTelemetry> = Arc::new(Inner::default());
    let (diagnostic, observer) = wrap(None, Some(inner.clone()));
    assert!(diagnostic.is_none());
    assert!(Arc::ptr_eq(&observer.unwrap(), &inner));
    let (diagnostic, observer) = wrap(None, None);
    assert!(diagnostic.is_none());
    assert!(observer.is_none());
}

#[test]
fn callbacks_preserve_arguments_and_caller_context_without_state_locks() {
    let request_capture = Capture::default();
    let callback_capture = Capture::default();
    let inner = Arc::new(Inner::default());
    let (diagnostic, observer) = tracing::subscriber::with_default(request_capture.clone(), || {
        let span = tracing::info_span!("original_request", scope = "request");
        wrap(Some(diagnostic(span)), Some(inner.clone()))
    });
    let diagnostic = diagnostic.unwrap();
    *inner.ids.lock().unwrap() = diagnostic.websocket_ids.clone();
    let observer = observer.unwrap();
    let error = Box::new(ApiError::InvalidRequest {
        message: "private callback error".into(),
    });
    let error_address = std::ptr::from_ref(error.as_ref()) as usize;
    let text = frame(json!({"x-request-id":"req_callback-1", "cf-ray":"ray_callback-1"}));
    tracing::subscriber::with_default(callback_capture.clone(), || {
        let span = tracing::info_span!("caller_callback", scope = "callback");
        let _entered = span.enter();
        observer.on_ws_request(Duration::from_millis(2), Some(error.as_ref()), true);
        observer.on_ws_receive_failure(Duration::from_millis(3));
        observer.on_ws_text_event(&text, Some("error"), Duration::from_millis(4));
        assert!(super::super::websocket_result(Some(&diagnostic), Err(*error)).is_err());
    });
    assert_eq!(
        *inner.calls.lock().unwrap(),
        [
            Call::Request(Duration::from_millis(2), Some(error_address), true),
            Call::Receive(Duration::from_millis(3)),
            Call::Text(
                text.as_ptr() as usize,
                text.len(),
                Some("error".into()),
                Duration::from_millis(4)
            ),
        ]
    );
    let callbacks = callback_capture.state.lock().unwrap();
    assert_eq!(callbacks.records.len(), 3);
    for record in &callbacks.records {
        assert_eq!(record["message"], "delegated telemetry callback");
        assert_eq!(record["scope"], "\"callback\"");
    }
    let request = request_capture.state.lock().unwrap();
    assert_eq!(request.records.len(), 1);
    assert_eq!(request.records[0]["scope"], "\"request\"");
    assert_eq!(request.records[0]["request_id"], "\"req_callback-1\"");
}

#[test]
fn header_precedence_reuses_normalization_and_tolerates_optional_containers() {
    let nested = json!({"X-OaI-ReQuEsT-Id":"req_nested-1", "Cf-RaY":"ray_nested-1",
        "authorization":"private-header", "retry-after":"7"});
    for outer in [Value::Null, json!(42), json!("invalid"), json!([])] {
        let text = json!({"type":"error", "headers":outer, "error":{"headers":nested}}).to_string();
        assert_eq!(
            event_ids(&text, Some("error")),
            expected("req_nested-1", "ray_nested-1")
        );
    }
    let text = json!({"headers":{"x-request-id":"invalid outer request", "x-oai-request-id":"req_outer-alias-1", "cf-ray":"ray_outer-1"}, "error":{"headers":nested}}).to_string();
    assert_eq!(
        event_ids(&text, Some("error")),
        expected("req_outer-alias-1", "ray_outer-1")
    );
    let text = json!({"headers":{"x-request-id":"req_outer-1", "x-oai-request-id":"req_other-1", "cf-ray":"ray_outer-1"}, "error":{"headers":nested}}).to_string();
    assert_eq!(
        event_ids(&text, Some("error")),
        expected("req_outer-1", "ray_outer-1")
    );
    let text = json!({"headers":{"x-request-id":"invalid outer request", "cf-ray":"invalid outer ray"}, "error":{"headers":nested}}).to_string();
    assert_eq!(
        event_ids(&text, Some("error")),
        expected("req_nested-1", "ray_nested-1")
    );
    for invalid in [
        "".into(),
        "private value".into(),
        "x".repeat(129),
        "private.id".into(),
        "private/id".into(),
        "private:id".into(),
    ] {
        assert_eq!(
            event_ids(
                &frame(json!({"x-request-id":invalid,"cf-ray":invalid})),
                Some("error")
            ),
            SafeIds::default()
        );
    }
    // The shared converter accepts scalar values and its case-collision order.
    let text =
        frame(json!({"X-Request-Id":"req_first-1", "x-request-id":"req_last-1", "cf-ray":1234}));
    let headers = serde_json::from_str::<Value>(&text).unwrap();
    let headers = json_headers_to_http_headers(headers["error"]["headers"].as_object().unwrap());
    assert_eq!(
        event_ids(&text, Some("error")).request_id.as_deref(),
        request_id(Some(&headers))
    );
    assert_eq!(
        event_ids(&text, Some("error")).cf_ray.as_deref(),
        Some("1234")
    );
}

#[test]
fn failed_error_arrays_preserve_outer_ids_without_positional_header_inference() {
    let outer = json!({"x-request-id":"req_outer-array-1", "cf-ray":"ray_outer-array-1"});
    let positional =
        json!({"x-request-id":"req_positional-unused", "cf-ray":"ray_positional-unused"});
    for code in ["invalid_request", "invalid_prompt"] {
        for error in [
            json!([null, code, "private message", null, null, null]),
            json!([null, code, "private message", null, null, {"headers":positional}]),
            json!([positional]),
            json!(42),
            json!("private non-object error"),
        ] {
            for kind in ["error", "response.failed"] {
                let mut event = if kind == "error" {
                    json!({"type":kind, "error":error})
                } else {
                    json!({"type":kind, "response":{"error":error}})
                };
                // A named outer container is independent of the error's JSON
                // representation. No array position is a named headers field.
                assert_eq!(
                    event_ids(&event.to_string(), Some(kind)),
                    SafeIds::default()
                );
                event["headers"] = outer.clone();
                assert_eq!(
                    event_ids(&event.to_string(), Some(kind)),
                    expected("req_outer-array-1", "ray_outer-array-1")
                );
            }
        }
    }
}

#[test]
fn optional_response_metadata_cannot_erase_valid_outer_ids() {
    let outer = json!({"x-request-id":"req_outer-response-1", "cf-ray":"ray_outer-response-1"});
    for response in [
        json!(42),
        json!("private response"),
        json!([{"headers":{"x-request-id":"req_position-unused"}}]),
    ] {
        let mut event = json!({"type":"response.failed", "response":response});
        assert_eq!(
            event_ids(&event.to_string(), Some("response.failed")),
            SafeIds::default()
        );
        event["headers"] = outer.clone();
        assert_eq!(
            event_ids(&event.to_string(), Some("response.failed")),
            expected("req_outer-response-1", "ray_outer-response-1")
        );
    }
    let duplicate = format!(
        r#"{{"type":"response.failed","headers":{outer},"response":{{"error":{{}},"error":{{}}}}}}"#
    );
    assert_eq!(
        event_ids(&duplicate, Some("response.failed")),
        expected("req_outer-response-1", "ray_outer-response-1")
    );
}

#[test]
fn every_text_frame_resets_ids_and_distinct_requests_do_not_share_them() {
    let (first, observer) = wrapped();
    let (second, other) = wrapped();
    let valid = frame(json!({"x-request-id":"req_first-1", "cf-ray":"ray_first-1"}));
    for (text, kind) in [
        ("{}", Some("error")),
        ("{", Some("error")),
        ("{", None),
        ("{}", Some("response.output_text.delta")),
        ("{}", Some("response.completed")),
        (r#"{"error":{"headers":42}}"#, Some("error")),
    ] {
        observer.on_ws_text_event(&valid, Some("error"), Duration::ZERO);
        assert_eq!(
            selected_ids(Some(&first)),
            expected("req_first-1", "ray_first-1")
        );
        observer.on_ws_text_event(text, kind, Duration::ZERO);
        assert_eq!(selected_ids(Some(&first)), SafeIds::default());
    }
    let failed = json!({"type":"response.failed", "response":{"error":{"headers":{"x-request-id":"req_failed-1", "cf-ray":"ray_failed-1"}}}}).to_string();
    observer.on_ws_text_event(&failed, Some("response.failed"), Duration::ZERO);
    assert_eq!(
        selected_ids(Some(&first)),
        expected("req_failed-1", "ray_failed-1")
    );
    assert_eq!(selected_ids(Some(&second)), SafeIds::default());
    other.on_ws_text_event(&valid, Some("error"), Duration::ZERO);
    assert_eq!(
        selected_ids(Some(&second)),
        expected("req_first-1", "ray_first-1")
    );
    assert_eq!(
        selected_ids(Some(&first)),
        expected("req_failed-1", "ray_failed-1")
    );
}

#[test]
fn only_selected_request_refusals_use_event_ids_and_transport_headers_keep_priority() {
    let capture = Capture::default();
    tracing::subscriber::with_default(capture.clone(), || {
        let (diagnostic, observer) = wrapped();
        observer.on_ws_text_event(
            &frame(json!({"x-request-id":"req_candidate-1", "cf-ray":"ray_candidate-1"})),
            Some("error"),
            Duration::ZERO,
        );
        let completed = ResponseEvent::Completed {
            response_id: "done".into(),
            token_usage: None,
            usage_metadata: None,
            reasoning_removals: None,
            interrupted: false,
            end_turn: None,
        };
        assert!(super::super::websocket_result(Some(&diagnostic), Ok(completed)).is_ok());
        assert!(capture.state.lock().unwrap().records.is_empty());
        assert!(
            super::super::websocket_result(
                Some(&diagnostic),
                Err(ApiError::Stream("private timeout".into()))
            )
            .is_err()
        );
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", "req_transport-1".parse().unwrap());
        headers.insert("cf-ray", "ray_transport-1".parse().unwrap());
        let transport = ApiError::Transport(codex_client::TransportError::Http {
            status: http::StatusCode::BAD_REQUEST,
            url: None,
            headers: Some(headers),
            body: Some("private body".into()),
            retry_after: None,
        });
        assert!(super::super::websocket_result(Some(&diagnostic), Err(transport)).is_err());
        assert!(
            super::super::websocket_result(
                Some(&diagnostic),
                Err(ApiError::InvalidRequest {
                    message: "private refusal".into()
                })
            )
            .is_err()
        );
        observer.on_ws_text_event(
            &json!({"type":"error", "error":{"code":"invalid_prompt",
                "headers":{"x-request-id":"req_prompt-1", "cf-ray":"ray_prompt-1"}}})
            .to_string(),
            Some("error"),
            Duration::ZERO,
        );
        let result = super::super::websocket_result(
            Some(&diagnostic),
            Err(ApiError::InvalidPrompt {
                message: "private prompt refusal".into(),
            }),
        );
        assert!(
            matches!(result, Err(ApiError::InvalidPrompt { message }) if message == "private prompt refusal")
        );
    });
    let state = capture.state.lock().unwrap();
    assert_eq!(state.records.len(), 4);
    assert!(!state.records[0].contains_key("request_id"));
    assert!(!state.records[0].contains_key("cf_ray"));
    assert_eq!(state.records[1]["request_id"], "\"req_transport-1\"");
    assert_eq!(state.records[1]["cf_ray"], "\"ray_transport-1\"");
    assert_eq!(state.records[2]["request_id"], "\"req_candidate-1\"");
    assert_eq!(state.records[2]["cf_ray"], "\"ray_candidate-1\"");
    assert_eq!(state.records[3]["failure_kind"], "\"invalid_prompt\"");
    assert!(!state.records[3].contains_key("http_status"));
    assert!(!state.records[3].contains_key("retry_advised"));
    assert!(!format!("{:?}", state.records).contains("private"));
    assert_eq!(
        state.records[3].get("request_id"),
        Some(&"\"req_prompt-1\"".to_string())
    );
    assert_eq!(
        state.records[3].get("cf_ray"),
        Some(&"\"ray_prompt-1\"".to_string())
    );
}

#[test]
fn poisoned_metadata_is_omitted_without_replacing_errors_or_callbacks() {
    let inner = Arc::new(Inner::default());
    let (diagnostic, observer) = wrap(Some(diagnostic(tracing::Span::none())), Some(inner.clone()));
    let diagnostic = diagnostic.unwrap();
    let ids = diagnostic.websocket_ids.as_ref().unwrap();
    let poison = std::panic::catch_unwind(|| {
        let _guard = ids.lock().unwrap();
        panic!("intentional metadata poison");
    });
    assert!(poison.is_err());
    assert_eq!(selected_ids(Some(&diagnostic)), SafeIds::default());
    observer.unwrap().on_ws_text_event(
        &frame(json!({"x-request-id":"req_unused-1"})),
        Some("error"),
        Duration::ZERO,
    );
    assert_eq!(inner.calls.lock().unwrap().len(), 1);
    assert_eq!(selected_ids(Some(&diagnostic)), SafeIds::default());
    let result = super::super::websocket_result(
        Some(&diagnostic),
        Err(ApiError::InvalidRequest {
            message: "original refusal".into(),
        }),
    );
    assert!(
        matches!(result, Err(ApiError::InvalidRequest { message }) if message == "original refusal")
    );
}

#[test]
fn advised_results_preserve_values_and_measured_advice_without_frame_ids() {
    const PRIVATE: &str = "private selected advised error";
    for advice in [
        Some(codex_http_client::RetryAfter::from_delay(Duration::from_secs(7)).unwrap()),
        Some(codex_http_client::RetryAfter::from_delay(Duration::ZERO).unwrap()),
        None,
    ] {
        for (error, kind) in [
            (
                ApiError::Retryable {
                    message: PRIVATE.into(),
                    retry_after: advice,
                },
                "retryable",
            ),
            (
                ApiError::RateLimitExceeded {
                    message: PRIVATE.into(),
                    retry_after: advice,
                },
                "rate_limit_exceeded",
            ),
            (
                ApiError::ServerOverloaded {
                    retry_after: advice,
                },
                "server_overloaded",
            ),
            (
                ApiError::PolicyCapacityUnavailable {
                    retry_after: advice,
                },
                "policy_capacity_unavailable",
            ),
        ] {
            let original_kind = std::mem::discriminant(&error);
            let capture = Capture::default();
            tracing::subscriber::with_default(capture.clone(), || {
                let (diagnostic, observer) = wrapped();
                observer.on_ws_text_event(
                    &frame(json!({"x-request-id":"req_advised-candidate", "cf-ray":"ray_advised-candidate",
                        "authorization":"private unrelated header"})),
                    Some("error"),
                    Duration::ZERO,
                );
                assert_eq!(
                    selected_ids(Some(&diagnostic)),
                    expected("req_advised-candidate", "ray_advised-candidate")
                );
                let result = super::super::websocket_result(Some(&diagnostic), Err(error));
                let Err(returned) = result else {
                    panic!("the selected error was replaced");
                };
                assert_eq!(std::mem::discriminant(&returned), original_kind);
                let returned_advice = match returned {
                    ApiError::Retryable {
                        message,
                        retry_after,
                    }
                    | ApiError::RateLimitExceeded {
                        message,
                        retry_after,
                    } => {
                        assert_eq!(message, PRIVATE);
                        retry_after
                    }
                    ApiError::ServerOverloaded { retry_after }
                    | ApiError::PolicyCapacityUnavailable { retry_after } => retry_after,
                    _ => panic!("the selected advised variant was replaced"),
                };
                assert_eq!(
                    returned_advice, advice,
                    "preserve the captured advice value, including zero"
                );
            });
            let state = capture.state.lock().unwrap();
            assert_eq!(state.records.len(), 1);
            let record = &state.records[0];
            assert_eq!(record["message"], "Responses request failed");
            assert_eq!(record["failure_kind"], format!("{kind:?}"));
            assert_eq!(record["retry_advised"], advice.is_some().to_string());
            for field in [
                "request_id",
                "cf_ray",
                "http_status",
                "headers",
                "body",
                "payload",
                "error_message",
            ] {
                assert!(!record.contains_key(field));
            }
            let logged = format!("{:?}", state.records);
            for excluded in [
                "private",
                "req_advised-candidate",
                "ray_advised-candidate",
                "authorization",
            ] {
                assert!(!logged.contains(excluded));
            }
        }
    }
}

#[test]
fn status_qualified_provider_errors_preserve_values_without_frame_ids() {
    const PRIVATE: &str = "private status-qualified provider error";
    for (error, kind) in [
        (
            ApiError::ModelNotFound {
                message: PRIVATE.into(),
                status: Some(http::StatusCode::NOT_FOUND),
                alab_empty_stream: false,
            },
            "model_not_found",
        ),
        (
            ApiError::ContentPolicyViolation {
                message: PRIVATE.into(),
                status: Some(http::StatusCode::BAD_REQUEST),
            },
            "content_policy_violation",
        ),
    ] {
        let original_kind = std::mem::discriminant(&error);
        let original_status = match &error {
            ApiError::ModelNotFound { status, .. }
            | ApiError::ContentPolicyViolation { status, .. } => *status,
            _ => unreachable!(),
        };
        let capture = Capture::default();
        tracing::subscriber::with_default(capture.clone(), || {
            let (diagnostic, observer) = wrapped();
            observer.on_ws_text_event(
                &frame(
                    json!({"x-request-id":"req_status-candidate", "cf-ray":"ray_status-candidate"}),
                ),
                Some("error"),
                Duration::ZERO,
            );
            assert_eq!(
                selected_ids(Some(&diagnostic)),
                expected("req_status-candidate", "ray_status-candidate")
            );
            let result = super::super::websocket_result(Some(&diagnostic), Err(error));
            let Err(returned) = result else {
                panic!("the selected error was replaced");
            };
            assert_eq!(std::mem::discriminant(&returned), original_kind);
            match returned {
                ApiError::ModelNotFound { message, status, .. }
                | ApiError::ContentPolicyViolation { message, status } => {
                    assert_eq!(message, PRIVATE);
                    assert_eq!(status, original_status);
                }
                _ => panic!("the selected status-qualified variant was replaced"),
            }
        });
        let state = capture.state.lock().unwrap();
        assert_eq!(state.records.len(), 1);
        let record = &state.records[0];
        assert_eq!(record["message"], "Responses request failed");
        assert_eq!(record["failure_kind"], format!("{kind:?}"));
        for field in [
            "request_id",
            "cf_ray",
            "retry_advised",
            "headers",
            "body",
            "payload",
            "error_message",
        ] {
            assert!(!record.contains_key(field));
        }
        assert!(!format!("{:?}", state.records).contains("private"));
    }
}
