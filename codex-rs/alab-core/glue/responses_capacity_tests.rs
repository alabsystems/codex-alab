// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Capacity events must reach retry handling without waiting for an SSE EOF.

use super::process_responses_event;
use super::spawn_response_stream;
use crate::api_bridge::map_api_error;
use crate::common::ResponseEvent;
use crate::error::ApiError;
use bytes::Bytes;
use codex_client::StreamResponse;
use codex_client::TransportError;
use codex_http_client::RetryAfter;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use futures::StreamExt;
use futures::stream;
use http::HeaderMap;
use http::StatusCode;
use serde_json::Value;
use serde_json::json;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use tokio::time::Instant;

const CHILD_MARKER: &str = "ALAB_STREAM_CAPACITY_TEST_CHILD";
const COMPLETE: &str = "ALAB_STREAM_CAPACITY_BOUNDARY_OK";

fn child_name() -> String {
    let Some((_, module)) = module_path!().split_once("::") else {
        panic!("test crate prefix");
    };
    format!("{module}::active_capacity_boundary_child")
}

fn isolated_capacity_case(case: &str) -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    let name = child_name();
    let mut child = Command::new(std::env::current_exe()?);
    child
        .args(["--exact", &name, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD_MARKER, case)
        .stdin(Stdio::null());
    for key in ["DYLD_LIBRARY_PATH", "LD_LIBRARY_PATH"] {
        if let Some(value) = std::env::var_os(key) {
            child.env(key, value);
        }
    }
    let output = child.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "isolated {case} test failed: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(&format!("{COMPLETE}:{case}")), "{stdout}");
    assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
    assert!(!codex_alab_core::active());
    Ok(())
}

async fn capacity_before_eof(frame: Value) {
    let before = Instant::now();
    let expected = if frame
        .pointer("/error/code")
        .or_else(|| frame.pointer("/response/error/code"))
        .and_then(Value::as_str)
        == Some("policy_capacity_unavailable")
    {
        CodexErrorInfo::PolicyCapacityUnavailable
    } else {
        CodexErrorInfo::ServerOverloaded
    };
    let bytes =
        stream::iter([Ok(Bytes::from(format!("data: {frame}\n\n")))]).chain(stream::pending());
    let mut response = spawn_response_stream(
        StreamResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            bytes: Box::pin(bytes),
        },
        Duration::from_secs(60),
        /*telemetry*/ None,
        /*turn_state*/ None,
    );
    let error = loop {
        match response.next().await {
            Some(Ok(ResponseEvent::RateLimits(_))) => {}
            Some(Err(error)) => break map_api_error(error),
            other => panic!("expected the provider capacity error, got {other:?}"),
        }
    };
    assert_eq!(
        error.to_codex_protocol_error(),
        expected,
        "capacity must retain its exact type instead of becoming EOF or idle timeout"
    );
    assert_eq!(
        error.retry_after().map(RetryAfter::deadline),
        Some(before + Duration::from_secs(7)),
        "the original provider deadline reaches retry handling"
    );
    assert_eq!(Instant::now(), before, "the idle deadline was not awaited");
    assert!(response.next().await.is_none());
}

#[test]
fn a_statusless_capacity_error_reaches_retry_handling_before_eof() -> std::io::Result<()> {
    isolated_capacity_case("statusless")
}

#[test]
fn a_statusless_capacity_error_keeps_top_level_advice_before_eof() -> std::io::Result<()> {
    isolated_capacity_case("top-level")
}

#[test]
fn a_failed_capacity_response_reaches_retry_handling_before_eof() -> std::io::Result<()> {
    isolated_capacity_case("failed")
}

#[test]
fn policy_capacity_sse_errors_retain_their_type_and_deadline_before_eof() -> std::io::Result<()> {
    for case in ["policy-statusless", "policy-top-level", "policy-failed"] {
        isolated_capacity_case(case)?;
    }
    Ok(())
}

#[test]
fn policy_capacity_failure_cannot_be_hidden_by_completion() -> std::io::Result<()> {
    isolated_capacity_case("known-capacity")
}

