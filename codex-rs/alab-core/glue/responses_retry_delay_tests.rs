// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Plaintext advice values survive the real failed-event parser and API bridge.
//! These paused-clock tests do not execute Core retry scheduling or a wire retry.

use super::*;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use serde_json::json;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::Instant;

const CHILD: &str = "ALAB_FRACTIONAL_RETRY_DELAY_CHILD";
const COMPLETE: &str = "ALAB_FRACTIONAL_RETRY_DELAY_OK";

fn errors(detail: Value) -> [ApiError; 2] {
    let direct = parse_failed_response(Some(json!({"error":detail})));
    let event = serde_json::from_value(json!({
        "type":"response.failed", "response":{"error":detail}
    }))
    .unwrap();
    let dispatched = crate::sse::process_responses_event(event)
        .expect_err("actual failed event selects an error")
        .into_api_error();
    [direct, dispatched]
}

fn rate_case(
    label: &str,
    detail: Value,
    expected_raw: Option<Duration>,
    check_raw: bool,
    header_delay: Option<Duration>,
) {
    let started = Instant::now();
    let raw = check_raw.then(|| {
        let decoded: Error = serde_json::from_value(detail.clone()).unwrap();
        try_parse_retry_delay(&decoded)
    });
    let expected_advice = header_delay.or(expected_raw).filter(|delay| {
        started
            .checked_add(*delay)
            .and_then(|at| at.checked_add(Duration::from_millis(1)))
            .is_some()
    });
    let mut observed = Vec::new();
    for error in errors(detail.clone()) {
        let ApiError::RateLimitExceeded {
            message,
            retry_after,
        } = &error
        else {
            panic!("the exact rate code retains its typed error: {error:?}");
        };
        assert_eq!(message, detail["message"].as_str().unwrap());
        let advice = *retry_after;
        observed.push(advice.map(|advice| {
            advice
                .sleep_deadline()
                .at_reading(None, started.into_std())
                .saturating_duration_since(started.into_std())
        }));
        let mapped = crate::map_api_error(error);
        assert!(
            matches!(mapped.details(), CodexErrorDetails::RateLimitExceeded(actual) if actual == detail["message"].as_str().unwrap())
        );
        assert_eq!(mapped.http_status_code_value(), None);
        assert_eq!(
            mapped.retry_after(),
            advice,
            "mapping preserves the captured advice copy"
        );
        assert!(mapped.retry_delay(1).is_some());
        assert_eq!(
            mapped.to_codex_protocol_error(),
            CodexErrorInfo::RateLimitExceeded
        );
    }
    assert_eq!(Instant::now(), started);
    println!("ALAB_FRACTIONAL_RETRY_DELAY:case={label}:stage=late_value");
    if let Some(raw) = raw {
        assert_eq!(
            raw, expected_raw,
            "plaintext conversion retains its duration before timer admission"
        );
    }
    assert_eq!(
        observed,
        vec![expected_advice; 2],
        "both actual parser paths retain the expected deadline"
    );
    println!("ALAB_FRACTIONAL_RETRY_DELAY:case={label}:stage=complete");
}

fn positive(code: &str) {
    for (text, delay) in [
        ("28ms", Some(Duration::from_millis(28))),
        ("0.5ms", Some(Duration::from_micros(500))),
        ("0.25ms", Some(Duration::from_micros(250))),
        ("1.5ms", Some(Duration::from_micros(1500))),
        ("0.001ms", Some(Duration::from_micros(1))),
        ("0.000001ms", Some(Duration::from_nanos(1))),
        ("0.0000001ms", Some(Duration::ZERO)),
        ("0ms", Some(Duration::ZERO)),
        ("2.75MS", Some(Duration::from_micros(2750))),
        (
            "1000000000000000000000ms",
            Some(Duration::from_secs(1_000_000_000_000_000_000)),
        ),
    ] {
        rate_case(
            text,
            json!({"code":code,"message":format!("Please try again in {text}.")}),
            delay,
            true,
            None,
        );
    }
    rate_case(
        "invalid-header-fractional-fallback",
        json!({"code":code,"message":"Please try again in 0.5ms.","headers":{"retry-after":"invalid"}}),
        Some(Duration::from_micros(500)),
        true,
        None,
    );
    rate_case(
        "finite-duration-overflow",
        json!({"code":code,"message":"Please try again in 1000000000000000000000000000ms."}),
        None,
        true,
        None,
    );
    let overflow = "9".repeat(400);
    rate_case(
        "millisecond-overflow",
        json!({"code":code,"message":format!("Please try again in {overflow}ms.")}),
        None,
        true,
        None,
    );
}

