// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Optional metadata must not turn an explicit HTTP refusal into capacity retry.

use super::*;
use crate::api_bridge::map_api_error;
use codex_protocol::error::CodexErrorDetails;
use serde_json::json;
use std::process::Command;
use std::process::Stdio;

const CHILD: &str = "ALAB_WS_HEADER_TEST_CHILD";
const COMPLETE: &str = "ALAB_WS_HEADER_BOUNDARY_OK";

fn isolated(case: &str) -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    let (_, module) = module_path!()
        .split_once("::")
        .unwrap_or_else(|| panic!("test crate prefix"));
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args([
            "--exact",
            &format!("{module}::active_header_boundary_child"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env(CHILD, case)
        .stdin(Stdio::null());
    for key in ["DYLD_LIBRARY_PATH", "LD_LIBRARY_PATH"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let output = command.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "isolated {case} failed: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(&format!("{COMPLETE}:{case}")), "{stdout}");
    assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
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
        other => panic!("unexpected isolated case: {other}"),
    }
    println!("{COMPLETE}:{case}");
}
