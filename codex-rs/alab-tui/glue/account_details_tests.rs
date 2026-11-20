// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::ViewCompletion;
use crate::chatwidget::StatusAccountDisplay;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::chatwidget::tests::render_bottom_popup;
use crate::clock_format::ClockFormat;
use crate::keymap::RuntimeKeymap;
use chrono::Local;
use chrono::TimeZone;
use codex_app_server_protocol::RateLimitSnapshot;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Cell;

fn scope() -> interface::Scope {
    let state = Box::leak(Box::new(interface::State::new()));
    let scope = interface::scoped(state);
    interface::set_enabled(true);
    scope
}

fn account(email: Option<&str>) -> Option<StatusAccountDisplay> {
    Some(StatusAccountDisplay::ChatGpt {
        email: email.map(str::to_string),
        plan: Some("Pro".to_string()),
        workspace: None,
    })
}

fn usage(used: i32) -> RateLimitSnapshot {
    serde_json::from_value(serde_json::json!({
        "limitId": "codex",
        "primary": { "usedPercent": 18, "windowDurationMins": 300, "resetsAt": 2_000_000_000_i64 },
        "secondary": { "usedPercent": used, "windowDurationMins": 10_080, "resetsAt": 2_000_080_000_i64 }
    }))
    .expect("account usage")
}

#[test]
fn weekly_history_and_reset_expiries_are_explained_without_claiming_unknown_usage() {
    let _scope = scope();
    let now = 1_800_000_000;
    let mut history = codex_alab_tui::quota::History::default();
    for (elapsed, used_percent) in [(0, 78.0), (900, 80.0), (1800, 82.0)] {
        assert!(history.observe(codex_alab_tui::quota::WeeklySnapshot {
            used_percent,
            observed_at: now + elapsed,
            resets_at: now + 86_400,
            window_minutes: 10_080,
        }));
    }
    interface::set_status(InterfaceStatus {
        account: "verified@example.test".into(),
        plan: Some("Pro 500".into()),
        weekly: Some("weekly 18% left".into()),
        credit_balance: Some(interface::CreditBalance::Current(
            "100.27 credits left".into(),
        )),
        thread_credit_usage: Some(interface::ChatCreditUsage {
            summary: interface::ChatCreditSummary {
                text: "0.125 credits used in this chat".into(),
                limit_comparison: Some("weekly 1.2%".into()),
                credit_activity: true,
            },
            as_of: Some(now),
            retained: false,
        }),
        weekly_reset: Some("21 Oct 17:00 PDT".into()),
        quota: history.view(now + 1800),
        reset_credits: Some(interface::ResetCredits {
            available_count: 3,
            expiring_count: 1,
            expired_count: 0,
            next_expiry: Some("22 Oct 09:30 PDT".into()),
            stale: false,
            details_known: true,
        }),
        ..Default::default()
    });
    let paragraph = AccountDetailsHeader {
        clock_format: ClockFormat::TwentyFourHour,
        show_account_pool: false,
    }
    .paragraph();
    let area = Rect::new(0, 0, 100, paragraph.desired_height(100));
    let mut buffer = Buffer::empty(area);
    Renderable::render(&paragraph, area, &mut buffer);
    let shown: String = buffer.content.iter().map(Cell::symbol).collect();
    assert!(shown.contains("Plan: Pro 500"));
    assert!(shown.contains("Usage credits"));
    assert!(shown.contains("100.27 credits left"));
    assert!(shown.contains("0.125 credits used in this chat"));
    assert!(shown.contains("Chat usage vs. current limits · weekly 1.2%"));
    assert!(shown.contains("Chat usage reported through"));
    assert!(shown.contains("0–100% used · 3 observations"));
    assert!(shown.contains("· means no observation"));
    assert!(shown.contains("Estimated at reset:"));
    assert!(shown.contains("recent pace"));
    assert!(shown.contains("Estimated limit reached:"));
    assert!(shown.contains("Usage limit resets: 3 available"));
    assert!(shown.contains("22 Oct 09:30 PDT"));
    assert!(shown.contains("1 expiring within 24 hours"));

    let mut status = interface::status_snapshot().unwrap();
    status.usage_stale = true;
    status.reset_credits.as_mut().unwrap().stale = true;
    interface::set_status(status);
    let paragraph = AccountDetailsHeader {
        clock_format: ClockFormat::TwentyFourHour,
        show_account_pool: false,
    }
    .paragraph();
    let area = Rect::new(0, 0, 100, paragraph.desired_height(100));
    let mut buffer = Buffer::empty(area);
    Renderable::render(&paragraph, area, &mut buffer);
    let stale: String = buffer.content.iter().map(Cell::symbol).collect();
    assert!(!stale.contains("Estimated at reset:"));
    assert!(!stale.contains("3 available"));
    assert!(!stale.contains("22 Oct 09:30 PDT"));
    assert!(stale.contains("Usage limit resets: stale"));
}

