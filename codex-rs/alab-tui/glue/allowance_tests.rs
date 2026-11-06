// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::chatwidget::StatusAccountDisplay;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::clock_format::ClockFormat;
use codex_app_server_protocol::RateLimitResetCredit;
use serde_json::json;

const NOW: i64 = 1_800_000_000;

#[test]
fn early_resume_requires_explicit_permission_and_no_bucket_denial() {
    let available = response(Some("account"), 20);
    assert_eq!(usable_quota_buckets(&available), vec!["codex"]);
    for permitted in [None, Some(false)] {
        let mut read = available.clone();
        read.ordinary_usage_allowed = permitted;
        assert!(usable_quota_buckets(&read).is_empty());
    }
    for primary in [true, false] {
        let mut read = available.clone();
        if primary {
            read.rate_limits.primary = None;
        } else {
            read.rate_limits.secondary = None;
        }
        assert_eq!(usable_quota_buckets(&read), vec!["codex"]);
    }
    assert_eq!(
        usable_quota_buckets(&response(Some("account"), 100)),
        vec!["codex"]
    );
    let mut absent = available.clone();
    absent.rate_limits.primary = None;
    absent.rate_limits.secondary = None;
    assert_eq!(usable_quota_buckets(&absent), vec!["codex"]);
    let mut contradictory = available;
    contradictory.rate_limits.spend_control_reached = Some(true);
    assert!(usable_quota_buckets(&contradictory).is_empty());
}

#[test]
fn premium_without_windows_can_resume_but_missing_bucket_or_permission_cannot() {
    let mut premium = response(Some("account"), 100);
    premium.rate_limits.limit_id = Some("premium".into());
    premium.rate_limits.primary = None;
    premium.rate_limits.secondary = None;
    premium.rate_limits.credits = Some(codex_app_server_protocol::CreditsSnapshot {
        has_credits: false,
        unlimited: false,
        balance: Some("0".into()),
    });
    assert_eq!(usable_quota_buckets(&premium), vec!["premium"]);
    assert!(!usable_quota_buckets(&response(Some("account"), 0)).contains(&"premium".into()));
    for permission in [None, Some(false)] {
        premium.ordinary_usage_allowed = permission;
        assert!(usable_quota_buckets(&premium).is_empty());
    }
    premium.ordinary_usage_allowed = Some(true);
    premium.rate_limit_upsell = Some(json!({"unknown": true}));
    assert!(usable_quota_buckets(&premium).is_empty());
}

fn credit(id: &str, expires_at: Option<i64>) -> RateLimitResetCredit {
    RateLimitResetCredit {
        id: id.to_string(),
        reset_type: RateLimitResetType::CodexRateLimits,
        status: RateLimitResetCreditStatus::Available,
        granted_at: NOW - 86_400,
        expires_at,
        title: None,
        description: None,
    }
}

fn response(account: Option<&str>, used: i32) -> GetAccountRateLimitsResponse {
    serde_json::from_value(json!({
        "accountId": account,
        "ordinaryUsageAllowed": true,
        "rateLimits": {
            "limitId": "codex",
            "primary": {"usedPercent": 18, "windowDurationMins": 300, "resetsAt": NOW + 3600},
            "secondary": {"usedPercent": used, "windowDurationMins": 10080, "resetsAt": NOW + 3 * 86400}
        },
        "rateLimitResetCredits": {"availableCount": 2, "credits": [
            {"id": "soon", "resetType": "codexRateLimits", "status": "available", "grantedAt": NOW - 86400, "expiresAt": NOW + 3600},
            {"id": "later", "resetType": "codexRateLimits", "status": "available", "grantedAt": NOW - 86400, "expiresAt": NOW + 3 * 86400}
        ]}
    }))
    .expect("native account usage read")
}

fn scope() -> interface::Scope {
    let state = Box::leak(Box::new(interface::State::new()));
    let scope = interface::scoped(state);
    interface::set_enabled(true);
    scope
}

#[test]
fn reset_expiries_follow_real_status_type_and_exact_time_boundaries() {
    let mut redeemed = credit("redeemed", Some(NOW - 1));
    redeemed.status = RateLimitResetCreditStatus::Redeemed;
    let mut unknown = credit("unknown", Some(NOW - 1));
    unknown.reset_type = RateLimitResetType::Unknown;
    let mut state = State::default();
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 4,
            credits: Some(vec![
                credit("expired", Some(NOW)),
                credit("soon", Some(NOW + EXPIRING_SOON_SECONDS)),
                credit("later", Some(NOW + EXPIRING_SOON_SECONDS + 1)),
                credit("never", None),
                credit("expired", Some(NOW)),
                redeemed,
                unknown,
            ]),
        }),
        NOW,
    );
    let resets = state.resets_at(NOW, ClockFormat::TwentyFourHour).unwrap();
    assert_eq!(resets.available_count, 3);
    assert_eq!(resets.expired_count, 1);
    assert_eq!(resets.expiring_count, 1);
    assert!(resets.next_expiry.is_some());
    assert!(resets.details_known);
    assert!(!resets.stale);
    assert_eq!(state.next_reset_change(NOW), Some(NOW + 1));
    assert!(
        state
            .resets_at(NOW + STALE_SECONDS + 1, ClockFormat::TwentyFourHour)
            .unwrap()
            .stale
    );
}

