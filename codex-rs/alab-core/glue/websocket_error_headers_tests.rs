// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Optional metadata must not turn an explicit HTTP refusal into capacity retry.

use super::*;
use crate::api_bridge::map_api_error;
use codex_protocol::error::CodexErrorDetails;
use serde_json::json;
use std::process::Stdio;
use tokio::process::Command;

const CHILD: &str = "ALAB_WS_HEADER_TEST_CHILD";
const COMPLETE: &str = "ALAB_WS_HEADER_BOUNDARY_OK";

fn isolated(case: &str) -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    let (_, module) = module_path!()
        .split_once("::")
        .unwrap_or_else(|| panic!("test crate prefix"));
    let name = format!("{module}::active_header_boundary_child");
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--exact", &name, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD, case)
        .stdin(Stdio::null());
    for key in ["DYLD_LIBRARY_PATH", "LD_LIBRARY_PATH"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let expected_marker = format!("{COMPLETE}:{case}\n");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(crate::test_child_process::run_with_stdout_marker(
            command,
            Duration::from_secs(60),
            Some(&expected_marker),
        ))
        .unwrap_or_else(|failure| panic!("isolated {case}: {failure}"))
        .assert_test_success(&name);
    assert!(!codex_alab_core::active());
    Ok(())
}

// Drive the production loop with a channel-backed socket. The pump acknowledges
// the real outgoing create command and holds the incoming channel open after the
// error frame. No EOF, network, account, or copied parser routing is involved.
async fn routed_error(frame: &Value) -> ApiError {
    routed_payload(frame.to_string()).await
}

async fn routed_payload(payload: String) -> ApiError {
    let (tx_command, mut rx_command) = mpsc::channel(1);
    let (tx_message, rx_message) = mpsc::unbounded_channel();
    let pump_task = tokio::spawn(async move {
        let Some(WsCommand::Send { message, tx_result }) = rx_command.recv().await else {
            panic!("expected the outgoing request");
        };
        assert!(matches!(message, Message::Text(_)));
        assert!(tx_result.send(Ok(())).is_ok());
        assert!(tx_message.send(Ok(Message::Text(payload.into()))).is_ok());
        std::future::pending::<()>().await;
        drop(tx_message);
    });
    let mut socket = WsStream {
        max_message_bytes: None,
        tx_command,
        rx_message,
        pump_task,
        routing_affinity: None,
    };
    let (tx_event, _rx_event) = mpsc::channel(8);
    let timing = ResponsesWebsocketTimingLogContext {
        model: "header-boundary-test".into(),
        session_id: None,
        thread_id: None,
        turn_id: None,
        traceparent: None,
        previous_response_id: None,
        request_start_ms: None,
        warmup: false,
        connection_reused: false,
    };
    let before = Instant::now();
    let mut completion = None;
    let result = run_websocket_response_stream(
        &mut socket,
        tx_event,
        "{}".into(),
        Duration::from_secs(60),
        /*telemetry*/ None,
        /*turn_state*/ None,
        &timing,
        std::future::pending::<ResponseInterrupt>(),
        &mut completion,
    )
    .await;
    socket.pump_task.abort();
    let cleanup = (&mut socket.pump_task).await;
    assert!(matches!(cleanup, Err(error) if error.is_cancelled()));
    assert!(completion.is_none());
    assert_eq!(Instant::now(), before, "do not await socket EOF or timeout");
    match result {
        Err(error) => error,
        Ok(()) => panic!("the explicit error frame must fail the response"),
    }
}

async fn explicit_status_survives_optional_headers() {
    for headers in [json!("invalid"), json!([]), json!(false), json!(42)] {
        for status_key in ["status", "status_code"] {
            let mut frame = json!({
                "type":"error", "error":{"code":"server_is_overloaded",
                "headers":{"retry-after":"7"}}, "headers":headers
            });
            frame[status_key] = json!(403);
            let before = Instant::now();
            let error = routed_error(&frame).await;
            let ApiError::Transport(TransportError::Http {
                status,
                body,
                retry_after,
                ..
            }) = &error
            else {
                panic!("explicit403 must survive optional headers: {error:?}");
            };
            assert_eq!(*status, StatusCode::FORBIDDEN);
            assert_eq!(body.as_deref(), Some(frame.to_string().as_str()));
            assert_eq!(
                retry_after.map(RetryAfter::deadline),
                Some(before + Duration::from_secs(7))
            );
            let mapped = map_api_error(error);
            assert_eq!(mapped.http_status_code_value(), Some(403));
            assert!(!matches!(
                mapped.details(),
                CodexErrorDetails::ServerOverloaded
            ));
            assert!(codex_alab_core::capacity::permanent_client_status(403));
        }
    }
}

