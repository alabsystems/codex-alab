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
    }
    .paragraph();
    let area = Rect::new(0, 0, 100, paragraph.desired_height(100));
    let mut buffer = Buffer::empty(area);
    Renderable::render(&paragraph, area, &mut buffer);
    let shown: String = buffer.content.iter().map(Cell::symbol).collect();
    assert!(shown.contains("Plan: Pro 500"));
    assert!(shown.contains("0–100% used · 3 observations"));
    assert!(shown.contains("· means no observation"));
    assert!(shown.contains("Estimated at reset:"));
    assert!(shown.contains("recent pace"));
    assert!(shown.contains("Estimated limit reached:"));
    assert!(shown.contains("Usage limit resets: 3 available"));
    assert!(shown.contains("22 Oct 09:30 PDT"));
    assert!(shown.contains("expire within 24 hours"));

    let mut status = interface::status_snapshot().unwrap();
    status.usage_stale = true;
    status.reset_credits.as_mut().unwrap().stale = true;
    interface::set_status(status);
    let paragraph = AccountDetailsHeader {
        clock_format: ClockFormat::TwentyFourHour,
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
async fn open_header_replaces_account_data_and_native_snapshot_removal_clears_usage() {
    let _scope = scope();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.update_account_state(account(Some("first@example.com")), None, true, true);
    chat.on_rate_limit_snapshot(Some(usage(27)));
    chat.alab_open_account_details();
    assert!(render_bottom_popup(&chat, 100).contains("weekly 73% left"));

    chat.update_account_state(account(None), None, true, true);
    let loading = render_bottom_popup(&chat, 100);
    assert!(!loading.contains("first@example.com"));
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
        chat.alab_account_details_is_open(),
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
        !chat.alab_account_details_is_open(),
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
        let mut view = account_view(keymap.clone(), ClockFormat::TwentyFourHour);
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
        let mut view = account_view(keymap.clone(), ClockFormat::TwentyFourHour);
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