// A complete, typed provider failure owns its outcome even when the body stays
// open or later sends a contradictory completion. Unknown failures retain upstream
// buffering; this is delivery of the existing parser's semantics, not classification.
async fn failed_provider_before_eof(code: &str, later_completion: bool) {
    let before = Instant::now();
    let frame = json!({
        "type": "response.failed",
        "response": {"error": {"code": code, "message": "Provider rejected this response.",
                                "headers": {"retry-after": "900"}}}
    });
    let event = serde_json::from_value(frame.clone())
        .unwrap_or_else(|error| panic!("provider failure fixture: {error}"));
    let expected = match process_responses_event(event) {
        Err(error) => map_api_error(error.into_api_error()),
        other => panic!("expected a parsed provider failure, got {other:?}"),
    };
    assert!(!matches!(expected.details(), CodexErrorDetails::Stream(_)));
    let expected_deadline = matches!(
        code,
        "rate_limit_exceeded" | "slow_down" | "policy_capacity_unavailable"
    )
    .then_some(before + Duration::from_secs(900));
    assert_eq!(
        expected.retry_after().map(RetryAfter::deadline),
        expected_deadline
    );
    let mut data = format!("data: {frame}\n\n");
    if later_completion {
        data.push_str(
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"later\"}}\n\n",
        );
    }
    let bytes = stream::iter([Ok(Bytes::from(data))]).chain(stream::pending());
    let mut response = spawn_response_stream(
        StreamResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            bytes: Box::pin(bytes),
        },
        Duration::from_secs(60),
        /*telemetry*/ None,
        /*turn_state*/ None,
    );
    let actual = loop {
        match response.next().await {
            Some(Ok(ResponseEvent::RateLimits(_))) => {}
            Some(Err(error)) => break map_api_error(error),
            other => panic!("{code}: known failure must end the stream, got {other:?}"),
        }
    };
    assert_eq!(
        actual.to_codex_protocol_error(),
        expected.to_codex_protocol_error(),
        "{code}: a completed provider failure must not become an idle timeout"
    );
    assert_eq!(actual.to_string(), expected.to_string(), "{code}");
    assert_eq!(
        actual.retry_after().map(RetryAfter::deadline),
        expected_deadline,
        "{code}: preserve the original provider deadline"
    );
    if expected_deadline.is_none() {
        assert!(actual.retry_delay(1).is_none(), "{code} remains terminal");
    }
    assert_eq!(
        Instant::now(),
        before,
        "{code}: never wait for idle timeout"
    );
    assert!(response.next().await.is_none());
}

#[test]
fn known_quota_failures_reach_their_owner_before_eof() -> std::io::Result<()> {
    isolated_capacity_case("known-quota")
}

#[test]
fn known_rate_limits_keep_their_original_advice_before_eof() -> std::io::Result<()> {
    isolated_capacity_case("known-rate")
}

#[test]
fn known_policy_failures_cannot_be_hidden_by_completion() -> std::io::Result<()> {
    isolated_capacity_case("known-policy")
}

#[test]
fn known_context_failure_reaches_recovery_before_eof() -> std::io::Result<()> {
    isolated_capacity_case("known-context")
}

#[test]
fn known_model_failure_cannot_be_hidden_by_completion() -> std::io::Result<()> {
    isolated_capacity_case("known-model")
}

async fn buffered_failure_keeps_upstream_reading(error: Value, later_completion: bool) {
    let before = Instant::now();
    let frame = json!({"type": "response.failed", "response": {"error": error}});
    let mut data = format!("data: {frame}\n\n");
    if later_completion {
        data.push_str(
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"later\"}}\n\n",
        );
    }
    let bytes = stream::iter([Ok(Bytes::from(data))]).chain(stream::pending());
    let mut response = spawn_response_stream(
        StreamResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            bytes: Box::pin(bytes),
        },
        Duration::from_secs(60),
        None,
        None,
    );
    loop {
        match response.next().await {
            Some(Ok(ResponseEvent::RateLimits(_))) => {}
            Some(Ok(ResponseEvent::Completed { .. })) if later_completion => {
                assert_eq!(Instant::now(), before);
                break;
            }
            Some(Err(ApiError::Stream(message))) if !later_completion => {
                assert_eq!(message, "idle timeout waiting for SSE");
                assert!(Instant::now() >= before + Duration::from_secs(60));
                break;
            }
            other => panic!("upstream buffering changed for {error}: {other:?}"),
        }
    }
    assert!(response.next().await.is_none());
}

