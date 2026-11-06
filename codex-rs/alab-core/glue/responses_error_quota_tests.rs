// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The parent unit-test process stays upstream-inactive. Only one exact child test
//! activates ALab, so unrelated upstream parser tests never observe its global switch.

use super::parse_failed_response;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use serde_json::Value;
use serde_json::json;
use std::process::Command;
use std::process::Stdio;

const CHILD_MARKER: &str = "ALAB_STREAMED_QUOTA_TEST_CHILD";
const COMPLETE: &str = "ALAB_STREAMED_QUOTA_ACTIVE_BOUNDARY_OK";

fn child_name() -> String {
    let Some((_, module)) = module_path!().split_once("::") else {
        panic!("test crate prefix");
    };
    format!("{module}::active_parser_boundary_child")
}

fn parse(error: Value) -> CodexErr {
    crate::map_api_error(parse_failed_response(Some(json!({ "error": error }))))
}

#[test]
fn inactive_parser_keeps_the_upstream_fallback() {
    assert!(!codex_alab_core::active());
    let error = parse(json!({
        "code": "organization_usage_limit_exceeded",
        "message": "Organization usage limit exceeded."
    }));
    assert!(matches!(error.details(), CodexErrorDetails::Stream(_)));
    assert!(error.retry_delay(1).is_some());
}

#[test]
fn active_parser_boundary_runs_in_its_own_exact_test_process() -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    let name = child_name();
    let mut child = Command::new(std::env::current_exe()?);
    child
        .args(["--exact", &name, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD_MARKER, "1")
        .stdin(Stdio::null());
    // Cargo may supply these to locate the test binary's dynamic dependencies.
    // No account, provider, proxy or shell environment is needed by this pure parser.
    for key in ["DYLD_LIBRARY_PATH", "LD_LIBRARY_PATH"] {
        if let Some(value) = std::env::var_os(key) {
            child.env(key, value);
        }
    }
    let output = child.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "isolated parser test failed: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains(COMPLETE),
        "child did not exercise active cases"
    );
    assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
    assert!(!codex_alab_core::active());
    Ok(())
}

#[test]
fn active_parser_boundary_child() {
    let name = child_name();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if std::env::var(CHILD_MARKER).as_deref() != Ok("1")
        || args != ["--exact", &name, "--nocapture", "--test-threads=1"]
    {
        return;
    }
    codex_alab_core::activate();
    for advice in [None, Some("2")] {
        let mut error = json!({
            "code": "organization_usage_limit_exceeded",
            "type": "invalid_request_error",
            "message": "Organization usage limit exceeded."
        });
        if let Some(advice) = advice {
            error["headers"] = json!({ "Retry-After": advice });
        }
        let quota = parse(error);
        assert!(matches!(quota.details(), CodexErrorDetails::QuotaExceeded));
        assert_eq!(
            quota.to_codex_protocol_error(),
            CodexErrorInfo::UsageLimitExceeded
        );
        assert!(quota.retry_delay(1).is_none());
        assert!(quota.retry_after().is_none());
    }

    for code in [
        "insufficient_quota",
        "credit_balance_exhausted",
        "organization_spend_limit_exceeded",
        "project_spend_limit_exceeded",
    ] {
        assert!(matches!(
            parse(json!({ "code": code })).details(),
            CodexErrorDetails::QuotaExceeded
        ));
    }

    let overloaded = parse(json!({
        "code": "server_is_overloaded",
        "type": "insufficient_quota",
        "message": "organization_usage_limit_exceeded"
    }));
    assert!(matches!(
        overloaded.details(),
        CodexErrorDetails::ServerOverloaded
    ));
    let transient = parse(json!({
        "code": "rate_limit_exceeded",
        "type": "insufficient_quota"
    }));
    assert!(matches!(
        transient.details(),
        CodexErrorDetails::RateLimitExceeded(_)
    ));
    for error in [
        json!({ "code": "unknown_error", "type": "insufficient_quota" }),
        json!({ "type": "insufficient_quota" }),
        json!({ "message": "organization_usage_limit_exceeded" }),
        json!({ "code": "ORGANIZATION_USAGE_LIMIT_EXCEEDED" }),
        json!({ "code": "organization_usage_limit_exceeded", "plan_type": 42 }),
    ] {
        assert!(matches!(
            parse(error).details(),
            CodexErrorDetails::Stream(_)
        ));
    }
    println!("{COMPLETE}");
}