#[tokio::test]
async fn pre_expiry_counts_cannot_resurrect_expired_credits_when_details_age() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(
        Some(StatusAccountDisplay::ChatGpt {
            email: Some("verified@example.test".to_string()),
            plan: Some("Pro 500".to_string()),
            workspace: None,
        }),
        None,
        true,
        true,
    );
    let mut detailed = response(Some("selected-account"), 12);
    detailed.rate_limit_reset_credits.as_mut().unwrap().credits = Some(vec![
        credit("expires-before-details-age", Some(NOW + 600)),
        credit("expires-after-details-age", Some(NOW + 1100)),
    ]);
    chat.alab_account_usage_read_for_origin_at(&detailed, RateLimitRefreshOrigin::Periodic, NOW);
    let mut count_only = detailed;
    count_only
        .rate_limit_reset_credits
        .as_mut()
        .unwrap()
        .credits = None;
    chat.alab_account_usage_read_for_origin_at(
        &count_only,
        RateLimitRefreshOrigin::Periodic,
        NOW + 500,
    );
    assert_eq!(
        chat.alab_allowance
            .resets_at(NOW + 600, ClockFormat::TwentyFourHour)
            .unwrap()
            .available_count,
        1,
    );
    let aged_details = chat
        .alab_allowance
        .resets_at(NOW + STALE_SECONDS + 1, ClockFormat::TwentyFourHour)
        .unwrap();
    assert_eq!(
        aged_details.available_count, 1,
        "detail metadata aging cannot undo a known expiry after the last server count"
    );
    assert_eq!(aged_details.expired_count, 1);
    assert!(!aged_details.stale, "the count itself is still current");
    assert!(!aged_details.details_known);
    assert_eq!(aged_details.expiring_count, 0);
    assert!(aged_details.next_expiry.is_none());
    assert_eq!(
        chat.alab_allowance
            .next_reset_change(NOW + STALE_SECONDS + 1),
        Some(NOW + 1100),
        "the remaining known expiry still needs a frame after warnings age out"
    );
    let both_expired = chat
        .alab_allowance
        .resets_at(NOW + 1100, ClockFormat::TwentyFourHour)
        .unwrap();
    assert_eq!(both_expired.available_count, 0);
    assert_eq!(both_expired.expired_count, 2);
    assert!(!both_expired.stale);
    assert!(
        chat.alab_allowance
            .resets_at(NOW + 1401, ClockFormat::TwentyFourHour)
            .unwrap()
            .stale
    );

    chat.alab_account_usage_read_for_origin_at(
        &count_only,
        RateLimitRefreshOrigin::Periodic,
        NOW + 1402,
    );
    let replacements = chat
        .alab_allowance
        .resets_at(NOW + 1402, ClockFormat::TwentyFourHour)
        .unwrap();
    assert_eq!(replacements.available_count, 2);
    assert_eq!(replacements.expired_count, 0);
    assert_eq!(replacements.expiring_count, 0);
    assert!(!replacements.stale);
    assert!(!replacements.details_known);
    assert!(replacements.next_expiry.is_none());
}

#[test]
fn a_fresh_count_after_an_expiry_does_not_subtract_the_same_credit_again() {
    let mut state = State::default();
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 2,
            credits: Some(vec![
                credit("expired-before-next-read", Some(NOW + 300)),
                credit("old-near-expiry", Some(NOW + 600)),
            ]),
        }),
        NOW,
    );
    assert_eq!(
        state
            .resets_at(NOW + 300, ClockFormat::TwentyFourHour)
            .unwrap()
            .available_count,
        1,
        "without a later server read, a known expiry reduces cached availability"
    );
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 2,
            credits: None,
        }),
        NOW + 301,
    );
    let after = state
        .resets_at(NOW + 301, ClockFormat::TwentyFourHour)
        .unwrap();
    assert_eq!(
        after.available_count, 2,
        "the server already counted the expiry and any newly granted credit"
    );
    assert_eq!(after.expired_count, 0);
    assert_eq!(
        after.expiring_count, 0,
        "old deadlines do not belong to replacement credits"
    );
    assert!(!after.details_known);
    assert!(after.next_expiry.is_none());
    assert!(
        state.request_details(NOW + 302),
        "expiry permits one detail refresh on the next existing poll"
    );
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 2,
            credits: None,
        }),
        NOW + 303,
    );
    assert!(
        !state.request_details(NOW + 303),
        "missing detail replies retain bounded attempts"
    );
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 2,
            credits: Some(vec![
                credit("replacement-a", Some(NOW + 3 * 86400)),
                credit("replacement-b", Some(NOW + 4 * 86400)),
            ]),
        }),
        NOW + 304,
    );
    let refreshed = state
        .resets_at(NOW + 304, ClockFormat::TwentyFourHour)
        .unwrap();
    assert_eq!(refreshed.available_count, 2);
    assert_eq!(refreshed.expiring_count, 0);
    assert!(refreshed.details_known);
    assert!(refreshed.next_expiry.is_some());
    assert!(!state.request_details(NOW + 305));
}