#[test]
fn reset_inventory_only_explains_relevant_expirations_and_links_to_usage_once() {
    let _scope = scope();
    for (available, expiring, expired, message, missing_expiry) in [
        (0, 0, 0, "", false),
        (2, 0, 0, "", true),
        (0, 0, 1, "1 expired", false),
        (
            3,
            1,
            2,
            "At least 1 expiring within 24 hours · 2 expired",
            true,
        ),
    ] {
        interface::set_status(InterfaceStatus {
            reset_credits: Some(interface::ResetCredits {
                available_count: available,
                expiring_count: expiring,
                expired_count: expired,
                next_expiry: (expiring > 0).then(|| "Thu 16:00".into()),
                stale: false,
                details_known: false,
            }),
            ..Default::default()
        });
        let paragraph = AccountDetailsHeader {
            clock_format: ClockFormat::TwentyFourHour,
            show_account_pool: false,
        }
        .paragraph();
        let area = Rect::new(0, 0, 100, paragraph.desired_height(100));
        let mut buf = Buffer::empty(area);
        Renderable::render(&paragraph, area, &mut buf);
        let rendered: String = buf.content.iter().map(Cell::symbol).collect();
        let (_, resets) = rendered.split_once("Usage limit resets: ").unwrap();
        assert!(
            resets.starts_with(&format!("{available} available")),
            "{resets}"
        );
        assert_eq!(
            resets.contains("expiry: unavailable"),
            missing_expiry,
            "{resets}"
        );
        if message.is_empty() {
            assert_eq!(resets.matches("/usage").count(), 0, "{resets}");
        } else {
            assert!(resets.contains(message), "{resets}");
            assert_eq!(resets.matches("/usage").count(), 1, "{resets}");
        }
    }
}

#[tokio::test]
async fn the_account_inspector_keeps_a_fresh_short_allowance_when_weekly_is_stale() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.alab_open_account_details();
    for (case, weekly_stale, short_stale) in [
        ("both fresh", false, false),
        ("short expired", false, true),
        ("snapshot aged", true, true),
        ("weekly expired", true, false),
    ] {
        interface::set_status(InterfaceStatus {
            account: "verified@example.test".into(),
            plan: Some("Pro 500".into()),
            weekly: Some("weekly 73% left".into()),
            five_hour: Some("5h 82% left".into()),
            usage_stale: weekly_stale,
            five_hour_stale: short_stale,
            ..Default::default()
        });
        let shown = render_bottom_popup(&chat, 100);
        assert_eq!(shown.contains("weekly 73% left"), !weekly_stale, "{case}");
        assert_eq!(shown.contains("Weekly: stale"), weekly_stale, "{case}");
        assert_eq!(shown.contains("5h 82% left"), !short_stale, "{case}");
        assert_eq!(shown.contains("5h: stale"), short_stale, "{case}");
    }
}

#[tokio::test]
async fn inspector_points_to_the_spending_report_for_the_current_account() {
    use codex_protocol::account::PlanType;

    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.requires_openai_auth = true;
    chat.alab_open_account_details();
    for (plan, personal, workspace) in [
        (Some(PlanType::Pro), true, false),
        (Some(PlanType::Team), false, true),
        (Some(PlanType::EnterpriseCbpUsageBased), false, true),
        (Some(PlanType::Edu), false, true),
        (Some(PlanType::Sci), false, true),
        (Some(PlanType::Unknown), false, false),
        (None, false, false),
    ] {
        chat.update_account_state(account(Some("person@example.test")), plan, true, true);
        let shown = render_bottom_popup(&chat, 38)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            shown.contains("View analytics → Chats"),
            personal,
            "{plan:?}"
        );
        assert_eq!(
            shown.contains("Chat spending unreported"),
            personal,
            "{plan:?}"
        );
        assert_eq!(
            shown.contains("View analytics → Credits"),
            workspace,
            "{plan:?}"
        );
    }

    chat.remote_connection = Some(crate::status::remote_connection::RemoteConnectionStatus {
        address: "ws://localhost:8888".into(),
        version: "test".into(),
        is_local_daemon: false,
        is_managed_daemon: false,
    });
    chat.update_account_state(
        account(Some("person@example.test")),
        Some(PlanType::Pro),
        true,
        true,
    );
    assert!(!render_bottom_popup(&chat, 76).contains("View analytics"));
    chat.remote_connection = None;
    chat.update_account_state(
        account(Some("person@example.test")),
        Some(PlanType::Pro),
        true,
        false,
    );
    assert!(!render_bottom_popup(&chat, 76).contains("View analytics"));
    chat.update_account_state(Some(StatusAccountDisplay::ApiKey), None, false, false);
    assert!(!render_bottom_popup(&chat, 76).contains("View analytics"));
}