#[tokio::test(start_paused = true)]
async fn known_failures_keep_upstream_reading_when_inactive() {
    assert!(!codex_alab_core::active());
    for code in [
        "insufficient_quota",
        "usage_not_included",
        "context_length_exceeded",
        "rate_limit_exceeded",
        "model_not_found",
        "cyber_policy",
        "bio_policy",
        "misalignment_policy_violation",
        "invalid_prompt",
    ] {
        for later_completion in [false, true] {
            buffered_failure_keeps_upstream_reading(
                json!({"code": code, "headers": {"retry-after": "900"}}),
                later_completion,
            )
            .await;
        }
    }
}

#[test]
fn known_boundary_keeps_unrecognized_failures_buffered() -> std::io::Result<()> {
    isolated_capacity_case("known-fallback")
}

#[tokio::test(start_paused = true)]
async fn inactive_statusless_capacity_keeps_the_upstream_idle_timeout() {
    assert!(!codex_alab_core::active());
    let before = Instant::now();
    let frame = json!({
        "type":"error", "error":{"code":"server_is_overloaded"},
        "headers":{"retry-after":"7"}
    });
    let bytes =
        stream::iter([Ok(Bytes::from(format!("data: {frame}\n\n")))]).chain(stream::pending());
    let mut response = spawn_response_stream(
        StreamResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            bytes: Box::pin(bytes),
        },
        Duration::from_secs(60),
        None,
        None,
    );
    loop {
        match response.next().await {
            Some(Ok(ResponseEvent::RateLimits(_))) => {}
            Some(Err(ApiError::Stream(message))) => {
                assert_eq!(message, "idle timeout waiting for SSE");
                break;
            }
            other => panic!("inactive upstream reading changed: {other:?}"),
        }
    }
    assert!(Instant::now() >= before + Duration::from_secs(60));
    assert!(response.next().await.is_none());
}

#[test]
fn unknown_statusless_events_keep_their_existing_reading() {
    assert!(!codex_alab_core::active());
    unknown_statusless_reading();
}

fn unknown_statusless_reading() {
    let event = serde_json::from_value(json!({
        "type": "error",
        "error": {"code": "unknown_failure", "headers": {"retry-after": "7"}}
    }))
    .unwrap_or_else(|error| panic!("unknown-error fixture: {error}"));
    assert!(matches!(process_responses_event(event), Ok(None)));
}

#[test]
fn permanent_http_refusals_are_not_reclassified_by_a_capacity_body() {
    assert!(!codex_alab_core::active());
    permanent_http_reading();
}

fn permanent_http_reading() {
    for status in [400, 401, 403] {
        for code in ["server_is_overloaded", "policy_capacity_unavailable"] {
            let advice = RetryAfter::from_delay(Duration::from_secs(7))
                .unwrap_or_else(|| panic!("representable retry deadline"));
            let error = map_api_error(ApiError::Transport(TransportError::Http {
                status: StatusCode::from_u16(status)
                    .unwrap_or_else(|error| panic!("HTTP status fixture: {error}")),
                url: None,
                headers: None,
                body: Some(
                    json!({"error": {"code": code}, "headers":{"retry-after":"7"}}).to_string(),
                ),
                retry_after: Some(advice),
            }));
            assert!(!matches!(
                error.details(),
                CodexErrorDetails::ServerOverloaded | CodexErrorDetails::PolicyCapacityUnavailable
            ));
            if status == 400 {
                assert!(matches!(
                    error.details(),
                    CodexErrorDetails::InvalidRequest(_)
                ));
                assert!(error.retry_delay(1).is_none());
            } else {
                assert_eq!(error.http_status_code_value(), Some(status));
            }
            assert!(codex_alab_core::capacity::permanent_client_status(status));
        }
    }
}

#[test]
fn existing_terminal_statusless_errors_keep_their_precedence() {
    assert!(!codex_alab_core::active());
    terminal_statusless_reading();
}

