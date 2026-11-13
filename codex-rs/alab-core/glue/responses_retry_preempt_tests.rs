// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Controls for steering's retry-outcome boundary; no ALab process switch is changed.

use super::*;
use codex_async_utils::OrCancelExt;
use codex_http_client::RetryAfter;
use codex_protocol::error::ConnectionFailedError;
use codex_protocol::error::UnexpectedResponseError;
use http::StatusCode;
use std::sync::Arc;
use std::time::Duration;

fn status(code: u16) -> CodexErr {
    CodexErr::UnexpectedStatus(UnexpectedResponseError {
        status: StatusCode::from_u16(code).expect("status"),
        body: "provider refused".into(),
        user_message: None,
        url: None,
        cf_ray: None,
        request_id: None,
        identity_authorization_error: None,
        identity_error_code: None,
    })
}

#[test]
fn refusals_and_capacity_keep_their_outcome_without_changing_inactive_behavior() {
    for error in [
        CodexErr::new(CodexErrorDetails::InferenceHookDenied {
            message: "workspace denied".into(),
        }),
        CodexErr::new(CodexErrorDetails::CyberPolicy {
            message: "refused".into(),
        }),
        CodexErr::new(CodexErrorDetails::InvalidPrompt {
            message: "invalid".into(),
        }),
        CodexErr::QuotaExceeded,
        CodexErr::UsageNotIncluded,
        CodexErr::ServerOverloaded,
        CodexErr::new(CodexErrorDetails::PolicyCapacityUnavailable),
        status(403),
    ] {
        for unbounded in [false, true] {
            assert!(protects_retry(true, &error, 0, 4, unbounded), "{error:?}");
            assert!(!protects_retry(false, &error, 4, 4, unbounded), "{error:?}");
        }
    }
}

#[test]
fn policy_capacity_uses_the_capacity_lane_without_turning_refusals_into_retries() {
    for reason in [
        CapacityReason::ServerOverloaded,
        CapacityReason::PolicyCapacityUnavailable,
    ] {
        for advice in [None, RetryAfter::from_delay(Duration::from_secs(7))] {
            let details = match reason {
                CapacityReason::ServerOverloaded => CodexErrorDetails::ServerOverloaded,
                CapacityReason::PolicyCapacityUnavailable => {
                    CodexErrorDetails::PolicyCapacityUnavailable
                }
            };
            let mut error = CodexErr::new(details);
            if let Some(advice) = advice {
                error = error.with_retry_after(advice);
            }
            assert!(should_retry_when_active(true, &error));
            assert!(!should_retry_when_active(false, &error));
            assert!(protects_retry(true, &error, 0, 4, false));
        }
    }
    for error in [
        status(403),
        CodexErr::QuotaExceeded,
        CodexErr::new(CodexErrorDetails::InferenceHookDenied {
            message: "workspace denied".into(),
        }),
        CodexErr::new(CodexErrorDetails::CyberPolicy {
            message: "policy_capacity_unavailable".into(),
        }),
        CodexErr::new(CodexErrorDetails::BioPolicy {
            message: "server_is_overloaded".into(),
        }),
        CodexErr::new(CodexErrorDetails::MisalignmentPolicyViolation {
            message: "policy_capacity_unavailable".into(),
            misalignment: None,
        }),
    ] {
        assert!(!should_retry_when_active(true, &error));
        assert!(capacity_reason(&error).is_none());
    }
}

#[test]
fn transient_and_filter_waits_keep_steering_until_the_ordinary_budget_is_spent() {
    for error in [
        CodexErr::Stream("reply cut off".into()),
        CodexErr::new(CodexErrorDetails::ContentFilter),
        CodexErr::InternalServerError,
        status(408),
        status(409),
        status(425),
        status(429),
    ] {
        assert!(!protects_retry(true, &error, 0, 1, false), "{error:?}");
        assert!(protects_retry(true, &error, 1, 1, false), "{error:?}");
    }
    let connection = CodexErr::ConnectionFailed(ConnectionFailedError {
        source: codex_http_client::HttpError::Timeout,
    });
    assert!(!protects_retry(true, &connection, 1, 1, true));
    assert!(
        protects_retry(true, &status(403), 1, 1, true),
        "permanent status wins"
    );
}