#[tokio::test]
async fn live_header_uses_full_native_identity_windows_resets_and_clock_preference() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    let email = "person.with.a.long.verified.address@example.com";
    chat.update_account_state(account(Some(email)), None, true, true);
    let mut snapshot = usage(27);
    std::mem::swap(&mut snapshot.primary, &mut snapshot.secondary);
    chat.on_rate_limit_snapshot(Some(snapshot));
    let mut other = usage(99);
    other.limit_id = Some("other-model".to_string());
    chat.on_rate_limit_snapshot(Some(other));
    chat.on_rolling_rate_limit_snapshot(usage(99));
    chat.alab_open_account_details();

    for clock in [ClockFormat::TwelveHour, ClockFormat::TwentyFourHour] {
        chat.clock_format = clock;
        let captured = Local
            .with_ymd_and_hms(2026, 10, 20, 18, 24, 0)
            .single()
            .unwrap();
        chat.rate_limit_snapshots_by_limit_id
            .get_mut("codex")
            .unwrap()
            .captured_at = captured;
        interface::set_status(chat.alab_interface_status_at(captured));
        let status = interface::status_snapshot().unwrap();
        let rendered = render_bottom_popup(&chat, 100);
        assert!(rendered.contains(email), "{rendered}");
        assert!(rendered.contains("Plan: Pro"));
        assert!(rendered.contains("weekly 73% left"));
        assert!(rendered.contains("5h 82% left"));
        assert!(rendered.contains(status.weekly_reset.as_deref().unwrap()));
        assert!(rendered.contains(status.five_hour_reset.as_deref().unwrap()));
        assert!(rendered.contains(&captured.format(clock.date_time_format()).to_string()));
        assert!(!rendered.contains("1% left"));
    }

    let width = 34;
    let header = AccountDetailsHeader {
        clock_format: ClockFormat::TwentyFourHour,
        show_account_pool: false,
    };
    let area = Rect::new(0, 0, width, header.desired_height(width));
    let mut buf = Buffer::empty(area);
    header.render(area, &mut buf);
    let compact: String = buf.content.iter().map(Cell::symbol).collect();
    assert!(
        compact.replace(' ', "").contains(email),
        "the full address wraps"
    );
}

#[tokio::test]
async fn unavailable_credits_precede_past_charges_until_spending_resumes() {
    use codex_alab_tui::banner::Depth;
    use codex_alab_tui::banner::Theme;
    use codex_app_server_protocol::CreditsSnapshot;

    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("person@example.test")), None, true, true);
    chat.alab_open_account_details();

    for (available, balance, detail, narrow) in [
        (
            false,
            Some("12.50"),
            "Unavailable to spend · 12.50 credits",
            "Unavailable to spend",
        ),
        (false, Some("0.00"), "0 credits left", "0 credits left"),
        (
            false,
            None,
            "Unavailable to spend · balance unreported",
            "Unavailable to spend",
        ),
        (
            true,
            Some("12.50"),
            "12.50 credits left",
            "2 credits used in this chat",
        ),
    ] {
        let mut snapshot = usage(100);
        snapshot.credits = Some(CreditsSnapshot {
            has_credits: available,
            unlimited: false,
            balance: balance.map(str::to_owned),
        });
        chat.on_rate_limit_snapshot(Some(snapshot));
        let mut status = interface::status_snapshot().unwrap();
        status.thread_credit_usage = Some(interface::ChatCreditUsage {
            summary: interface::ChatCreditSummary {
                text: "2 credits used in this chat".into(),
                limit_comparison: None,
                credit_activity: true,
            },
            as_of: None,
            retained: false,
        });
        interface::set_status(status);
        let area = Rect::new(0, 0, 30, 1);
        let mut buf = Buffer::empty(area);
        interface::render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        let footer: String = buf.content.iter().map(Cell::symbol).collect();
        assert!(footer.contains(narrow), "{footer}");
        let inspector = render_bottom_popup(&chat, 76);
        assert!(inspector.contains(detail), "{inspector}");
        assert!(
            inspector.contains("2 credits used in this chat"),
            "{inspector}"
        );
    }
}

#[tokio::test]
async fn individual_credit_limit_survives_unreadable_totals_and_replenishes() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("person@example.com")), None, true, true);
    chat.alab_open_account_details();
    let mut snapshot = usage(100);
    snapshot.credits = Some(codex_app_server_protocol::CreditsSnapshot {
        has_credits: true,
        unlimited: false,
        balance: Some("12.50".into()),
    });
    for (remaining, used, limit, expected_balance) in [
        (0, "", "100", "Monthly credit limit reached · 12.50 credits"),
        (
            0,
            "100",
            "NaN",
            "Monthly credit limit reached · 12.50 credits",
        ),
        (40, "60", "100", "12.50 credits left"),
    ] {
        snapshot.individual_limit = Some(codex_app_server_protocol::SpendControlLimitSnapshot {
            used: used.into(),
            limit: limit.into(),
            remaining_percent: remaining,
            resets_at: 2_000_000_000,
        });
        chat.on_rate_limit_snapshot(Some(snapshot.clone()));
        chat.on_rolling_rate_limit_snapshot(usage(100));
        let shown = render_bottom_popup(&chat, 100);
        assert!(shown.contains(expected_balance), "{shown}");
        assert!(
            shown.contains(&format!(
                "Your monthly credit limit · {remaining}% left · resets "
            )),
            "{shown}"
        );
    }
    snapshot.individual_limit.as_mut().unwrap().resets_at = i64::MAX;
    chat.on_rate_limit_snapshot(Some(snapshot.clone()));
    let shown = render_bottom_popup(&chat, 100);
    assert!(
        shown.contains("Your monthly credit limit · 40% left"),
        "{shown}"
    );
    assert!(!shown.contains("40% left · resets"), "{shown}");

    let now = Local::now();
    chat.alab_set_usage_captured_at_for_test(now - chrono::Duration::minutes(16));
    chat.alab_refresh_quota_freshness_at(now);
    assert!(!render_bottom_popup(&chat, 100).contains("Your monthly credit limit"));

    snapshot.individual_limit = None;
    chat.on_rate_limit_snapshot(Some(snapshot));
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("12.50 credits left"), "{shown}");
    assert!(!shown.contains("Your monthly credit limit"), "{shown}");
}