#[test]
fn count_only_reconciliation_preserves_current_rows_and_respects_changed_or_stale_counts() {
    for (elapsed, count, details_known, expiring) in [
        (299, 2, true, 2),
        (301, 1, false, 0),
        (STALE_SECONDS + 1, 2, false, 0),
    ] {
        let mut state = State::default();
        state.remember_resets(
            Some(&RateLimitResetCreditsSummary {
                available_count: 2,
                credits: Some(vec![
                    credit("first", Some(NOW + 300)),
                    credit("second", Some(NOW + 600)),
                ]),
            }),
            NOW,
        );
        state.remember_resets(
            Some(&RateLimitResetCreditsSummary {
                available_count: count,
                credits: None,
            }),
            NOW + elapsed,
        );
        let resets = state
            .resets_at(NOW + elapsed, ClockFormat::TwentyFourHour)
            .unwrap();
        assert_eq!(resets.available_count, count as usize, "elapsed={elapsed}");
        assert_eq!(resets.details_known, details_known, "elapsed={elapsed}");
        assert_eq!(resets.expiring_count, expiring, "elapsed={elapsed}");
        assert!(!resets.stale, "the accepted count is current");
    }
}

#[test]
fn only_an_available_codex_credit_expiry_invalidates_count_only_detail_associations() {
    let mut redeemed = credit("redeemed", Some(NOW + 300));
    redeemed.status = RateLimitResetCreditStatus::Redeemed;
    let mut unknown = credit("other-reset-type", Some(NOW + 300));
    unknown.reset_type = RateLimitResetType::Unknown;
    let mut state = State::default();
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 1,
            credits: Some(vec![
                redeemed,
                unknown,
                credit("available", Some(NOW + 3600)),
            ]),
        }),
        NOW,
    );
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 1,
            credits: None,
        }),
        NOW + 301,
    );
    let current = state
        .resets_at(NOW + 301, ClockFormat::TwentyFourHour)
        .unwrap();
    assert_eq!(current.available_count, 1);
    assert_eq!(current.expiring_count, 1);
    assert!(current.details_known);
    assert!(current.next_expiry.is_some());
}

#[test]
fn capped_credit_details_preserve_known_expiries_without_claiming_complete_knowledge() {
    let mut complete = State::default();
    complete.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 2,
            credits: Some(vec![
                credit("soon", Some(NOW + 3600)),
                credit("non-expiring", None),
            ]),
        }),
        NOW,
    );
    let complete = complete
        .resets_at(NOW, ClockFormat::TwentyFourHour)
        .unwrap();
    assert!(complete.details_known);
    assert_eq!(complete.available_count, 2);
    assert_eq!(complete.expiring_count, 1);
    assert!(complete.next_expiry.is_some());

    for (known, expiring) in [
        (credit("later", Some(NOW + 3 * 86400)), 0),
        (credit("soon", Some(NOW + 3600)), 1),
        (credit("non-expiring", None), 0),
    ] {
        let has_expiry = known.expires_at.is_some();
        let mut partial = State::default();
        partial.remember_resets(
            Some(&RateLimitResetCreditsSummary {
                available_count: 2,
                credits: Some(vec![known]),
            }),
            NOW,
        );
        let partial = partial.resets_at(NOW, ClockFormat::TwentyFourHour).unwrap();
        assert_eq!(partial.available_count, 2);
        assert_eq!(partial.expired_count, 0);
        assert_eq!(partial.expiring_count, expiring);
        assert_eq!(partial.next_expiry.is_some(), has_expiry);
        assert!(!partial.details_known);
    }
}

#[test]
fn count_only_reads_preserve_agreeing_details_without_inventing_expiry() {
    let mut state = State::default();
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 2,
            credits: Some(vec![credit("known", Some(NOW + 3600))]),
        }),
        NOW,
    );
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 2,
            credits: None,
        }),
        NOW + 1,
    );
    let same = state.resets_at(NOW + 1, ClockFormat::TwelveHour).unwrap();
    assert_eq!(
        same.available_count, 2,
        "a capped detail list is not the count"
    );
    assert_eq!(same.expiring_count, 1);
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 1,
            credits: None,
        }),
        NOW + 2,
    );
    let changed = state.resets_at(NOW + 2, ClockFormat::TwelveHour).unwrap();
    assert_eq!(changed.available_count, 1);
    assert_eq!(changed.expiring_count, 0);
    assert!(!changed.details_known);
    assert_eq!(changed.next_expiry, None);
    state.remember_resets(None, NOW + 3);
    assert!(state.resets_at(NOW + 3, ClockFormat::TwelveHour).is_none());
}

