// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Recognized provider failure handling inside the existing Responses parser.

use super::ApiError;
use super::parse_failed_response;
use crate::responses_headers::json_headers_to_http_headers;
use codex_alab_core::capacity::CapacityReason;
use codex_http_client::RetryAfter;
use serde_json::Value;
use serde_json::json;

/// Existing terminal event handling has precedence. Exact capacity and request
/// refusal codes share the response.failed parser and its typed validation.
/// Request refusals discard retry advice. For capacity, valid nested advice wins;
/// otherwise the event envelope supplies its validated advice, as in WebSockets.
/// A malformed error is never promoted.
pub(super) fn from_event(error: Option<&Value>, headers: Option<&Value>) -> Option<ApiError> {
    from_event_when_active(codex_alab_core::active(), error, headers)
}

fn from_event_when_active(
    active: bool,
    error: Option<&Value>,
    headers: Option<&Value>,
) -> Option<ApiError> {
    if !active {
        return None;
    }
    let error = error?.as_object()?;
    let code = error.get("code")?.as_str()?;
    // Ignored codes must not enter the parser, which can capture retry clock state.
    if !matches!(code, "invalid_request" | "invalid_prompt")
        && CapacityReason::from_code(code).is_none()
    {
        return None;
    }
    let mut error = parse_failed_response(Some(json!({ "error": error })));
    if request_refusal(&error) {
        return Some(error);
    }
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

/// Keep already captured advice when ordinary stream failure is selected later.
/// Deferred failures may still be superseded by completion; no pending message
/// or error category replaces the selected terminal cause.
/// Typed rate/capacity advice can be pending after a live mode change.
pub(super) fn terminal_with_buffered_advice(
    selected: ApiError,
    pending: Option<&ApiError>,
) -> ApiError {
    if !codex_alab_core::active() {
        return selected;
    }
    match (selected, pending) {
        (
            ApiError::Stream(message),
            Some(
                ApiError::Retryable {
                    retry_after: Some(retry_after),
                    ..
                }
                | ApiError::RateLimitExceeded {
                    retry_after: Some(retry_after),
                    ..
                }
                | ApiError::ServerOverloaded {
                    retry_after: Some(retry_after),
                }
                | ApiError::PolicyCapacityUnavailable {
                    retry_after: Some(retry_after),
                },
            ),
        ) => ApiError::Retryable {
            message,
            retry_after: Some(*retry_after),
        },
        (selected, _) => selected,
    }
}

/// A recognized provider failure owns its outcome without awaiting EOF. Otherwise
/// idle timeout can erase its retry deadline or terminal meaning, and a later
/// completion can hide it. Unknown/malformed failures keep upstream buffering.
pub(super) fn emit_immediately(error: &ApiError) -> bool {
    emit_when_active(codex_alab_core::active(), error)
}

fn request_refusal(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::InvalidRequest { .. } | ApiError::InvalidPrompt { .. }
    )
}

fn emit_when_active(active: bool, error: &ApiError) -> bool {
    active
        && (request_refusal(error)
            || matches!(
                error,
                ApiError::ServerOverloaded { .. }
                    | ApiError::PolicyCapacityUnavailable { .. }
                    | ApiError::RateLimitExceeded { .. }
                    | ApiError::ContextWindowExceeded
                    | ApiError::QuotaExceeded
                    | ApiError::UsageNotIncluded
                    | ApiError::ModelNotFound { .. }
                    | ApiError::InvalidAudioInput { .. }
                    | ApiError::CyberPolicy { .. }
                    | ApiError::InferenceHookDenied { .. }
                    | ApiError::BioPolicy { .. }
                    | ApiError::MisalignmentPolicyViolation { .. }
            ))
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
            ApiError::InvalidRequest {
                message: "invalid request".into(),
            },
            ApiError::InvalidAudioInput {
                message: "invalid recording".into(),
                per_recording: true,
            },
            ApiError::InvalidAudioInput {
                message: "invalid audio payload".into(),
                per_recording: false,
            },
            ApiError::InferenceHookDenied {
                message: "denied".into(),
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
