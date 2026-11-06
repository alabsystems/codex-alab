// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Capacity-only event handling inside codex-api's existing Responses parser.

use super::ApiError;
use super::parse_failed_response;
use crate::responses_headers::json_headers_to_http_headers;
use codex_alab_core::capacity::CapacityReason;
use codex_http_client::RetryAfter;
use serde_json::Value;
use serde_json::json;

/// Existing terminal event handling has precedence. Only the exact capacity codes
/// take the shared response.failed parser, including its validation and deadline.
/// Valid nested advice wins; otherwise the event envelope supplies its validated advice,
/// in the same order as the WebSocket wrapper. A malformed error is never promoted.
pub(super) fn from_event(error: Option<&Value>, headers: Option<&Value>) -> Option<ApiError> {
    from_event_when_active(codex_alab_core::active(), error, headers)
}

fn from_event_when_active(
    active: bool,
    error: Option<&Value>,
    headers: Option<&Value>,
) -> Option<ApiError> {
    let error = error?;
    if !active
        || error
            .get("code")
            .and_then(Value::as_str)
            .and_then(CapacityReason::from_code)
            .is_none()
    {
        return None;
    }
    let mut error = parse_failed_response(Some(json!({ "error": error })));
    let (ApiError::ServerOverloaded { retry_after }
    | ApiError::PolicyCapacityUnavailable { retry_after }) = &mut error
    else {
        return None;
    };
    if retry_after.is_none() {
        *retry_after = headers
            .and_then(Value::as_object)
            .and_then(|headers| RetryAfter::from_headers(&json_headers_to_http_headers(headers)));
    }
    Some(error)
}

/// A recognized provider failure owns its outcome without awaiting EOF. Otherwise
/// idle timeout can erase its retry deadline or terminal meaning, and a later
/// completion can hide it. Unknown/malformed failures keep upstream buffering.
pub(super) fn emit_immediately(error: &ApiError) -> bool {
    emit_when_active(codex_alab_core::active(), error)
}

