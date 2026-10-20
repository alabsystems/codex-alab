// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside core's Responses retry module so upstream needs only a thin hook.

use super::ResponsesStreamRequest;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_alab_core::capacity::CapacityBackoff;
use codex_client::RetryOperation;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use tokio::time::Instant;

pub(super) fn should_retry(err: &CodexErr, request: ResponsesStreamRequest) -> bool {
    codex_alab_core::active()
        && matches!(request, ResponsesStreamRequest::Sampling)
        && matches!(err.details(), CodexErrorDetails::ServerOverloaded)
}

/// The sampling caller wraps this entire future in its existing cancellation guards.
/// Returning to that loop retains its original input, model and task; no new turn is sent.
pub(super) async fn wait(
    backoff: &mut CapacityBackoff,
    err: CodexErr,
    sess: &Session,
    turn_context: &TurnContext,
) {
    let now = Instant::now();
    let local_deadline = now + backoff.next_delay(rand::random());
    // Retry-After is an absolute deadline. Reporting the wait must not restart it, and
    // absent, expired or zero advice must never bypass our positive local backoff.
    let retry_at = err
        .retry_after()
        .map(|advice| advice.deadline().max(local_deadline))
        .unwrap_or(local_deadline);
    let delay = retry_at.saturating_duration_since(now);
    tracing::warn!(
        turn_id = %turn_context.sub_id,
        attempts = backoff.attempts(),
        ?delay,
        "selected model is at capacity; waiting to retry the active task"
    );
    sess.notify_stream_error(
        turn_context,
        "Model is at capacity; retrying automatically (interrupt to stop)...",
        err,
    )
    .await;
    codex_client::record_retry!(backoff.attempts(), delay, RetryOperation::Sampling);
    tokio::time::sleep_until(retry_at).await;
}
