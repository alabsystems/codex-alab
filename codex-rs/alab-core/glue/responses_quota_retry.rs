// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Headless exec retains its active request across account-quota walls.
//! Interactive consumers retain their existing policy and account recovery path.

use super::*;
use chrono::Utc;
use codex_protocol::protocol::RateLimitReachedType;
use std::time::Duration;

const UNKNOWN_RESET_WAIT: Duration = Duration::from_secs(30);

pub(super) fn eligible(err: &CodexErr, turn: &TurnContext) -> bool {
    reset_delay(
        codex_alab_core::active(),
        turn.extension_data
            .get::<codex_alab_core::quota::HeadlessExec>()
            .is_some()
            || turn
                .extension_data
                .get::<codex_alab_core::quota::ExecQuotaMember>()
                .is_some(),
        err,
        Utc::now(),
    )
    .is_some()
}

fn reset_delay(
    active: bool,
    runtime_exec: bool,
    err: &CodexErr,
    now: chrono::DateTime<Utc>,
) -> Option<Duration> {
    if !active
        || !runtime_exec
        || err
            .http_status_code_value()
            .is_some_and(permanent_client_status)
    {
        return None;
    }
    let CodexErrorDetails::UsageLimitReached(quota) = err.details() else {
        return None;
    };
    if quota
        .rate_limit_reached_type
        .is_some_and(|kind| kind != RateLimitReachedType::RateLimitReached)
    {
        return None;
    }
    let Some(reset) = quota.resets_at else {
        return Some(UNKNOWN_RESET_WAIT);
    };
    Some(
        reset
            .signed_duration_since(now)
            .to_std()
            .unwrap_or_default(),
    )
}

pub(super) async fn wait(
    backoff: &mut CapacityBackoff,
    err: CodexErr,
    sess: &Session,
    turn: &TurnContext,
) {
    let Some(reset) = reset_delay(true, true, &err, Utc::now()) else {
        return;
    };
    // The live sampler, rather than a UI error string, identifies the exact
    // attempted account. This lease ends before a retry or on cancellation.
    let _quota_observation = match err.details() {
        CodexErrorDetails::UsageLimitReached(details)
            if codex_alab_quota_recovery::ordinary_weekly_wall(details) =>
        {
            crate::session::alab_exec_recovery_options::quota_observation::publish(sess, turn)
        }
        _ => None,
    };
    // A stale provider reset still needs positive backoff. Repeated quota errors
    // grow that backoff rather than spinning at the expired reset timestamp.
    let delay = reset.max(backoff.next_delay(rand::random()));
    let local_deadline = Instant::now() + delay;
    let armed = err.retry_after().map_or_else(
        || codex_alab_core::wake::ArmedWait::new(local_deadline),
        |advice| advice.arm_sleep_wait(Some(local_deadline)),
    );
    let observer = turn
        .extension_data
        .get::<InFlightRetryObserver>()
        .map(|observer| observer.waiting(armed.deadline().into_std()));
    tracing::info!(turn_id = %turn.sub_id, ?delay, "account quota exhausted; waiting to retry");
    sess.notify_stream_error(turn, "Waiting for account quota", err)
        .await;
    // The caller retains task cancellation. Reporting does not restart the wait,
    // and host sleep counts toward the provider's real-time reset deadline.
    armed
        .wait_observed(|deadline| {
            if let Some(observer) = &observer {
                let _ = observer.update(deadline.into_std());
            }
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::error::UsageLimitReachedError;

    fn quota_details(reset: Option<chrono::DateTime<Utc>>) -> UsageLimitReachedError {
        UsageLimitReachedError {
            plan_type: None,
            resets_at: reset,
            limit_window_minutes: None,
            rate_limits: None,
            promo_message: None,
            rate_limit_reached_type: None,
        }
    }

    fn quota(reset: Option<chrono::DateTime<Utc>>) -> CodexErr {
        CodexErr::UsageLimitReached(quota_details(reset))
    }

    #[test]
    fn only_active_exec_retries_recoverable_account_quota() {
        let now = chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let future = now + chrono::Duration::seconds(120);
        let error = quota(Some(future));
        assert_eq!(
            reset_delay(true, true, &error, now),
            Some(Duration::from_secs(120))
        );
        assert_eq!(reset_delay(false, true, &error, now), None);
        assert_eq!(reset_delay(true, false, &error, now), None);
        assert_eq!(
            reset_delay(true, true, &quota(None), now),
            Some(UNKNOWN_RESET_WAIT)
        );
        for error in [
            CodexErr::QuotaExceeded,
            CodexErr::UsageNotIncluded,
            CodexErr::ServerOverloaded,
        ] {
            assert_eq!(reset_delay(true, true, &error, now), None);
        }
        assert_eq!(
            reset_delay(true, true, &quota(Some(now)), now),
            Some(Duration::ZERO)
        );
        for kind in [
            RateLimitReachedType::WorkspaceOwnerCreditsDepleted,
            RateLimitReachedType::WorkspaceMemberCreditsDepleted,
            RateLimitReachedType::WorkspaceOwnerUsageLimitReached,
            RateLimitReachedType::WorkspaceMemberUsageLimitReached,
        ] {
            let mut details = quota_details(Some(future));
            details.rate_limit_reached_type = Some(kind);
            assert_eq!(
                reset_delay(true, true, &CodexErr::UsageLimitReached(details), now),
                None
            );
        }
    }
}