fn controls() {
    codex_alab_core::set_enhanced(false);
    assert!(!codex_alab_core::active());
    for code in ["rate_limit_exceeded", "slow_down"] {
        for (text, delay) in [
            ("0.5ms", Duration::ZERO),
            ("1.5ms", Duration::from_millis(1)),
        ] {
            rate_case(
                "activated-vanilla",
                json!({"code":code,"message":format!("Please try again in {text}.")}),
                Some(delay),
                true,
                None,
            );
        }
    }
    codex_alab_core::set_enhanced(true);
    assert!(codex_alab_core::active());
    for code in ["rate_limit_exceeded", "slow_down"] {
        for (text, delay) in [
            ("0s", Some(Duration::ZERO)),
            ("2s", Some(Duration::from_secs(2))),
            ("1.898s", Some(Duration::from_millis(1898))),
            ("35 seconds", Some(Duration::from_secs(35))),
            ("28ms", Some(Duration::from_millis(28))),
            ("-1ms", None),
            ("NaNms", None),
            ("infms", None),
            ("1e3ms", None),
            ("1.2.3ms", None),
            ("0.5 milliseconds", None),
        ] {
            rate_case(
                text,
                json!({"code":code,"message":format!("Please try again in {text}.")}),
                delay,
                true,
                None,
            );
        }
        for header in ["7", "0"] {
            rate_case(
                "header-before-fraction",
                json!({"code":code,"message":"Please try again in 0.5ms.","headers":{"retry-after":header}}),
                None,
                false,
                Some(Duration::from_secs(header.parse().unwrap())),
            );
        }
        rate_case(
            "invalid-header-falls-back",
            json!({"code":code,"message":"Please try again in 28ms.","headers":{"retry-after":"invalid"}}),
            Some(Duration::from_millis(28)),
            true,
            None,
        );
    }
    for detail in [
        json!({"code":"invalid_prompt","message":"Please try again in 0.5ms."}),
        json!({"code":"insufficient_quota","message":"Please try again in 0.5ms."}),
    ] {
        for error in errors(detail) {
            assert!(matches!(
                error,
                ApiError::InvalidPrompt { .. } | ApiError::QuotaExceeded
            ));
            let mapped = crate::map_api_error(error);
            assert!(mapped.retry_delay(1).is_none());
            assert!(mapped.retry_after().is_none());
        }
    }
    for detail in [
        json!({"code":42,"message":"Please try again in 0.5ms."}),
        json!({"code":"rate_limit_exceeded","plan_type":42,"message":"Please try again in 0.5ms."}),
        json!({"code":"unknown_failure","message":"Please try again in 0.5ms."}),
    ] {
        for error in errors(detail) {
            assert!(matches!(
                error,
                ApiError::Stream(_)
                    | ApiError::Retryable {
                        retry_after: None,
                        ..
                    }
            ));
            let mapped = crate::map_api_error(error);
            assert!(mapped.retry_after().is_none());
        }
    }
}

fn isolated(case: &str) -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    let (_, module) = module_path!().split_once("::").unwrap();
    let name = format!("{module}::active_retry_delay_child");
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
    let marker = format!("{COMPLETE}:{case}\n");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(crate::test_child_process::run_with_stdout_marker(
            command,
            Duration::from_secs(30),
            Some(&marker),
        ))
        .unwrap_or_else(|failure| panic!("isolated retry-delay parser: {failure}"))
        .assert_test_success(&name);
    assert!(!codex_alab_core::active());
    Ok(())
}

#[test]
fn rate_limit_exceeded_preserves_fractional_millisecond_advice() -> std::io::Result<()> {
    isolated("rate-limit")
}

#[test]
fn slow_down_preserves_fractional_millisecond_advice() -> std::io::Result<()> {
    isolated("slow-down")
}

#[test]
fn fractional_millisecond_controls_preserve_precedence_and_terminal_meaning() -> std::io::Result<()>
{
    isolated("controls")
}

#[tokio::test(start_paused = true)]
async fn inactive_millisecond_advice_keeps_upstream_truncation() {
    assert!(!codex_alab_core::active());
    for code in ["rate_limit_exceeded", "slow_down"] {
        for (text, delay) in [
            ("28ms", Duration::from_millis(28)),
            ("0.5ms", Duration::ZERO),
            ("1.5ms", Duration::from_millis(1)),
        ] {
            rate_case(
                text,
                json!({"code":code,"message":format!("Please try again in {text}.")}),
                Some(delay),
                true,
                None,
            );
        }
    }
}

#[tokio::test(start_paused = true)]
async fn active_retry_delay_child() {
    let Ok(case) = std::env::var(CHILD) else {
        return;
    };
    let (_, module) = module_path!().split_once("::").unwrap();
    let name = format!("{module}::active_retry_delay_child");
    assert_eq!(
        std::env::args().skip(1).collect::<Vec<_>>(),
        ["--exact", &name, "--nocapture", "--test-threads=1"]
    );
    codex_alab_core::activate();
    match case.as_str() {
        "rate-limit" => positive("rate_limit_exceeded"),
        "slow-down" => positive("slow_down"),
        "controls" => controls(),
        other => panic!("unexpected retry-delay child: {other}"),
    }
    println!("{COMPLETE}:{case}");
}