#[test]
fn existing_polls_refresh_expiry_details_without_count_only_reads_renewing_them() {
    let mut state = State::default();
    assert!(
        state.request_details(NOW),
        "the first existing poll asks for expiry details"
    );
    assert!(
        !state.request_details(NOW + 1),
        "failed or missing details have a bounded refresh cadence"
    );
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 2,
            credits: Some(vec![credit("known", Some(NOW + 3600))]),
        }),
        NOW,
    );
    for elapsed in [100, 200, 800] {
        state.remember_resets(
            Some(&RateLimitResetCreditsSummary {
                available_count: 2,
                credits: None,
            }),
            NOW + elapsed,
        );
        assert!(!state.request_details(NOW + elapsed));
    }
    assert!(state.request_details(NOW + STALE_SECONDS));
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 2,
            credits: None,
        }),
        NOW + STALE_SECONDS + 1,
    );
    let old_details = state
        .resets_at(NOW + STALE_SECONDS + 1, ClockFormat::TwentyFourHour)
        .unwrap();
    assert!(!old_details.stale, "the count itself was just observed");
    assert_eq!(
        old_details.expiring_count, 0,
        "old detail rows cannot glow as current credits"
    );
    assert!(!old_details.details_known);
    assert!(old_details.next_expiry.is_none());
    assert!(!state.request_details(NOW + STALE_SECONDS + 2));
    state.remember_resets(
        Some(&RateLimitResetCreditsSummary {
            available_count: 3,
            credits: None,
        }),
        NOW + STALE_SECONDS + 3,
    );
    assert!(
        state.request_details(NOW + STALE_SECONDS + 3),
        "a changed count permits one detail refresh on the next existing poll"
    );
    state.inspector_requested_details = true;
    assert!(
        state.request_details(NOW + STALE_SECONDS + 4),
        "opening account details explicitly refreshes expiry data"
    );
    assert!(!state.request_details(NOW + STALE_SECONDS + 5));
}

#[test]
fn a_backward_clock_step_refreshes_reset_details_once_then_keeps_the_normal_cadence() {
    let detailed = RateLimitResetCreditsSummary {
        available_count: 1,
        credits: Some(vec![credit("known", Some(NOW + 3600))]),
    };
    let count_only = RateLimitResetCreditsSummary {
        credits: None,
        ..detailed
    };
    let mut state = State::default();
    assert!(state.request_details(NOW));
    state.remember_resets(Some(&detailed), NOW);
    assert!(!state.request_details(NOW + 1));

    let rolled_back = NOW - 60;
    state.remember_resets(Some(&count_only), rolled_back);
    let before = state
        .resets_at(rolled_back, ClockFormat::TwentyFourHour)
        .unwrap();
    assert!(!before.stale, "the latest count was just read");
    assert!(!before.details_known);
    assert_eq!(before.expiring_count, 0);
    assert!(before.next_expiry.is_none());
    assert!(
        state.request_details(rolled_back),
        "future detail and attempt timestamps cannot postpone repair"
    );

    state.remember_resets(Some(&detailed), rolled_back);
    let repaired = state
        .resets_at(rolled_back, ClockFormat::TwentyFourHour)
        .unwrap();
    assert!(repaired.details_known);
    assert_eq!(repaired.expiring_count, 1);
    assert!(!state.request_details(rolled_back));
    assert!(!state.request_details(rolled_back + STALE_SECONDS - 1));
    assert!(state.request_details(rolled_back + STALE_SECONDS));
    assert!(!state.request_details(rolled_back + STALE_SECONDS + 1));
}

#[tokio::test]
async fn account_usage_observations_require_the_same_pending_origin_as_native_completion() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(
        Some(StatusAccountDisplay::ChatGpt {
            email: Some("verified@example.test".to_string()),
            plan: Some("Pro 500".to_string()),
            workspace: None,
        }),
        None,
        true,
        true,
    );
    chat.alab_account_usage_read_at(&response(Some("selected-account"), 27), NOW);
    chat.pending_rate_limit_reset_hint_request_id = Some(11);
    chat.pending_rate_limit_reset_request_id = Some(22);
    chat.pending_usage_menu_rate_limit_request_id = Some(33);
    chat.add_status_output(true, Some(44));
    let store = quota::Store::new(&chat.local_settings.codex_home);
    for (index, (stale, current)) in [
        (
            RateLimitRefreshOrigin::StartupPrefetch {
                reset_hint_request_id: 22,
            },
            RateLimitRefreshOrigin::StartupPrefetch {
                reset_hint_request_id: 11,
            },
        ),
        (
            RateLimitRefreshOrigin::ResetPicker { request_id: 11 },
            RateLimitRefreshOrigin::ResetPicker { request_id: 22 },
        ),
        (
            RateLimitRefreshOrigin::ResetConsume { request_id: 33 },
            RateLimitRefreshOrigin::ResetConsume { request_id: 22 },
        ),
        (
            RateLimitRefreshOrigin::UsageMenu { request_id: 44 },
            RateLimitRefreshOrigin::UsageMenu { request_id: 33 },
        ),
        (
            RateLimitRefreshOrigin::StatusCommand { request_id: 11 },
            RateLimitRefreshOrigin::StatusCommand { request_id: 44 },
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let now = NOW + index as i64 + 1;
        let mut incoming = response(Some("selected-account"), 42 + index as i32);
        let count = 1 + index as i64 % 3;
        incoming
            .rate_limit_reset_credits
            .as_mut()
            .unwrap()
            .available_count = count;
        let before = chat.alab_allowance.weekly.clone().unwrap();
        let resets_before = chat
            .alab_allowance
            .resets_at(now, ClockFormat::TwentyFourHour);
        let stored_before = store.read("account:selected-account", now).unwrap();

        assert!(!chat.alab_usage_origin_current(stale));
        assert!(chat.alab_usage_origin_current(current));
        chat.alab_account_usage_read_for_origin_at(&incoming, stale, now);

        assert_eq!(chat.alab_allowance.weekly.as_ref(), Some(&before));
        assert_eq!(
            chat.alab_allowance
                .resets_at(now, ClockFormat::TwentyFourHour),
            resets_before,
            "an ID pending for a different origin grants no admission: {stale:?}"
        );
        assert_eq!(
            store.read("account:selected-account", now).unwrap(),
            stored_before,
            "an obsolete completion cannot change durable usage history"
        );

        chat.alab_account_usage_read_for_origin_at(&incoming, current, now);

        let accepted = chat.alab_allowance.weekly.as_ref().unwrap();
        assert_eq!(accepted.observed_at, now, "{current:?}");
        assert_eq!(accepted.used_percent, f64::from(42 + index as i32));
        assert_eq!(
            chat.alab_allowance
                .resets_at(now, ClockFormat::TwentyFourHour)
                .unwrap()
                .available_count,
            count as usize
        );
    }

    chat.pending_rate_limit_reset_hint_request_id = None;
    chat.pending_rate_limit_reset_request_id = None;
    chat.pending_usage_menu_rate_limit_request_id = None;
    chat.refreshing_status_outputs.clear();
    for (index, origin) in [
        RateLimitRefreshOrigin::Periodic,
        RateLimitRefreshOrigin::Recovery,
    ]
    .into_iter()
    .enumerate()
    {
        let now = NOW + 100 + index as i64;
        chat.alab_account_usage_read_for_origin_at(
            &response(Some("selected-account"), 90 + index as i32),
            origin,
            now,
        );
        assert_eq!(
            chat.alab_allowance.weekly.as_ref().unwrap().observed_at,
            now
        );
    }
}