async fn consistent_status_aliases_preserve_the_http_error() {
    for status in [403, 503] {
        let frame = json!({
            "type": "error", "status": status, "status_code": status,
            "error": {"code": "server_is_overloaded", "headers": {"retry-after": "7"}},
            "headers": {"retry-after": "11"}
        });
        let before = Instant::now();
        let error = routed_error(&frame).await;
        let ApiError::Transport(TransportError::Http {
            status: actual_status,
            body,
            retry_after,
            ..
        }) = &error
        else {
            panic!("equal status aliases must retain the HTTP error: {error:?}");
        };
        assert_eq!(actual_status.as_u16(), status);
        assert_eq!(body.as_deref(), Some(frame.to_string().as_str()));
        assert_eq!(
            retry_after.map(RetryAfter::deadline),
            Some(before + Duration::from_secs(7))
        );
        let mapped = map_api_error(error);
        if status == 403 {
            assert_eq!(mapped.http_status_code_value(), Some(403));
            assert!(!matches!(
                mapped.details(),
                CodexErrorDetails::ServerOverloaded
            ));
            assert!(codex_alab_core::capacity::permanent_client_status(status));
        } else {
            assert!(matches!(
                mapped.details(),
                CodexErrorDetails::ServerOverloaded
            ));
        }
    }
}

async fn contradictory_refusal_aliases_cannot_enter_the_capacity_loop() {
    for (status, alias) in [
        (json!(403), json!(503)),
        (json!(503), json!(403)),
        (json!(400), json!(403)),
        (json!(403), json!("invalid")),
        (json!("invalid"), json!(403)),
        (json!(403), json!(null)),
    ] {
        let frame = json!({
            "type": "error", "status": status, "status_code": alias,
            "error": {"code": "server_is_overloaded", "headers": {"retry-after": "7"}},
            "headers": {"retry-after": "11"}
        });
        let before = Instant::now();
        let error = routed_error(&frame).await;
        let ApiError::Retryable {
            message,
            retry_after,
        } = &error
        else {
            panic!("ambiguous refusal must use bounded protocol retry: {error:?}");
        };
        assert_eq!(message, "invalid WebSocket error metadata");
        assert_eq!(
            retry_after.map(RetryAfter::deadline),
            Some(before + Duration::from_secs(7))
        );
        let mapped = map_api_error(error);
        assert!(matches!(mapped.details(), CodexErrorDetails::Stream(_)));
        assert_eq!(
            mapped.http_status_code_value(),
            None,
            "never invent HTTP identity"
        );
        assert_eq!(
            mapped.retry_after().map(RetryAfter::deadline),
            Some(before + Duration::from_secs(7))
        );
    }
}

async fn raw_duplicate_keys_cannot_hide_a_reported_refusal() {
    // Value-built fixtures cannot represent repeated names. Keep the original wire
    // fields so a last-value-wins fallback cannot silently select HTTP 503.
    for payload in [
        r#"{"type":"error","status":403,"status":503,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"retry-after":"7"}}}"#,
        r#"{"type":"error","status":503,"status_code":403,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"retry-after":"7"}}}"#,
        r#"{"type":"error","status":403,"status_code":403,"error":{"code":"server_is_overloaded","code":"server_is_overloaded","headers":{"retry-after":"7"}}}"#,
        r#"{"type":"error","\u0073tatus":403,"status":503,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"retry-after":"7"}}}"#,
        r#"{"type":"response.completed","type":"error","status":403,"status_code":403,"error":{"code":"server_is_overloaded","headers":{"retry-after":"7"}}}"#,
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"retry-after":"7"}},"error":{"code":"server_is_overloaded","headers":{"retry-after":"3"}}}"#,
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"retry-after":"3"}},"error":{"code":"server_is_overloaded","headers":{"retry-after":"7"}}}"#,
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"server_is_overloaded"},"headers":{"retry-after":"3"},"headers":{"retry-after":"7"}}"#,
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"retry-after":"7"},"headers":{"retry-after":"3"}}}"#,
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"retry-after":"3"},"headers":{"retry-after":"7"}}}"#,
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"retry-after":"7","retry-after":"3"}}}"#,
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"Retry-After":"7","retry-after":"3"}}}"#,
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"server_is_overloaded","headers":{"retry-after":"7"},"headers":null},"headers":{"retry-after":"11"}}"#,
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"server_is_overloaded"},"headers":{"retry-after":"7","retry-after":"3"}}"#,
    ] {
        let before = Instant::now();
        let error = routed_payload(payload.into()).await;
        let ApiError::Retryable { retry_after, .. } = &error else {
            panic!("raw duplicate keys must not invent an HTTP identity: {error:?}");
        };
        assert_eq!(
            retry_after.map(RetryAfter::deadline),
            Some(before + Duration::from_secs(7))
        );
        let mapped = map_api_error(error);
        assert!(matches!(mapped.details(), CodexErrorDetails::Stream(_)));
        assert_eq!(mapped.http_status_code_value(), None);
    }
}