#[tokio::test]
async fn unbounded_connection_retries_keep_steering_and_their_original_retry_lane() {
    let (session, mut turn) = crate::session::tests::make_session_and_context().await;
    Arc::make_mut(&mut turn.config)
        .features
        .enable(Feature::UnboundedConnectionRetries)
        .expect("unbounded connection retries");
    turn.session_source = codex_protocol::protocol::SessionSource::Exec;
    let error = CodexErr::ConnectionFailed(ConnectionFailedError {
        source: codex_http_client::HttpError::Timeout,
    });
    assert!(unbounded_connection_retry(&error, &turn));
    let state = ResponsesStreamRetryState::default();
    let preempt = CancellationToken::new();
    preempt.cancel();
    assert!(preempt_when_active(true, &error, &preempt, &state, 0, &turn).is_cancelled());
    turn.session_source = codex_protocol::protocol::SessionSource::Internal(
        codex_protocol::protocol::InternalSessionSource::Guardian,
    );
    assert!(!unbounded_connection_retry(&error, &turn));
    assert!(!preempt_when_active(true, &error, &preempt, &state, 0, &turn).is_cancelled());
    turn.session_source = codex_protocol::protocol::SessionSource::Exec;
    let step = crate::session::step_context::StepContext::for_test(Arc::new(turn));
    let mut state = ResponsesStreamRetryState::default();
    let mut client = session.services.model_client.new_session();
    tokio::time::pause();
    let before = Instant::now();
    crate::responses_retry::handle_response_stream_error(
        &mut state,
        0,
        error,
        &mut client,
        &session,
        &step,
        ResponsesStreamRequest::Sampling,
    )
    .await
    .expect("the real unbounded connection lane still retries");
    assert!(Instant::now() >= before + Duration::from_secs(5));
    assert_eq!(state.retries, 0);
    assert_eq!(state.connection_retries, 1);
}

#[test]
fn completed_errors_propagate_while_cancelled_waits_and_success_keep_follow_up() {
    let error = Ok(Err(CodexErr::Stream("exhausted".into())));
    for preempted in [false, true] {
        for mailbox in [false, true] {
            assert!(!follow_up_when_active(true, &error, preempted, mailbox));
            assert_eq!(
                follow_up_when_active(false, &error, preempted, mailbox),
                preempted
            );
        }
    }
    assert!(follow_up_when_active(
        true,
        &Err(CancelErr::Cancelled),
        true,
        false
    ));
    assert!(!follow_up_when_active(
        true,
        &Err(CancelErr::Cancelled),
        false,
        true
    ));
    assert!(follow_up_when_active(true, &Ok(Ok(())), true, false));
    assert!(follow_up_when_active(true, &Ok(Ok(())), false, true));
    assert!(!follow_up_when_active(true, &Ok(Ok(())), false, false));
}

#[tokio::test]
async fn protecting_an_exhausted_verdict_still_allows_real_fallback_and_its_deadline() {
    let (session, turn, _events) =
        crate::session::tests::make_session_and_context_with_auth_and_config_and_rx(
            codex_login::CodexAuth::from_api_key("private-test-key"),
            Vec::new(),
            |config| {
                config.model_provider.supports_websockets = true;
                config
                    .features
                    .disable(Feature::UnboundedConnectionRetries)
                    .expect("bounded retries");
            },
        )
        .await;
    let step = crate::session::step_context::StepContext::for_test(Arc::clone(&turn));
    let mut client = session.services.model_client.new_session();
    assert!(session.services.model_client.responses_websocket_enabled());
    let mut state = ResponsesStreamRetryState::default();
    let preempt = CancellationToken::new();
    preempt.cancel();
    tokio::time::pause();
    let before = Instant::now();
    let advice = RetryAfter::from_delay(Duration::from_secs(10)).expect("provider advice");
    let error = CodexErr::InternalServerError.with_retry_after(advice);
    let protected = preempt_when_active(true, &error, &preempt, &state, 0, &turn);
    assert!(!protected.is_cancelled());
    let handling = crate::responses_retry::handle_response_stream_error(
        &mut state,
        0,
        error,
        &mut client,
        &session,
        &step,
        ResponsesStreamRequest::Sampling,
    )
    .or_cancel(&protected);
    tokio::pin!(handling);
    assert!(futures::poll!(&mut handling).is_pending());
    tokio::time::advance(Duration::from_secs(9)).await;
    assert!(futures::poll!(&mut handling).is_pending());
    tokio::time::advance(Duration::from_secs(1)).await;
    let result = handling.await;
    assert!(
        matches!(result, Ok(Ok(()))),
        "the real handler selected fallback: {result:?}"
    );
    assert!(Instant::now() >= before + Duration::from_secs(10));
    assert!(!session.services.model_client.responses_websocket_enabled());
    assert!(follow_up_when_active(true, &result, true, false));
}