#[tokio::test]
async fn accepted_observations_keep_account_history_and_never_record_render_frames() {
    let _scope = scope();
    let (mut chat, _tx, _rx, mut ops) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(
        Some(StatusAccountDisplay::ChatGpt {
            email: Some("verified@example.test".to_string()),
            plan: Some("Pro 500".to_string()),
            workspace: None,
        }),
        None,
        true,
        true,
    );
    while ops.try_recv().is_ok() {}
    for (elapsed, used) in [(0, 10), (900, 12), (1800, 14)] {
        chat.alab_account_usage_read_at(&response(Some("selected-account"), used), NOW + elapsed);
    }
    let weekly = chat.alab_allowance.weekly.as_ref().unwrap();
    assert_eq!(weekly.sample_count, 3);
    assert!(weekly.projection.is_some());
    assert_eq!(weekly.used_percent, 14.0);
    let stored = quota::Store::new(&chat.local_settings.codex_home)
        .read("account:selected-account", NOW + 1800)
        .unwrap()
        .unwrap();
    assert_eq!(stored.sample_count, 3);
    // Rendering/freshness reevaluation reads the view only, never Store::observe.
    for later in [NOW + 1801, NOW + 1802, NOW + 1803] {
        assert_eq!(weekly.at(later).sample_count, 3);
    }
    assert!(
        ops.try_recv().is_err(),
        "quota display does not send model work or redeem a credit"
    );
    chat.update_account_state(
        Some(StatusAccountDisplay::ChatGpt {
            email: Some("verified@example.test".to_string()),
            plan: Some("Pro 500".to_string()),
            workspace: None,
        }),
        None,
        true,
        true,
    );
    assert!(
        chat.alab_allowance.weekly.is_none(),
        "same-email account notifications still invalidate accepted data"
    );
    assert!(
        chat.alab_allowance
            .resets_at(NOW + 1800, ClockFormat::TwentyFourHour)
            .is_none()
    );
    chat.alab_account_usage_read_at(&response(Some("other-account"), 71), NOW + 1900);
    let other = chat.alab_allowance.weekly.as_ref().unwrap();
    assert_eq!(other.used_percent, 71.0);
    assert_eq!(other.sample_count, 1);
    assert!(other.projection.is_none());
    chat.pending_rate_limit_reset_request_id = Some(7);
    assert!(!chat.finish_rate_limit_reset_consume(
        6,
        "stale-key".into(),
        None,
        Err("old response".into())
    ));
    assert!(
        chat.alab_allowance
            .resets_at(NOW + 1900, ClockFormat::TwentyFourHour)
            .is_some(),
        "an unrelated redemption result cannot clear this account's display"
    );
    assert!(!chat.finish_rate_limit_reset_consume(
        7,
        "current-key".into(),
        None,
        Err("redemption response lost".into())
    ));
    assert!(
        chat.alab_allowance
            .resets_at(NOW + 1900, ClockFormat::TwentyFourHour)
            .is_none(),
        "a credit may have been spent while its response was lost"
    );
    assert!(
        chat.alab_allowance.weekly.is_some(),
        "only reset availability is invalidated"
    );
}