async fn capacity_and_special_codes_keep_their_existing_meaning() {
    for headers in [
        json!("invalid"),
        json!([]),
        json!(false),
        json!(42),
        json!(null),
        json!({"retry-after":"11"}),
    ] {
        for status in [Some(503), None] {
            let mut frame = json!({
                "type":"error", "error":{"code":"server_is_overloaded",
                "headers":{"retry-after":"7"}}, "headers":headers
            });
            if let Some(status) = status {
                frame["status"] = json!(status);
            }
            let before = Instant::now();
            let error = map_api_error(routed_error(&frame).await);
            assert!(matches!(
                error.details(),
                CodexErrorDetails::ServerOverloaded
            ));
            assert_eq!(
                error.retry_after().map(RetryAfter::deadline),
                Some(before + Duration::from_secs(7))
            );
        }
    }
    for code in [
        WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE,
        PREVIOUS_RESPONSE_NOT_FOUND_CODE,
    ] {
        let frame = json!({"type":"error", "status":403, "error":{"code":code}, "headers":{"retry-after":"7"}});
        assert!(matches!(
            routed_error(&frame).await,
            ApiError::Retryable { .. }
        ));
        let mut duplicate = frame;
        duplicate["status_code"] = json!(403);
        assert!(matches!(
            routed_error(&duplicate).await,
            ApiError::Retryable { .. }
        ));
    }
    for headers in [json!(null), json!({}), json!({"retry-after":"11"})] {
        let frame = json!({"type":"error", "status":403, "error":{"code":"server_is_overloaded"}, "headers":headers});
        assert!(matches!(
            routed_error(&frame).await,
            ApiError::Transport(TransportError::Http {
                status: StatusCode::FORBIDDEN,
                ..
            })
        ));
    }
    let frame = json!({"type":"error", "status":403, "error":{"code":"server_is_overloaded"}});
    assert!(matches!(
        routed_error(&frame).await,
        ApiError::Transport(TransportError::Http {
            status: StatusCode::FORBIDDEN,
            ..
        })
    ));
    assert!(
        parse_wrapped_websocket_error_event("{\"type\":\"error\",\"status\":403,\"headers\":")
            .is_none()
    );
}

async fn policy_capacity_keeps_typed_advice_and_permanent_status_precedence() {
    for status in [None, Some(503), Some(403)] {
        let mut frame = json!({"type":"error", "error":{"code":"policy_capacity_unavailable", "headers":{"retry-after":"7"}}});
        if let Some(status) = status {
            frame["status"] = json!(status);
        }
        let before = Instant::now();
        let error = map_api_error(routed_error(&frame).await);
        if status == Some(403) {
            assert_eq!(error.http_status_code_value(), Some(403));
            assert!(!matches!(
                error.details(),
                CodexErrorDetails::ServerOverloaded | CodexErrorDetails::PolicyCapacityUnavailable
            ));
        } else {
            assert!(matches!(
                error.details(),
                CodexErrorDetails::PolicyCapacityUnavailable
            ));
        }
        assert_eq!(
            error.retry_after().map(RetryAfter::deadline),
            Some(before + Duration::from_secs(7))
        );
    }
    for payload in [
        r#"{"type":"error","status":403,"status_code":503,"error":{"code":"policy_capacity_unavailable","headers":{"retry-after":"7"}}}"#,
        r#"{"type":"error","status":403,"status":503,"error":{"code":"policy_capacity_unavailable","headers":{"retry-after":"7"}}}"#,
        r#"{"type":"error","status":503,"status_code":403,"status_code":503,"error":{"code":"policy_capacity_unavailable","headers":{"retry-after":"7"}}}"#,
    ] {
        let before = Instant::now();
        let error = routed_payload(payload.into()).await;
        assert!(matches!(error, ApiError::Retryable { .. }));
        let mapped = map_api_error(error);
        assert!(matches!(mapped.details(), CodexErrorDetails::Stream(_)));
        assert_eq!(mapped.http_status_code_value(), None);
        assert_eq!(
            mapped.retry_after().map(RetryAfter::deadline),
            Some(before + Duration::from_secs(7))
        );
    }
}

