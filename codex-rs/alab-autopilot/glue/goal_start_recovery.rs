// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Only local, typed admission interruption. Provider requests retain their own retry lane.

use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;

pub fn retryable_start(error: &CodexErr) -> bool {
    error.retry_after().is_none()
        && error.http_status_code_value().is_none()
        && matches!(error.details(), CodexErrorDetails::Io(error)
            if matches!(error.kind(), std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_transient_io_admission_failures_retry() {
        for kind in [
            std::io::ErrorKind::Interrupted,
            std::io::ErrorKind::WouldBlock,
            std::io::ErrorKind::TimedOut,
        ] {
            assert!(retryable_start(&CodexErr::Io(std::io::Error::from(kind))));
        }
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::InvalidData,
            std::io::ErrorKind::Other,
        ] {
            assert!(!retryable_start(&CodexErr::Io(std::io::Error::from(kind))));
        }
        for error in [
            CodexErr::ServerOverloaded,
            CodexErr::new(CodexErrorDetails::PolicyCapacityUnavailable),
            CodexErr::QuotaExceeded,
            CodexErr::UsageNotIncluded,
            CodexErr::SessionBudgetExceeded,
            CodexErr::Interrupted,
            CodexErr::InternalAgentDied,
            CodexErr::InternalServerError,
            CodexErr::new(CodexErrorDetails::ContentFilter),
            CodexErr::new(CodexErrorDetails::CyberPolicy {
                message: "refused".into(),
            }),
            CodexErr::new(CodexErrorDetails::InvalidPrompt {
                message: "invalid".into(),
            }),
            CodexErr::Json(serde_json::from_str::<serde_json::Value>("not JSON").unwrap_err()),
        ] {
            assert!(!retryable_start(&error), "{error:?}");
        }
    }

    #[test]
    fn a_capacity_shaped_forbidden_response_never_moves_to_local_retry() {
        let forbidden = CodexErr::UnexpectedStatus(codex_protocol::error::UnexpectedResponseError {
            status: "403".parse().expect("status"),
            body: "{\"error\":{\"code\":\"server_is_overloaded\",\"message\":\"Selected model is at capacity.\"}}".into(),
            user_message: None, url: None, cf_ray: None, request_id: None,
            identity_authorization_error: None, identity_error_code: None,
        });
        assert!(!retryable_start(&forbidden));
    }

    #[tokio::test]
    async fn a_join_failure_is_not_an_unbounded_admission_retry() {
        let task = tokio::spawn(std::future::pending::<()>());
        task.abort();
        let error = CodexErr::TokioJoin(task.await.expect_err("cancelled task"));
        assert!(!retryable_start(&error));
    }
}