#[tokio::test]
async fn weekly_history_keeps_accepted_observations_through_storage_failures() {
    let _scope = scope();
    for failure in ["locked", "malformed", "not-a-file"] {
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(
            Some(StatusAccountDisplay::ChatGpt {
                email: Some("verified@example.test".into()),
                plan: Some("Pro 500".into()),
                workspace: None,
            }),
            None,
            true,
            true,
        );
        for (elapsed, used) in [(0, 10), (900, 12), (1800, 14)] {
            chat.alab_account_usage_read_at(
                &response(Some("selected-account"), used),
                NOW + elapsed,
            );
        }
        let before = chat.alab_allowance.weekly.as_ref().unwrap().clone();
        assert_eq!(before.sample_count, 3);
        assert!(before.projection.is_some());
        let history_file = std::fs::read_dir(chat.local_settings.codex_home.join("alab/quota"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .unwrap();
        let lock = match failure {
            "locked" => {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(history_file.with_extension("lock"))
                    .unwrap();
                file.try_lock().unwrap();
                Some(file)
            }
            "malformed" => {
                std::fs::write(&history_file, b"not a quota cache").unwrap();
                None
            }
            "not-a-file" => {
                std::fs::remove_file(&history_file).unwrap();
                std::fs::create_dir(&history_file).unwrap();
                None
            }
            _ => unreachable!(),
        };
        for (elapsed, used, count) in [(2700, 16, 4), (3600, 18, 5)] {
            chat.alab_account_usage_read_at(
                &response(Some("selected-account"), used),
                NOW + elapsed,
            );
            let current = chat.alab_allowance.weekly.as_ref().unwrap();
            assert_eq!(current.used_percent, f64::from(used), "{failure}");
            assert_eq!(current.observed_at, NOW + elapsed, "{failure}");
            assert_eq!(
                current.sample_count, count,
                "{failure}: accepted same-cycle history survives unavailable storage"
            );
            assert!(current.projection.is_some(), "{failure}");
        }
        drop(lock);
        if failure == "not-a-file" {
            std::fs::remove_dir(&history_file).unwrap();
        }
        chat.alab_account_usage_read_at(&response(Some("selected-account"), 20), NOW + 4500);
        let recovered = chat.alab_allowance.weekly.as_ref().unwrap();
        assert_eq!(recovered.used_percent, 20.0);
        assert_eq!(recovered.sample_count, 6, "{failure}");
        assert!(recovered.projection.is_some(), "{failure}");
        assert_eq!(
            quota::Store::new(&chat.local_settings.codex_home)
                .read("account:selected-account", NOW + 4500)
                .unwrap()
                .as_ref(),
            Some(recovered),
            "{failure}: accepted observations survive a later relaunch"
        );
    }
}

#[tokio::test]
async fn weekly_history_storage_fallback_keeps_account_and_cycle_boundaries() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
    let display = || {
        Some(StatusAccountDisplay::ChatGpt {
            email: Some("same-email@example.test".into()),
            plan: Some("Pro 500".into()),
            workspace: None,
        })
    };
    chat.update_account_state(display(), None, true, true);
    for (elapsed, used) in [(0, 10), (900, 12), (1800, 14)] {
        chat.alab_account_usage_read_at(&response(Some("selected-account"), used), NOW + elapsed);
    }
    let quota_directory = chat.local_settings.codex_home.join("alab/quota");
    std::fs::rename(&quota_directory, quota_directory.with_extension("saved")).unwrap();
    std::fs::write(&quota_directory, b"quota directory temporarily unavailable").unwrap();

    let mut corrected = response(Some("selected-account"), 14);
    corrected.rate_limits.secondary.as_mut().unwrap().resets_at = Some(NOW + 4 * 86400);
    chat.alab_account_usage_read_at(&corrected, NOW + 2700);
    let replacement = chat.alab_allowance.weekly.as_ref().unwrap();
    assert_eq!(replacement.sample_count, 1);
    assert_eq!(replacement.resets_at, NOW + 4 * 86400);
    assert!(replacement.projection.is_none());
    for (elapsed, used) in [(3600, 16), (4500, 18)] {
        corrected
            .rate_limits
            .secondary
            .as_mut()
            .unwrap()
            .used_percent = used;
        chat.alab_account_usage_read_at(&corrected, NOW + elapsed);
    }
    let rebuilt = chat.alab_allowance.weekly.as_ref().unwrap();
    assert_eq!(rebuilt.sample_count, 3);
    assert!(rebuilt.projection.is_some());

    // A native account notification is a boundary even when optional display
    // metadata agrees, and storage failure cannot bypass that boundary.
    chat.update_account_state(display(), None, true, true);
    chat.alab_account_usage_read_at(&response(Some("other-account"), 40), NOW + 5400);
    let other = chat.alab_allowance.weekly.as_ref().unwrap();
    assert_eq!(other.used_percent, 40.0);
    assert_eq!(other.sample_count, 1);
    assert!(other.projection.is_none());
    chat.alab_account_usage_read_at(&response(Some("selected-account"), 50), NOW + 6300);
    let switched_back = chat.alab_allowance.weekly.as_ref().unwrap();
    assert_eq!(switched_back.used_percent, 50.0);
    assert_eq!(switched_back.sample_count, 1);
    assert!(switched_back.projection.is_none());
}

#[tokio::test]
async fn unidentified_weekly_history_cannot_cross_a_same_email_native_account_generation() {
    let _scope = scope();
    for account_id in [None, Some("stable-account")] {
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        let display = || {
            Some(StatusAccountDisplay::ChatGpt {
                email: Some("same-email@example.test".to_string()),
                plan: Some("Pro 500".to_string()),
                workspace: None,
            })
        };
        chat.update_account_state(display(), None, true, true);
        for (elapsed, used) in [(0, 10), (900, 20), (1800, 30)] {
            chat.alab_account_usage_read_at(&response(account_id, used), NOW + elapsed);
        }
        let first = chat.alab_allowance.weekly.as_ref().unwrap();
        assert_eq!(first.sample_count, 3);
        assert!(
            first.projection.is_some(),
            "one native generation retains its observed pace"
        );

        // Workspace switches can retain the same email and plan. The native
        // notification is the boundary; an email cannot identify the workspace.
        chat.update_account_state(display(), None, true, true);
        assert!(chat.alab_allowance.weekly.is_none());
        chat.alab_account_usage_read_at(&response(account_id, 35), NOW + 2700);
        let next = chat.alab_allowance.weekly.as_ref().unwrap();
        if account_id.is_some() {
            assert_eq!(
                next.sample_count, 4,
                "a stable backend identity keeps its durable history"
            );
            assert!(next.projection.is_some());
        } else {
            assert_eq!(
                next.sample_count, 1,
                "the same email cannot reload another workspace's observations"
            );
            assert!(next.projection.is_none());
            assert!(
                quota::Store::new(&chat.local_settings.codex_home)
                    .read("email:same-email@example.test", NOW + 2700)
                    .unwrap()
                    .is_none()
            );
        }
        for (elapsed, used) in [(3600, 40), (4500, 45)] {
            chat.alab_account_usage_read_at(&response(account_id, used), NOW + elapsed);
        }
        let recent = chat.alab_allowance.weekly.as_ref().unwrap();
        assert_eq!(
            recent.sample_count,
            if account_id.is_some() { 6 } else { 3 }
        );
        assert!(
            recent.projection.is_some(),
            "the new generation can build its own forecast"
        );
    }
}

#[tokio::test]
async fn unidentified_weekly_history_does_not_load_legacy_email_cache_files() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
    let store = quota::Store::new(&chat.local_settings.codex_home);
    for (elapsed, used) in [(-3600, 1.0), (-2700, 2.0), (-1800, 3.0)] {
        store
            .observe(
                "email:legacy@example.test",
                quota::WeeklySnapshot {
                    used_percent: used,
                    observed_at: NOW + elapsed,
                    resets_at: NOW + 3 * 86400,
                    window_minutes: 10080,
                },
            )
            .unwrap();
    }
    let before = store.read("email:legacy@example.test", NOW + 1800).unwrap();
    chat.update_account_state(
        Some(StatusAccountDisplay::ChatGpt {
            email: Some("legacy@example.test".to_string()),
            plan: Some("Pro".to_string()),
            workspace: None,
        }),
        None,
        true,
        true,
    );
    for (elapsed, used) in [(0, 10), (900, 12), (1800, 14)] {
        chat.alab_account_usage_read_at(&response(None, used), NOW + elapsed);
    }
    let current = chat.alab_allowance.weekly.as_ref().unwrap();
    assert_eq!(
        current.sample_count, 3,
        "only this generation's observations belong in its graph"
    );
    assert!(current.projection.is_some());
    assert_eq!(
        store.read("email:legacy@example.test", NOW + 1800).unwrap(),
        before,
        "a legacy file is ignored, without rewriting or deleting it"
    );
}