#[tokio::test]
async fn monthly_reset_waits_for_confirmation_without_losing_the_reported_balance() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("person@example.com")), None, true, true);
    chat.alab_open_account_details();
    let reset = Local
        .timestamp_opt(Local::now().timestamp() + 30, 0)
        .unwrap();
    let mut snapshot = usage(27);
    snapshot.credits = Some(codex_app_server_protocol::CreditsSnapshot {
        has_credits: true,
        unlimited: false,
        balance: Some("12.50".into()),
    });
    let (requester, mut frames) = crate::tui::FrameRequester::test_channel();
    chat.frame_requester = requester;
    for (remaining, spend_control, balance) in [
        (0, false, "Last check · 12.50 credits"),
        (40, false, "Last check · 12.50 credits"),
        (
            0,
            true,
            "Last check · Account spending limit reached · 12.50 credits",
        ),
    ] {
        snapshot.spend_control_reached = Some(spend_control);
        snapshot.individual_limit = Some(codex_app_server_protocol::SpendControlLimitSnapshot {
            used: (100 - remaining).to_string(),
            limit: "100".into(),
            remaining_percent: remaining,
            resets_at: reset.timestamp(),
        });
        chat.on_rate_limit_snapshot(Some(snapshot.clone()));
        while frames.try_recv().is_ok() {}
        let earliest = std::time::Instant::now() + std::time::Duration::from_millis(1);
        chat.alab_refresh_quota_freshness_at(reset - chrono::Duration::milliseconds(1));
        let latest = std::time::Instant::now() + std::time::Duration::from_millis(1);
        let scheduled = frames.try_recv().expect("monthly reset frame");
        let before = render_bottom_popup(&chat, 100);
        assert!(
            before.contains(&format!("Your monthly credit limit · {remaining}% left")),
            "{before}"
        );
        chat.alab_refresh_quota_freshness_at(reset);
        let after = render_bottom_popup(&chat, 100);
        assert!(
            after.contains("Your monthly credit limit · refresh needed"),
            "{after}"
        );
        assert!(after.contains(balance), "{after}");
        assert!((earliest..=latest).contains(&scheduled));
        assert!(!after.contains("Monthly credit limit reached"), "{after}");
        assert!(!after.contains("12.50 credits left"), "{after}");
        assert!(after.contains("weekly 73% left"), "{after}");
    }
    snapshot.spend_control_reached = Some(false);
    let limit = snapshot.individual_limit.as_mut().unwrap();
    limit.remaining_percent = 90;
    limit.resets_at += 30 * 86_400;
    chat.on_rate_limit_snapshot(Some(snapshot));
    let refreshed = render_bottom_popup(&chat, 100);
    assert!(
        refreshed.contains("Your monthly credit limit · 90% left"),
        "{refreshed}"
    );
    assert!(refreshed.contains("12.50 credits left"), "{refreshed}");
}

#[tokio::test]
async fn monthly_limit_without_a_balance_ages_after_the_other_windows_have_reset() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("person@example.com")), None, true, true);
    chat.alab_open_account_details();
    let now = Local::now();
    chat.alab_allowance.weekly_resets_at = Some(now.timestamp() - 1);
    chat.alab_allowance.five_hour_resets_at = Some(now.timestamp() - 1);
    let mut snapshot = usage(27);
    snapshot.individual_limit = Some(codex_app_server_protocol::SpendControlLimitSnapshot {
        used: "60".into(),
        limit: "100".into(),
        remaining_percent: 40,
        resets_at: now.timestamp() + 86_400,
    });
    let mut elapsed = snapshot.clone();
    elapsed.individual_limit.as_mut().unwrap().resets_at = now.timestamp() - 1;
    chat.on_rate_limit_snapshot(Some(elapsed));
    let pending = interface::status_snapshot().unwrap();
    assert!(pending.credit_balance.is_none());
    assert_eq!(
        pending.individual_credit_limit,
        Some(interface::IndividualCreditLimit::AwaitingRefresh)
    );
    chat.on_rate_limit_snapshot(Some(snapshot));
    assert!(render_bottom_popup(&chat, 100).contains("Your monthly credit limit · 40% left"));
    chat.alab_set_usage_captured_at_for_test(now - chrono::Duration::minutes(16));
    chat.alab_refresh_quota_freshness_at(now);
    let after = render_bottom_popup(&chat, 100);
    assert!(!after.contains("Your monthly credit limit"), "{after}");
}