fn opaque_handshake_keeps_its_body_and_provider_deadline(active: bool) {
    assert_eq!(codex_alab_core::active(), active);
    for status in [StatusCode::BAD_REQUEST, StatusCode::FORBIDDEN] {
        for (body, opaque) in [
            (r#"{"detail":"Bad Request"}"#, true),
            (r#"["Bad Request"]"#, false),
            (r#"{"error":{"code":"invalid_prompt"}}"#, false),
            (r#"{"detail":"Bad Request","code":"denied"}"#, false),
        ] {
            let response = http::Response::builder()
                .status(status)
                .header("retry-after", "7")
                .header("x-request-id", "req_handshake-1")
                .body(Some(body.as_bytes().to_vec()))
                .unwrap();
            let before = Instant::now();
            let api_error = map_ws_error(
                WsError::Http(Box::new(response)),
                &Url::parse("wss://example.test/responses").unwrap(),
            );
            let after = Instant::now();
            let ApiError::Transport(TransportError::Http {
                body: preserved_body,
                headers,
                ..
            }) = &api_error
            else {
                panic!("handshake HTTP failure lost its response: {api_error:?}");
            };
            assert_eq!(preserved_body.as_deref(), Some(body));
            assert_eq!(headers.as_ref().unwrap()["x-request-id"], "req_handshake-1");
            let error = map_api_error(api_error);
            let retry = active && status == StatusCode::BAD_REQUEST && opaque;
            if status == StatusCode::BAD_REQUEST {
                assert_eq!(error.retry_delay(1).is_some(), retry);
            } else {
                assert_eq!(error.http_status_code_value(), Some(status.as_u16()));
            }
            assert_eq!(
                matches!(error.details(), CodexErrorDetails::Stream(_)),
                retry
            );
            let deadline = error.retry_after().unwrap().deadline();
            assert!(deadline >= before + Duration::from_secs(7));
            assert!(deadline <= after + Duration::from_secs(7));
        }
    }
}

#[test]
fn opaque_websocket_handshake_uses_the_shared_http_recovery_boundary() -> std::io::Result<()> {
    opaque_handshake_keeps_its_body_and_provider_deadline(false);
    isolated("opaque-handshake")
}

#[test]
fn policy_capacity_websocket_errors_keep_their_type_advice_and_refusal_boundaries()
-> std::io::Result<()> {
    isolated("policy-capacity")
}

#[test]
fn malformed_optional_headers_do_not_erase_an_explicit_refusal() -> std::io::Result<()> {
    isolated("status")
}

#[test]
fn equal_status_aliases_do_not_erase_an_explicit_refusal() -> std::io::Result<()> {
    isolated("aliases")
}

#[test]
fn conflicting_refusal_aliases_keep_the_ordinary_retry_budget() -> std::io::Result<()> {
    isolated("conflicting-aliases")
}

#[test]
fn raw_duplicate_refusals_cannot_be_replaced_by_transient_status() -> std::io::Result<()> {
    isolated("raw-duplicates")
}

#[test]
fn inactive_status_aliases_keep_upstream_duplicate_field_rejection() {
    assert!(!codex_alab_core::active());
    for status in [403, 503] {
        let frame = json!({"type":"error", "status":status, "status_code":status,
                           "error":{"code":"server_is_overloaded"}});
        assert!(parse_wrapped_websocket_error_event(&frame.to_string()).is_none());
        assert!(matches!(
            alab_error_headers::parse_for_stream(&frame.to_string()),
            Ok(None)
        ));
    }
}

#[test]
fn header_tolerance_preserves_capacity_and_special_code_routing() -> std::io::Result<()> {
    isolated("controls")
}

#[test]
fn inactive_headers_keep_the_upstream_strict_parser() {
    assert!(!codex_alab_core::active());
    for headers in [json!("invalid"), json!([]), json!(false), json!(42)] {
        let frame = json!({"type":"error", "status":403, "error":{"code":"server_is_overloaded"}, "headers":headers});
        assert!(parse_wrapped_websocket_error_event(&frame.to_string()).is_none());
    }
}

fn active_nested_correlation_header_matrix() {
    assert!(codex_alab_core::active());
    let nested = json!({
        "x-request-id":"req_nested-primary", "x-oai-request-id":"req_nested-alias",
        "cf-ray":"ray_nested", "retry-after":"7",
        "authorization":"Bearer private-nested-auth",
        "x-openai-authorization-error":"private-nested-authorization-error",
        "x-error-json":"private-nested-error-json", "x-unrelated":"private-nested-header"
    });
    for (primary, alias) in [
        ("req_outer-primary", "req_outer-alias"),
        ("invalid outer primary", "req_outer-alias"),
        ("req_outer-primary", "invalid outer alias"),
    ] {
        let outer = json!({
            "X-Request-ID":primary, "X-OAI-Request-ID":alias,
            "CF-Ray":"ray_outer", "Retry-After":"11",
            "authorization":"Bearer private-outer-auth",
            "x-openai-authorization-error":"private-outer-authorization-error",
            "x-error-json":"private-outer-error-json", "x-unrelated":"private-outer-header"
        });
        let expected = json_headers_to_http_headers(outer.as_object().unwrap());
        assert_eq!(
            alab_error_headers::http_headers(outer.as_object(), Some(&nested)),
            Some(expected),
            "a valid outer primary or alias preserves every existing outer entry"
        );
    }

    for (outer, expected) in [
        (
            json!({}),
            json!({"x-request-id":"req_nested-primary", "cf-ray":"ray_nested"}),
        ),
        (
            json!({"x-request-id":"req_outer"}),
            json!({"x-request-id":"req_outer", "cf-ray":"ray_nested"}),
        ),
        (
            json!({"cf-ray":"ray_outer"}),
            json!({"x-request-id":"req_nested-primary", "cf-ray":"ray_outer"}),
        ),
        (
            json!({"x-request-id":"invalid outer primary", "x-oai-request-id":"invalid outer alias", "cf-ray":"invalid outer ray"}),
            json!({"x-request-id":"req_nested-primary", "x-oai-request-id":"invalid outer alias", "cf-ray":"ray_nested"}),
        ),
    ] {
        assert_eq!(
            alab_error_headers::http_headers(outer.as_object(), Some(&nested)),
            Some(json_headers_to_http_headers(expected.as_object().unwrap())),
            "request and ray fallbacks are independent; only selected IDs are copied"
        );
    }

    let alias = json!({
        "X-Request-Id":"invalid nested primary",
        "X-OAI-Request-ID":"req_nested-alias", "CF-Ray":"ray_nested",
        "authorization":"Bearer private-nested-auth", "retry-after":"7"
    });
    assert_eq!(
        alab_error_headers::http_headers(None, Some(&alias)),
        Some(json_headers_to_http_headers(
            json!({"x-request-id":"req_nested-alias", "cf-ray":"ray_nested"})
                .as_object()
                .unwrap()
        )),
        "a mixed-case nested alias supplies only the canonical selected request ID"
    );

    let mixed_case = json!({
        "X-Request-ID":"req_upper", "x-request-id":"req_lower",
        "CF-Ray":"ray_upper", "cf-ray":"ray_lower",
        "authorization":"Bearer private-nested-auth"
    });
    let normalized = json_headers_to_http_headers(mixed_case.as_object().unwrap());
    let mut expected = http::HeaderMap::new();
    expected.insert("x-request-id", normalized["x-request-id"].clone());
    expected.insert("cf-ray", normalized["cf-ray"].clone());
    assert_eq!(
        alab_error_headers::http_headers(None, Some(&mixed_case)),
        Some(expected),
        "case collisions follow the shared converter's final normalized values"
    );
    let collided_primary = json!({
        "X-Request-ID":"req_earlier-primary", "x-request-id":"invalid final primary",
        "X-OAI-Request-ID":"req_nested-alias"
    });
    assert_eq!(
        json_headers_to_http_headers(collided_primary.as_object().unwrap())["x-request-id"],
        "invalid final primary"
    );
    assert_eq!(
        alab_error_headers::http_headers(None, Some(&collided_primary)),
        Some(json_headers_to_http_headers(
            json!({"x-request-id":"req_nested-alias"})
                .as_object()
                .unwrap()
        )),
        "never select an earlier valid case variant ahead of the converter's final value"
    );

    for (request, ray, expected_request, expected_ray) in [
        (json!(42), json!(true), "42", "true"),
        (json!(false), json!(17), "false", "17"),
    ] {
        let headers = json!({"x-request-id":request, "cf-ray":ray});
        let actual = alab_error_headers::http_headers(None, Some(&headers)).unwrap();
        assert_eq!(actual.len(), 2);
        assert_eq!(actual["x-request-id"], expected_request);
        assert_eq!(actual["cf-ray"], expected_ray);
    }

    let empty = json!({});
    for value in [
        json!(null),
        json!([]),
        json!({"private":"nested object"}),
        json!(""),
        json!("contains spaces"),
        json!("has.period"),
        json!("has/slash"),
        json!("has:colon"),
        json!("contains\nnewline"),
        json!("contains\rcarriage-return"),
        json!("non-ascii-é"),
        json!("x".repeat(129)),
    ] {
        let headers = json!({
            "x-request-id":value, "x-oai-request-id":value, "cf-ray":value,
            "authorization":"Bearer private-nested-auth", "retry-after":"7"
        });
        assert_eq!(alab_error_headers::http_headers(None, Some(&headers)), None);
        assert_eq!(
            alab_error_headers::http_headers(empty.as_object(), Some(&headers)),
            Some(http::HeaderMap::new()),
            "invalid nested IDs cannot replace the presence of an empty outer map"
        );
    }
    let boundary = "x".repeat(128);
    let headers = json!({"x-request-id":boundary, "cf-ray":boundary});
    let actual = alab_error_headers::http_headers(None, Some(&headers)).unwrap();
    assert_eq!(actual.len(), 2);
    assert_eq!(actual["x-request-id"], boundary);
    assert_eq!(actual["cf-ray"], boundary);

    assert_eq!(alab_error_headers::http_headers(None, None), None);
    assert_eq!(
        alab_error_headers::http_headers(empty.as_object(), None),
        Some(http::HeaderMap::new())
    );
    for malformed in [
        json!(null),
        json!([]),
        json!(false),
        json!(42),
        json!("private"),
    ] {
        assert_eq!(
            alab_error_headers::http_headers(None, Some(&malformed)),
            None
        );
        assert_eq!(
            alab_error_headers::http_headers(empty.as_object(), Some(&malformed)),
            Some(http::HeaderMap::new())
        );
    }
}

#[test]
fn nested_error_correlation_header_fallback_preserves_outer_metadata() -> std::io::Result<()> {
    isolated("correlation-headers")
}

#[test]
fn inactive_nested_error_headers_keep_outer_conversion_only() {
    assert!(!codex_alab_core::active());
    let nested = json!({"x-request-id":"req_nested", "cf-ray":"ray_nested", "retry-after":"7"});
    for outer in [
        json!(null),
        json!({}),
        json!({
            "X-Request-ID":42, "CF-Ray":true, "Retry-After":"11",
            "authorization":"Bearer private-outer-auth", "x-unrelated":"private-outer-header",
            "invalid header name":"ignored", "x-array":["ignored"], "x-null":null
        }),
    ] {
        let expected = outer.as_object().map(json_headers_to_http_headers);
        assert_eq!(
            alab_error_headers::http_headers(outer.as_object(), Some(&nested)),
            expected,
            "inactive behavior is exactly the original outer-only conversion"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn active_header_boundary_child() {
    let Ok(case) = std::env::var(CHILD) else {
        return;
    };
    codex_alab_core::activate();
    match case.as_str() {
        "status" => explicit_status_survives_optional_headers().await,
        "aliases" => consistent_status_aliases_preserve_the_http_error().await,
        "conflicting-aliases" => {
            contradictory_refusal_aliases_cannot_enter_the_capacity_loop().await
        }
        "raw-duplicates" => raw_duplicate_keys_cannot_hide_a_reported_refusal().await,
        "controls" => capacity_and_special_codes_keep_their_existing_meaning().await,
        "policy-capacity" => {
            policy_capacity_keeps_typed_advice_and_permanent_status_precedence().await
        }
        "opaque-handshake" => opaque_handshake_keeps_its_body_and_provider_deadline(true),
        "correlation-headers" => active_nested_correlation_header_matrix(),
        other => panic!("unexpected isolated case: {other}"),
    }
    println!("{COMPLETE}:{case}");
}