#[tokio::test]
async fn native_usage_before_email_metadata_keeps_generation_scoped_quota() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
    let display = || {
        crate::app_server_session::status_account_display_from_auth_mode(
            Some(codex_app_server_protocol::AuthMode::Chatgpt),
            Some(codex_protocol::account::PlanType::ProMax),
        )
    };
    assert!(matches!(
        display(),
        Some(StatusAccountDisplay::ChatGpt { email: None, .. })
    ));
    chat.update_account_state(display(), None, true, true);
    for (elapsed, used) in [(0, 10), (900, 12), (1800, 14)] {
        chat.alab_account_usage_read_for_origin_at(
            &response(None, used),
            RateLimitRefreshOrigin::Periodic,
            NOW + elapsed,
        );
    }
    let before_email =
        chat.alab_allowance.weekly.as_ref().expect(
            "an accepted native usage read does not require optional account display metadata",
        );
    assert_eq!(before_email.sample_count, 3);
    assert_eq!(before_email.used_percent, 14.0);
    assert!(before_email.projection.is_some());
    let resets = chat
        .alab_allowance
        .resets_at(NOW + 1800, ClockFormat::TwentyFourHour)
        .unwrap();
    assert_eq!(resets.available_count, 2);
    assert_eq!(resets.expiring_count, 1);
    assert!(resets.details_known);

    chat.on_account_email_loaded(Some("verified@example.test".into()), None);
    chat.alab_account_usage_read_at(&response(None, 16), NOW + 2700);
    assert_eq!(
        chat.alab_allowance.weekly.as_ref().unwrap().sample_count,
        4,
        "loading display metadata does not change the native account generation"
    );
    let mut unrelated = response(None, 18);
    unrelated
        .rate_limits
        .secondary
        .as_mut()
        .unwrap()
        .window_duration_mins = Some(300);
    chat.alab_account_usage_read_at(&unrelated, NOW + 2701);
    assert!(chat.alab_allowance.weekly.is_none());
    chat.alab_account_usage_read_at(&response(None, 18), NOW + 3600);
    assert_eq!(
        chat.alab_allowance.weekly.as_ref().unwrap().sample_count,
        5,
        "an unrelated window cannot add a weekly observation"
    );

    chat.update_account_state(display(), None, true, true);
    assert!(chat.alab_allowance.weekly.is_none());
    assert!(
        chat.alab_allowance
            .resets_at(NOW + 3600, ClockFormat::TwentyFourHour)
            .is_none()
    );
    chat.alab_account_usage_read_at(&response(None, 50), NOW + 4500);
    let next_generation = chat.alab_allowance.weekly.as_ref().unwrap();
    assert_eq!(next_generation.sample_count, 1);
    assert!(next_generation.projection.is_none());
    assert!(
        quota::Store::new(&chat.local_settings.codex_home)
            .read("email:verified@example.test", NOW + 4500)
            .unwrap()
            .is_none(),
        "unidentified history remains local to the native generation"
    );
    chat.update_account_state(Some(StatusAccountDisplay::ApiKey), None, false, true);
    chat.alab_account_usage_read_at(&response(Some("backend-id"), 75), NOW + 5400);
    assert!(chat.alab_allowance.weekly.is_none());
    assert!(
        chat.alab_allowance
            .resets_at(NOW + 5400, ClockFormat::TwentyFourHour)
            .is_none()
    );
}