fn terminal_statusless_reading() {
    for (code, expected) in [
        ("flex_unavailable", CodexErrorInfo::FlexUnavailable),
        ("model_not_found", CodexErrorInfo::ModelNotFound),
    ] {
        let event = serde_json::from_value(json!({
            "type": "error", "error": {"code": code, "headers": {"retry-after": "7"}}
        }))
        .unwrap_or_else(|error| panic!("terminal-error fixture: {error}"));
        let error = match process_responses_event(event) {
            Err(error) => error.into_api_error(),
            other => panic!("expected the terminal provider error, got {other:?}"),
        };
        assert_eq!(map_api_error(error).to_codex_protocol_error(), expected);
    }
}

#[test]
fn active_capacity_boundary_child() {
    let name = child_name();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let case = std::env::var(CHILD_MARKER).ok();
    if args != ["--exact", &name, "--nocapture", "--test-threads=1"]
        || !matches!(
            case.as_deref(),
            Some(
                "statusless"
                    | "failed"
                    | "top-level"
                    | "policy-statusless"
                    | "policy-top-level"
                    | "policy-failed"
                    | "known-capacity"
                    | "known-quota"
                    | "known-rate"
                    | "known-policy"
                    | "known-context"
                    | "known-model"
                    | "known-fallback"
            )
        )
    {
        return;
    }
    let case = case.unwrap_or_else(|| panic!("validated child case"));
    codex_alab_core::activate();
    if case.starts_with("known-") {
        let codes: &[&str] = match case.as_str() {
            "known-quota" => &[
                "insufficient_quota",
                "credit_balance_exhausted",
                "organization_spend_limit_exceeded",
                "project_spend_limit_exceeded",
                "organization_usage_limit_exceeded",
                "usage_not_included",
            ],
            "known-rate" => &["rate_limit_exceeded", "slow_down"],
            "known-capacity" => &["policy_capacity_unavailable"],
            "known-policy" => &[
                "cyber_policy",
                "bio_policy",
                "misalignment_policy_violation",
                "invalid_prompt",
            ],
            "known-context" => &["context_length_exceeded"],
            "known-model" => &["model_not_found"],
            "known-fallback" => &[],
            _ => unreachable!("validated child case"),
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap_or_else(|error| panic!("owned paused runtime: {error}"))
            .block_on(async {
                for code in codes {
                    let completion_first = matches!(case.as_str(), "known-policy" | "known-model");
                    for later_completion in [completion_first, !completion_first] {
                        failed_provider_before_eof(code, later_completion).await;
                    }
                }
                if case == "known-fallback" {
                    for error in [
                        json!({"code": "unknown_failure", "headers": {"retry-after": "900"}}),
                        json!({"code": "server_is_overloaded", "plan_type": 42}),
                        json!({"code": "insufficient_quota", "plan_type": 42,
                               "headers": {"retry-after": "900"}}),
                        json!({"code": 42}),
                        json!(null),
                    ] {
                        for later_completion in [false, true] {
                            buffered_failure_keeps_upstream_reading(
                                error.clone(),
                                later_completion,
                            )
                            .await;
                        }
                    }
                }
            });
        println!("{COMPLETE}:{case}");
        return;
    }
    let (shape, code) = case
        .strip_prefix("policy-")
        .map_or((case.as_str(), "server_is_overloaded"), |shape| {
            (shape, "policy_capacity_unavailable")
        });
    let frame = if shape == "statusless" {
        json!({
            "type": "error",
            "error": {"code": code, "message": "Capacity unavailable.",
                      "headers": {"retry-after": "7"}}
        })
    } else if shape == "top-level" {
        json!({
            "type": "error",
            "error": {"code": code, "message": "Capacity unavailable."},
            "headers": {"Retry-After": "7"}
        })
    } else {
        json!({
            "type": "response.failed",
            "response": {"error": {"code": code, "message": "Capacity unavailable.",
                                   "headers": {"Retry-After": "7"}}}
        })
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap_or_else(|error| panic!("owned paused runtime: {error}"))
        .block_on(capacity_before_eof(frame));
    unknown_statusless_reading();
    permanent_http_reading();
    terminal_statusless_reading();
    println!("{COMPLETE}:{case}");
}
