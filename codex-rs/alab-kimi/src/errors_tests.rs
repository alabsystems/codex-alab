// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_api::ApiError;
use codex_api::TransportError;
use codex_http_client::RetryAfter;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;

fn provider() -> ModelProviderInfo {
    ModelProviderInfo {
        name: "Kimi Code".to_string(),
        env_key: Some("KIMI_API_KEY".to_string()),
        ..Default::default()
    }
}

fn http_error(status: u16, message: &str) -> ApiError {
    http_error_with_advice(status, message, None)
}

fn http_error_with_advice(status: u16, message: &str, retry_after: Option<RetryAfter>) -> ApiError {
    ApiError::Transport(TransportError::Http {
        status: http::StatusCode::from_u16(status).expect("valid status"),
        retry_after,
        url: None,
        headers: None,
        body: Some(serde_json::json!({"error": {"message": message}}).to_string()),
    })
}

#[test]
fn recognized_limits_retain_the_original_provider_deadline() {
    let advice = RetryAfter::from_delay(std::time::Duration::from_secs(3600))
        .expect("representable provider deadline");
    for (message, quota) in [
        ("You've reached your 5-hour usage limit", true),
        ("The engine is currently overloaded", false),
    ] {
        for error in [
            http_error_with_advice(429, message, Some(advice)),
            ApiError::RateLimitExceeded {
                message: message.into(),
                retry_after: Some(advice),
            },
            ApiError::Retryable {
                message: message.into(),
                retry_after: Some(advice),
            },
        ] {
            let error = map(true, &provider(), error);
            assert_eq!(
                matches!(error.details(), CodexErrorDetails::QuotaExceeded),
                quota
            );
            assert_eq!(error.retry_delay(1).is_none(), quota, "{message}");
            assert_eq!(error.retry_after(), Some(advice), "{message}");
        }
    }
}

fn map(active: bool, provider: &ModelProviderInfo, error: ApiError) -> CodexErr {
    crate::map_api_error(active, provider, error)
}

#[test]
fn documented_quota_walls_stay_terminal() {
    for message in [
        "You've reached your 5-hour usage limit",
        "You've reached your weekly (7-day) usage limit",
        "You've reached your monthly usage limit for this billing cycle",
        "Your credit balance is insufficient",
        "Your organization's Extra Usage balance has run out",
        "You've reached the Extra Usage limit set for your account by your organization",
    ] {
        assert!(matches!(
            map(true, &provider(), http_error(429, message)).details(),
            CodexErrorDetails::QuotaExceeded
        ));
    }
}

#[test]
fn only_documented_transient_limits_enter_capacity_recovery() {
    for message in [
        "You've reached your concurrent request limit",
        "We're receiving too many requests at the moment",
        "The engine is currently overloaded, please try again later",
    ] {
        assert!(matches!(
            map(true, &provider(), http_error(429, message)).details(),
            CodexErrorDetails::ServerOverloaded
        ));
    }
    assert!(matches!(
        map(
            true,
            &provider(),
            ApiError::RateLimitExceeded {
                message: "The engine is currently overloaded".to_string(),
                retry_after: None,
            }
        )
        .details(),
        CodexErrorDetails::ServerOverloaded
    ));
}

#[test]
fn unknown_and_non_kimi_errors_keep_generic_mapping() {
    assert!(matches!(
        map(true, &provider(), http_error(429, "unclassified")).details(),
        CodexErrorDetails::RetryLimit(_)
    ));
    assert!(matches!(
        map(true, &provider(), http_error(401, "Invalid Authentication")).details(),
        CodexErrorDetails::UnexpectedStatus(_)
    ));
    assert!(matches!(
        map(
            true,
            &ModelProviderInfo::default(),
            http_error(429, "The engine is currently overloaded")
        )
        .details(),
        CodexErrorDetails::RetryLimit(_)
    ));
    assert!(matches!(
        map(
            false,
            &provider(),
            http_error(429, "The engine is currently overloaded")
        )
        .details(),
        CodexErrorDetails::RetryLimit(_)
    ));
    assert!(matches!(
        map(true, &provider(), http_error(400, "bad request")).details(),
        CodexErrorDetails::InvalidRequest(_)
    ));
}