fn emit_when_active(active: bool, error: &ApiError) -> bool {
    active
        && matches!(
            error,
            ApiError::ServerOverloaded { .. }
                | ApiError::PolicyCapacityUnavailable { .. }
                | ApiError::RateLimitExceeded { .. }
                | ApiError::ContextWindowExceeded
                | ApiError::QuotaExceeded
                | ApiError::UsageNotIncluded
                | ApiError::ModelNotFound { .. }
                | ApiError::InvalidPrompt { .. }
                | ApiError::CyberPolicy { .. }
                | ApiError::BioPolicy { .. }
                | ApiError::MisalignmentPolicyViolation { .. }
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_and_other_event_codes_keep_the_upstream_path() {
        let capacity = json!({ "code": "server_is_overloaded" });
        let headers = json!({ "retry-after": "7" });
        assert!(from_event_when_active(false, Some(&capacity), Some(&headers)).is_none());
        assert!(
            from_event_when_active(
                false,
                Some(&json!({"code":"policy_capacity_unavailable"})),
                Some(&headers)
            )
            .is_none()
        );
        assert!(from_event_when_active(true, None, Some(&headers)).is_none());
        for code in [
            "unknown_failure",
            "rate_limit_exceeded",
            "insufficient_quota",
            "flex_unavailable",
            "model_not_found",
            "SERVER_IS_OVERLOADED",
            "server_is_overloaded ",
        ] {
            assert!(
                from_event_when_active(true, Some(&json!({ "code": code })), Some(&headers))
                    .is_none()
            );
        }
    }

    #[test]
    fn only_active_known_provider_errors_end_the_sse_stream_early() {
        for error in [
            ApiError::ServerOverloaded { retry_after: None },
            ApiError::PolicyCapacityUnavailable { retry_after: None },
            ApiError::RateLimitExceeded {
                message: "rate limit".into(),
                retry_after: None,
            },
            ApiError::ContextWindowExceeded,
            ApiError::QuotaExceeded,
            ApiError::UsageNotIncluded,
            ApiError::ModelNotFound {
                message: "model gone".into(),
                status: None,
            },
            ApiError::InvalidPrompt {
                message: "invalid".into(),
            },
            ApiError::CyberPolicy {
                message: "refused".into(),
            },
            ApiError::BioPolicy {
                message: "refused".into(),
            },
            ApiError::MisalignmentPolicyViolation {
                message: "refused".into(),
                misalignment: None,
            },
        ] {
            assert!(!emit_when_active(false, &error));
            assert!(emit_when_active(true, &error));
        }
        for other in [
            ApiError::Stream("disconnected".into()),
            ApiError::Retryable {
                message: "unknown".into(),
                retry_after: None,
            },
            ApiError::RateLimit("untyped".into()),
            ApiError::InvalidRequest {
                message: "not a provider failure".into(),
            },
        ] {
            assert!(!emit_when_active(false, &other));
            assert!(!emit_when_active(true, &other));
        }
    }

    #[test]
    fn malformed_capacity_fields_keep_the_shared_parser_fallback() {
        let error = json!({
            "code": "server_is_overloaded", "plan_type": 42,
            "headers": {"retry-after": "7"}
        });
        assert!(
            from_event_when_active(true, Some(&error), Some(&json!({"retry-after":"7"}))).is_none()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn capacity_advice_prefers_valid_nested_then_valid_envelope_headers() {
        use std::time::Duration;
        use tokio::time::Instant;

        for (nested, envelope, seconds) in [
            (json!(null), json!({"retry-after":"7"}), Some(7)),
            (
                json!({"retry-after":"5"}),
                json!({"retry-after":"7"}),
                Some(5),
            ),
            (json!("invalid"), json!({"retry-after":"7"}), Some(7)),
            (
                json!({"retry-after":"\n5\n"}),
                json!({"retry-after":"7"}),
                Some(7),
            ),
            (
                json!({"retry-after":"\t5\t"}),
                json!({"retry-after":"7"}),
                Some(5),
            ),
            (
                json!({"Retry-After":"5","retry-after":"30"}),
                json!({"retry-after":"7"}),
                Some(30),
            ),
            (json!({"retry-after":"5"}), json!("invalid"), Some(5)),
            (json!(null), json!({"retry-after":"\n7\n"}), None),
            (json!(null), json!("invalid"), None),
        ] {
            let before = Instant::now();
            let error = json!({"code":"server_is_overloaded", "headers":nested});
            let Some(ApiError::ServerOverloaded { retry_after }) =
                from_event_when_active(true, Some(&error), Some(&envelope))
            else {
                panic!("the valid capacity code must retain its identity");
            };
            assert_eq!(
                retry_after.map(RetryAfter::deadline),
                seconds.map(|seconds| before + Duration::from_secs(seconds))
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn policy_capacity_keeps_its_type_and_nested_or_envelope_deadline() {
        use std::time::Duration;
        use tokio::time::Instant;
        for (nested, envelope, seconds) in [
            (
                json!({"retry-after":"5"}),
                json!({"retry-after":"7"}),
                Some(5),
            ),
            (json!("invalid"), json!({"retry-after":"7"}), Some(7)),
            (json!(null), json!(null), None),
        ] {
            let before = Instant::now();
            let raw = json!({"code":"policy_capacity_unavailable", "headers":nested});
            let Some(ApiError::PolicyCapacityUnavailable { retry_after }) =
                from_event_when_active(true, Some(&raw), Some(&envelope))
            else {
                panic!("retain the exact typed policy-capacity failure");
            };
            assert_eq!(
                retry_after.map(RetryAfter::deadline),
                seconds.map(|n| before + Duration::from_secs(n))
            );
        }
        let malformed = json!({"code":"policy_capacity_unavailable", "plan_type":42});
        assert!(from_event_when_active(true, Some(&malformed), None).is_none());
    }
}