#[tokio::test]
async fn weekly_meter_uses_the_native_top_level_and_multi_bucket_canonicalization() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(
        Some(StatusAccountDisplay::ChatGpt {
            email: Some("verified@example.test".into()),
            plan: Some("Pro 500".into()),
            workspace: None,
        }),
        None,
        true,
        true,
    );
    for (case, (top_level_id, expected_used, expected_reset)) in [
        (Some("codex"), 12.0, NOW + 3 * 86_400),
        (Some("other-model"), 62.0, NOW + 2 * 86_400),
        (None, 62.0, NOW + 2 * 86_400),
    ]
    .into_iter()
    .enumerate()
    {
        let mut read = response(Some(&format!("selected-account-{case}")), 12);
        read.rate_limits.limit_id = top_level_id.map(str::to_string);
        let mut mapped = response(Some("selected-account"), 62).rate_limits;
        mapped.secondary.as_mut().unwrap().resets_at = Some(NOW + 2 * 86_400);
        read.rate_limits_by_limit_id = Some(std::collections::HashMap::from([(
            "codex".to_string(),
            mapped,
        )]));
        chat.alab_account_usage_read_at(&read, NOW);
        for snapshot in crate::app_server_session::app_server_rate_limit_snapshots(read) {
            chat.on_rate_limit_snapshot(Some(snapshot));
        }
        let native = chat.rate_limit_snapshots_by_limit_id.get("codex").unwrap();
        let native_weekly = native.secondary.as_ref().unwrap();
        assert_eq!(
            native_weekly.used_percent, expected_used,
            "top-level ID: {top_level_id:?}"
        );
        let meter = chat.alab_allowance.weekly.as_ref().unwrap();
        assert_eq!(
            meter.used_percent, expected_used,
            "meter must agree with native usage"
        );
        assert_eq!(
            meter.resets_at, expected_reset,
            "reset cycle must follow the same selected bucket"
        );
        assert_eq!(chat.alab_allowance.weekly_resets_at, Some(expected_reset));
    }
}

#[tokio::test]
async fn provider_plan_changes_refresh_capabilities_without_opening_a_picker() {
    let _scope = scope();
    let (mut chat, _tx, mut rx, _ops) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(
        Some(crate::status::StatusAccountDisplay::ChatGpt {
            email: Some("owner@example.com".into()),
            plan: Some("Pro".into()),
            workspace: None,
        }),
        None,
        true,
        true,
    );
    chat.bottom_pane
        .set_composer_text("keep the draft".into(), Vec::new(), Vec::new());
    let mut response = response(Some("owner"), 18);
    response.rate_limits.plan_type = Some(codex_protocol::account::PlanType::Pro);
    while rx.try_recv().is_ok() {}
    chat.alab_account_usage_read_at(&response, NOW);
    let count = |rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::app_event::AppEvent>| {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter(|event| matches!(event, crate::app_event::AppEvent::FetchModels { .. }))
            .count()
    };
    assert_eq!(count(&mut rx), 1);
    chat.alab_account_usage_read_at(&response, NOW + 1);
    assert_eq!(count(&mut rx), 0);
    response.rate_limits.plan_type = Some(codex_protocol::account::PlanType::ProMax);
    chat.alab_account_usage_read_at(&response, NOW + 2);
    assert_eq!(count(&mut rx), 1);
    assert_eq!(chat.bottom_pane.composer_text(), "keep the draft");
    assert!(chat.bottom_pane.no_modal_or_popup_active());
}