#[tokio::test]
async fn workspace_credit_restrictions_survive_sparse_updates_and_clear_on_account_refresh() {
    use codex_app_server_protocol::CreditsSnapshot;
    use codex_app_server_protocol::RateLimitReachedType;

    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("person@example.com")), None, true, true);
    chat.alab_open_account_details();
    let mut snapshot = usage(100);
    snapshot.credits = Some(CreditsSnapshot {
        has_credits: true,
        unlimited: false,
        balance: Some("31068.82776".into()),
    });
    snapshot.spend_control_reached = Some(true);
    for (reason, expected) in [
        (
            RateLimitReachedType::WorkspaceOwnerCreditsDepleted,
            "Workspace out of credits",
        ),
        (
            RateLimitReachedType::WorkspaceMemberCreditsDepleted,
            "Workspace out of credits",
        ),
        (
            RateLimitReachedType::WorkspaceOwnerUsageLimitReached,
            "Workspace spending limit reached",
        ),
        (
            RateLimitReachedType::WorkspaceMemberUsageLimitReached,
            "Member spending limit reached",
        ),
    ] {
        snapshot.rate_limit_reached_type = Some(reason);
        chat.on_rate_limit_snapshot(Some(snapshot.clone()));
        chat.on_rolling_rate_limit_snapshot(usage(100));
        let shown = render_bottom_popup(&chat, 100);
        assert!(
            shown.contains(&format!("{expected} · 31068.82776 credits")),
            "{shown}"
        );
    }
    snapshot.spend_control_reached = Some(false);
    snapshot.rate_limit_reached_type = Some(RateLimitReachedType::RateLimitReached);
    chat.on_rate_limit_snapshot(Some(snapshot.clone()));
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("31068.82776 credits left"), "{shown}");
    assert!(!shown.contains("spending limit reached"), "{shown}");

    snapshot.credits = None;
    snapshot.spend_control_reached = Some(true);
    chat.on_rate_limit_snapshot(Some(snapshot.clone()));
    let shown = render_bottom_popup(&chat, 100);
    assert!(
        shown.contains("Account spending limit reached · balance unreported"),
        "{shown}"
    );

    snapshot.credits = Some(CreditsSnapshot {
        has_credits: false,
        unlimited: false,
        balance: Some("0".into()),
    });
    chat.on_rate_limit_snapshot(Some(snapshot.clone()));
    let shown = render_bottom_popup(&chat, 100);
    assert!(
        shown.contains("Account spending limit reached · 0 credits"),
        "{shown}"
    );

    let now = Local::now();
    for (balance, unlimited, expected) in [
        (Some("0"), false, "Stale balance · 0 credits"),
        (
            Some("31068.82776"),
            false,
            "Stale balance · 31068.82776 credits",
        ),
        (None, true, "Stale balance · unlimited credits"),
        (None, false, "credit balance stale"),
    ] {
        snapshot.credits = Some(CreditsSnapshot {
            has_credits: false,
            unlimited,
            balance: balance.map(str::to_owned),
        });
        chat.on_rate_limit_snapshot(Some(snapshot.clone()));
        chat.alab_set_usage_captured_at_for_test(now - chrono::Duration::minutes(16));
        chat.alab_refresh_quota_freshness_at(now);
        let shown = render_bottom_popup(&chat, 100);
        assert!(shown.contains(expected), "{shown}");
        assert!(!shown.contains("spending limit reached"), "{shown}");
    }
}

#[tokio::test]
async fn open_header_replaces_account_data_and_native_snapshot_removal_clears_usage() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("first@example.com")), None, true, true);
    let mut first = usage(27);
    first.credits = Some(codex_app_server_protocol::CreditsSnapshot {
        has_credits: true,
        unlimited: false,
        balance: Some("31068.82776".into()),
    });
    chat.on_rate_limit_snapshot(Some(first));
    chat.alab_open_account_details();
    assert!(render_bottom_popup(&chat, 100).contains("weekly 73% left"));

    assert!(render_bottom_popup(&chat, 100).contains("31068.82776 credits left"));

    chat.update_account_state(account(None), None, true, true);
    let loading = render_bottom_popup(&chat, 100);
    assert!(!loading.contains("first@example.com"));
    assert!(!loading.contains("31068.82776"));
    assert!(!loading.contains("73%"));
    assert!(loading.contains("Account unavailable"));
    assert!(loading.contains("Usage checked: Unavailable"));
    chat.on_account_email_loaded(Some("second@example.com".to_string()), None);
    assert!(render_bottom_popup(&chat, 100).contains("second@example.com"));
    chat.on_rate_limit_snapshot(Some(usage(61)));
    assert!(render_bottom_popup(&chat, 100).contains("weekly 39% left"));
    chat.on_rate_limit_snapshot(None);
    let failed = render_bottom_popup(&chat, 100);
    assert!(failed.contains("Weekly: unavailable"));
    assert!(failed.contains("5h: unavailable"));
    assert!(failed.contains("Usage checked: Unavailable"));
    assert!(!failed.contains("39%"));
}

#[tokio::test]
async fn stale_header_keeps_identity_and_checked_time_without_presenting_expired_resets() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("person@example.com")), None, true, true);
    chat.on_rate_limit_snapshot(Some(usage(27)));
    let now = Local::now();
    let captured = now - chrono::Duration::minutes(16);
    chat.alab_set_usage_captured_at_for_test(captured);
    chat.alab_open_account_details();
    interface::set_status(chat.alab_interface_status_at(now));
    let status = interface::status_snapshot().unwrap();
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("person@example.com"));
    assert!(shown.contains("Plan: Pro"));
    assert!(shown.contains("Weekly: stale · refresh needed"));
    assert!(shown.contains("5h: stale · refresh needed"));
    assert!(shown.contains(&format!(
        "Usage checked: {}",
        status.usage_checked_at.as_deref().unwrap()
    )));
    assert!(!shown.contains("73%"));
    assert!(!shown.contains("82%"));
    assert!(!shown.contains("resets"));
    assert!(status.weekly_reset.is_some());
    assert!(status.five_hour_reset.is_some());

    // Unknown windows remain unavailable even when their source snapshot is old.
    let mut unknown = usage(27);
    unknown.primary.as_mut().unwrap().window_duration_mins = Some(30);
    unknown.secondary.as_mut().unwrap().window_duration_mins = None;
    chat.on_rate_limit_snapshot(Some(unknown));
    chat.alab_set_usage_captured_at_for_test(captured);
    interface::set_status(chat.alab_interface_status_at(now));
    let unavailable = render_bottom_popup(&chat, 100);
    assert!(unavailable.contains("Weekly: unavailable"));
    assert!(unavailable.contains("5h: unavailable"));
    assert!(!unavailable.contains("stale"));
}

#[tokio::test]
async fn updates_preserve_native_list_state_and_never_reopen_a_closed_popup() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("person@example.com")), None, true, true);
    chat.alab_open_account_details();
    let selected = chat.bottom_pane.selected_index_for_active_view(VIEW_ID);
    chat.bottom_pane.key_chord_reset_requested = false;
    for used in [27, 28, 29] {
        chat.on_rate_limit_snapshot(Some(usage(used)));
        chat.alab_refresh_interface();
        assert_eq!(
            chat.bottom_pane.selected_index_for_active_view(VIEW_ID),
            selected
        );
        assert!(
            !chat.bottom_pane.key_chord_reset_requested,
            "live data must not reset list chords"
        );
    }
    chat.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!chat.bottom_pane.has_active_view());
    chat.on_rate_limit_snapshot(Some(usage(30)));
    chat.alab_refresh_account_details();
    assert!(!chat.bottom_pane.has_active_view());
}

#[tokio::test]
async fn inspection_prefetches_once_and_native_remapped_cancel_preserves_the_draft() {
    let _scope = scope();
    let (mut chat, _tx, mut rx, mut op_rx) = make_chatwidget_manual_with_sender().await;
    chat.requires_openai_auth = true;
    chat.update_account_state(account(Some("person@example.com")), None, true, true);
    chat.apply_external_edit("unfinished prompt".to_string());
    let mut keymap = RuntimeKeymap::defaults();
    keymap.list.cancel = vec![crate::key_hint::plain(KeyCode::Char('q'))];
    chat.bottom_pane.set_keymap_bindings(&keymap);
    while rx.try_recv().is_ok() {}
    while op_rx.try_recv().is_ok() {}
    chat.alab_open_account_details();
    chat.alab_open_account_details();
    assert!(
        chat.alab_account_details_is_active(),
        "interactive account reads request reset expiry details"
    );
    chat.alab_refresh_interface();
    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::RefreshRateLimits {
            origin: RateLimitRefreshOrigin::Periodic
        })
    ));
    assert!(
        rx.try_recv().is_err(),
        "an already-open picker does not prefetch again or write history"
    );
    chat.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(chat.bottom_pane.active_view_id(), Some(VIEW_ID));
    chat.handle_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(!chat.bottom_pane.has_active_view());
    assert!(
        !chat.alab_account_details_is_active(),
        "background reads omit reset expiry details after inspection closes"
    );
    assert_eq!(chat.composer_text_with_pending(), "unfinished prompt");
    assert!(
        op_rx.try_recv().is_err(),
        "inspection cannot submit model input"
    );
    assert!(rx.try_recv().is_err(), "closing does not create history");
}

#[tokio::test]
async fn api_key_unknown_and_disabled_interfaces_do_not_fetch_subscription_usage() {
    let _scope = scope();
    let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    for account in [Some(StatusAccountDisplay::ApiKey), None] {
        chat.update_account_state(account, None, false, false);
        while rx.try_recv().is_ok() {}
        chat.alab_open_account_details();
        let shown = render_bottom_popup(&chat, 100);
        assert!(shown.contains("Plan: Unavailable"));
        assert!(shown.contains("Weekly: unavailable"));
        assert!(shown.contains("Usage checked: Unavailable"));
        assert!(rx.try_recv().is_err());
        chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!chat.bottom_pane.has_active_view());
    }
    interface::set_enabled(false);
    chat.alab_open_account_details();
    assert!(!chat.bottom_pane.has_active_view());
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn an_unknown_duration_is_never_relabelled_as_a_week_or_five_hours() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("person@example.com")), None, true, true);
    let mut snapshot = usage(27);
    snapshot.primary.as_mut().unwrap().window_duration_mins = Some(30);
    snapshot.secondary.as_mut().unwrap().window_duration_mins = None;
    chat.on_rate_limit_snapshot(Some(snapshot));
    chat.alab_open_account_details();
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("Weekly: unavailable"));
    assert!(shown.contains("5h: unavailable"));
    assert!(!shown.contains("73%"));
    assert!(!shown.contains("82%"));
}

fn draw(view: &DetailsView, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    view.render(area, &mut buffer);
    buffer
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(Cell::symbol).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn compact(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn visible_details(rendered: &str) -> String {
    rendered
        .lines()
        .skip(1)
        .take_while(|line| !line.contains("› Close"))
        .collect::<String>()
}

#[cfg(unix)]
#[tokio::test]
async fn accounts_pane_tracks_the_connected_identity_separately_from_its_roster() {
    use codex_alab_tui::accounts_ui::Destination;
    use codex_alab_tui::accounts_ui::Snapshot;
    use codex_alab_tui::accounts_ui::Update;

    let _scope = scope();
    let (mut chat, _tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    chat.requires_openai_auth = true;
    chat.update_account_state(account(Some("moon.2@example.test")), None, true, true);
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let view = chat.alab_accounts_view();
    let owner = view.upgrade().unwrap();
    let token = owner.lock().unwrap().begin_snapshot();
    let roster = "main · m***@example.test\nspare · m***@example.test";
    let destination = Destination {
        label: "spare".into(),
        masked_email: Some("m***@example.test".into()),
        id: "spare-id".into(),
        selection_ref: "d".repeat(64),
        revision: "revision".into(),
    };
    chat.alab_accounts_ready(
        view.clone(),
        chat.thread_id(),
        Update::Snapshot {
            token,
            result: Ok(Snapshot {
                text: roster.into(),
                cached_text: None,
                shared_mask: true,
                address_revision: Some(destination.revision.as_str().into()),
                destinations: vec![destination.clone()],
            }),
        },
    );
    let shown = render_bottom_popup(&chat, 100);
    assert!(
        shown.contains("Connected server account: moon.2@example.test"),
        "{shown}"
    );
    assert!(shown.contains("main · m***@example.test"), "{shown}");
    assert!(!shown.contains("/status"), "{shown}");

    for (state, message) in [
        (
            interface::ConnectionState::Reconnecting,
            "Reconnecting to the app server",
        ),
        (
            interface::ConnectionState::Offline,
            "The app server is offline",
        ),
    ] {
        interface::set_connection_state(state);
        let shown = render_bottom_popup(&chat, 100);
        assert!(shown.contains(message), "{shown}");
        assert!(shown.contains("main · m***@example.test"), "{shown}");
        assert!(shown.contains("spare · m***@example.test"), "{shown}");
        assert!(!shown.contains("/status"), "{shown}");
        assert!(!shown.contains("moon.2@example.test"), "{shown}");
    }
    interface::set_connection_state(interface::ConnectionState::Connected);
    let shown = render_bottom_popup(&chat, 100);
    assert!(
        shown.contains("Connected server account: moon.2@example.test"),
        "{shown}"
    );
    assert!(
        !shown.contains("The connected email is unavailable."),
        "{shown}"
    );

    let status_hint = "Use /status for this session's account details.";
    for (identity, has_subscription) in [
        (Some(StatusAccountDisplay::ApiKey), false),
        (account(None), true),
    ] {
        chat.update_account_state(identity, None, has_subscription, has_subscription);
        let shown = render_bottom_popup(&chat, 100);
        assert_eq!(shown.matches(status_hint).count(), 1, "{shown}");
        assert!(shown.contains("main · m***@example.test"), "{shown}");
        assert!(shown.contains("spare · m***@example.test"), "{shown}");
        assert!(!shown.contains("Connected server account:"), "{shown}");
        assert!(!shown.contains("moon.2@example.test"), "{shown}");
    }
    chat.on_account_email_loaded(Some("moon@example.test".into()), None);
    let shown = render_bottom_popup(&chat, 100);
    assert!(
        shown.contains("Connected server account: moon@example.test"),
        "{shown}"
    );
    assert!(!shown.contains("moon.2@example.test"), "{shown}");
    assert!(!shown.contains("/status"), "{shown}");
    assert!(shown.contains("spare · m***@example.test"), "{shown}");

    let token = {
        let mut current = owner.lock().unwrap();
        assert_eq!(current.destinations(), &[destination]);
        current.begin_snapshot()
    };
    chat.alab_accounts_ready(
        view,
        chat.thread_id(),
        Update::Snapshot {
            token,
            result: Err("Account information unavailable".into()),
        },
    );
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("Showing previously loaded"), "{shown}");
    assert!(shown.contains("moon@example.test"), "{shown}");
    assert!(!shown.contains("/status"), "{shown}");
    interface::set_enabled(false);
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("spare · m***@example.test"), "{shown}");
    assert_eq!(shown.matches(status_hint).count(), 1, "{shown}");
    assert!(!shown.contains("moon@example.test"), "{shown}");
    assert!(!shown.contains("moon.2@example.test"), "{shown}");
    assert!(owner.lock().unwrap().destinations().is_empty());
    while let Ok(event) = events.try_recv() {
        assert!(!matches!(event, AppEvent::InsertHistoryCell(_)));
    }
    assert!(ops.try_recv().is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn accounts_pane_shows_the_email_only_for_a_connected_local_subscription() {
    let _scope = scope();
    let (mut chat, _tx, _events, _ops) = make_chatwidget_manual_with_sender().await;
    let has_email_line =
        |chat: &ChatWidget| render_bottom_popup(chat, 100).contains("Connected server account:");
    chat.requires_openai_auth = true;
    chat.update_account_state(account(Some("local@example.test")), None, true, true);
    chat.alab_accounts_view();
    assert!(has_email_line(&chat));
    for state in [
        interface::ConnectionState::Reconnecting,
        interface::ConnectionState::Offline,
    ] {
        interface::set_connection_state(state);
        assert!(!has_email_line(&chat));
    }
    interface::set_connection_state(interface::ConnectionState::Connected);
    assert!(has_email_line(&chat));
    interface::set_enabled(false);
    assert!(!has_email_line(&chat));
    interface::set_enabled(true);
    chat.alab_refresh_interface();
    assert!(has_email_line(&chat));
    chat.update_account_state(Some(StatusAccountDisplay::ApiKey), None, false, false);
    assert!(!has_email_line(&chat));
    chat.requires_openai_auth = false;
    chat.update_account_state(account(Some("other@example.test")), None, true, true);
    assert!(!has_email_line(&chat));
    chat.requires_openai_auth = true;
    for is_local_daemon in [false, true] {
        chat.remote_connection = Some(crate::status::remote_connection::RemoteConnectionStatus {
            address: "ws://localhost:8888".into(),
            version: "test".into(),
            is_local_daemon,
            is_managed_daemon: false,
        });
        chat.update_account_state(account(Some("external@example.test")), None, true, true);
        assert!(!has_email_line(&chat));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn subscription_pool_hint_requires_a_local_session_under_aterm() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    let render = |chat: &ChatWidget, aterm_session| {
        let view = account_view(
            chat.bottom_pane.list_keymap(),
            chat.clock_format,
            chat.alab_show_account_pool_hint(aterm_session),
        );
        draw(&view, 90, 24)
    };
    assert!(render(&chat, true).contains("Use /accounts"));
    assert!(!render(&chat, false).contains("/accounts"));
    chat.remote_connection = Some(crate::status::remote_connection::RemoteConnectionStatus {
        address: "wss://remote.example".into(),
        version: "test".into(),
        is_local_daemon: false,
        is_managed_daemon: false,
    });
    assert!(!render(&chat, true).contains("/accounts"));
}

#[tokio::test]
async fn short_viewports_can_scroll_every_detail_with_remapped_native_navigation() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    let email = "person.with.a.long.verified.address@example.com";
    chat.update_account_state(account(Some(email)), None, true, true);
    chat.on_rate_limit_snapshot(Some(usage(27)));
    let status = interface::status_snapshot().unwrap();
    let mut keymap = RuntimeKeymap::defaults().list;
    keymap.page_down = vec![crate::key_hint::plain(KeyCode::F(8))];
    keymap.jump_bottom = vec![crate::key_hint::plain(KeyCode::F(9))];
    keymap.jump_top = vec![crate::key_hint::plain(KeyCode::F(10))];
    for height in [4, 8, 10, 12] {
        let mut view = account_view(keymap.clone(), ClockFormat::TwentyFourHour, false);
        let mut seen = visible_details(&draw(&view, 46, height));
        assert!(view.scroll_state().1 >= 1);
        view.handle_key_event(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(
            view.scroll_state().0,
            0,
            "configured page keys take precedence"
        );
        while view.scroll_state().0 < view.scroll_state().2 {
            view.handle_key_event(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
            seen.push_str(&visible_details(&draw(&view, 46, height)));
        }
        let seen = compact(&seen);
        for expected in [
            email,
            "Plan: Pro",
            "weekly 73% left",
            "5h 82% left",
            status.weekly_reset.as_deref().unwrap(),
            status.five_hour_reset.as_deref().unwrap(),
            status.usage_checked_at.as_deref().unwrap(),
        ] {
            assert!(
                seen.contains(&compact(expected)),
                "missing {expected} at 46×{height}: {seen}"
            );
        }
        view.handle_key_event(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        assert_eq!(view.scroll_state().0, 0);
        view.handle_key_event(KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE));
        assert_eq!(view.scroll_state().0, view.scroll_state().2);
        let offset = view.scroll_state().0;
        chat.alab_refresh_interface();
        draw(&view, 46, height);
        assert_eq!(
            view.scroll_state().0,
            offset,
            "async refresh preserves the scroll position"
        );
        draw(&view, 46, 30);
        assert_eq!(
            view.scroll_state().0,
            0,
            "a taller viewport clamps the scroll offset"
        );
    }
}

#[test]
fn native_completion_and_disconnected_input_contracts_are_preserved() {
    let mut keymap = RuntimeKeymap::defaults().list;
    keymap.accept = vec![crate::key_hint::plain(KeyCode::F(11))];
    keymap.cancel = vec![crate::key_hint::plain(KeyCode::Char('q'))];
    for (code, expected) in [
        (KeyCode::F(11), ViewCompletion::Accepted),
        (KeyCode::Char('q'), ViewCompletion::Cancelled),
    ] {
        let mut view = account_view(keymap.clone(), ClockFormat::TwentyFourHour, false);
        assert!(view.accepts_input_when_disconnected());
        assert!(view.prefer_esc_to_handle_key_event());
        view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        view.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!view.is_complete());
        view.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(view.is_complete());
        assert_eq!(view.completion(), Some(expected));
    }
}
