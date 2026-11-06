// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::owned_transcript::tests::attach_thread;
use crate::app::tests::make_test_app_with_channels;
use crate::chatwidget::ChatWidget;
use crate::status::StatusAccountDisplay;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;

fn scope() -> interface::Scope {
    let state = Box::leak(Box::new(interface::State::new()));
    let scope = interface::scoped(state);
    interface::configure(true);
    scope
}

fn account(email: &str) -> Option<StatusAccountDisplay> {
    Some(StatusAccountDisplay::ChatGpt {
        email: Some(email.into()),
        plan: Some("Pro".into()),
        workspace: None,
    })
}

fn usage(used: i32) -> RateLimitSnapshot {
    serde_json::from_value(serde_json::json!({
        "limitId": "codex",
        "primary": {"usedPercent": 18, "windowDurationMins": 300},
        "secondary": {"usedPercent": used, "windowDurationMins": 10_080}
    }))
    .unwrap()
}

fn take_refresh(events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> AppEvent {
    std::iter::from_fn(|| events.try_recv().ok())
        .find(|event| {
            matches!(
                event,
                AppEvent::RefreshRateLimits {
                    origin: crate::app_event::RateLimitRefreshOrigin::Periodic
                }
            )
        })
        .expect("replacement requests an immediate native quota refresh")
}

#[tokio::test]
async fn parked_thread_routing_cannot_replace_the_selected_account_quota_or_workspace()
-> color_eyre::eyre::Result<()> {
    let _scope = scope();
    let (mut app, mut events, mut ops) = make_test_app_with_channels().await;
    let voice = ThreadId::from_u128(881);
    let selected = ThreadId::from_u128(882);
    attach_thread(&mut app, voice);
    app.active_thread_id = Some(voice);
    app.primary_thread_id = Some(voice);
    app.chat_widget
        .update_account_state(account("old@example.test"), None, true, true);
    app.chat_widget.on_rate_limit_snapshot(Some(usage(28)));
    crate::chatwidget::activate_voice_for_thread(&mut app.chat_widget, voice);
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let init = app.chatwidget_init_for_forked_or_resumed_thread(&mut tui, app.config.clone(), None);
    app.replace_chat_widget(ChatWidget::new_with_app_event(init));
    assert_eq!(
        app.background_voice.as_ref().unwrap().thread_id(),
        Some(voice)
    );
    attach_thread(&mut app, selected);
    app.active_thread_id = Some(selected);
    app.chat_widget
        .update_account_state(account("current@example.test"), None, true, true);
    app.chat_widget.on_rate_limit_snapshot(Some(usage(61)));
    let current = interface::status_snapshot().unwrap();
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}

    app.enqueue_thread_notification(
        voice,
        ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: voice.to_string(),
            turn: Turn {
                id: "parked-turn".into(),
                root_turn_id: None,
                items_view: TurnItemsView::Full,
                items: Vec::new(),
                status: TurnStatus::InProgress,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
            },
        }),
    )
    .await?;

    assert!(
        app.background_voice
            .as_ref()
            .unwrap()
            .is_task_running_for_test(),
        "the real parked owner processed its turn notification"
    );
    assert_eq!(interface::status_snapshot(), Some(current));
    assert_eq!(app.chat_widget.thread_id(), Some(selected));
    assert!(
        ops.try_recv().is_err(),
        "status publication cannot submit model work"
    );
    Ok(())
}

#[tokio::test]
async fn replacement_queues_native_refresh_and_coalesces_pending_reads_without_copying_old_usage()
-> color_eyre::eyre::Result<()> {
    let _scope = scope();
    let (mut app, mut events, mut ops) = make_test_app_with_channels().await;
    app.chat_widget.requires_openai_auth = true;
    app.chat_widget
        .update_account_state(account("selected@example.test"), None, true, true);
    app.chat_widget.on_rate_limit_snapshot(Some(usage(28)));
    app.chat_widget.hold_rate_limit_recovery();
    let (request_id, hard_stop_generation) = app
        .rate_limit_refresh_state
        .start(
            crate::app_event::RateLimitRefreshOrigin::Periodic,
            &mut app.rate_limit_hard_stop_generation,
        )
        .unwrap();
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let init = app.chatwidget_init_for_forked_or_resumed_thread(&mut tui, app.config.clone(), None);
    app.replace_chat_widget(ChatWidget::new_with_app_event(init));
    let loading = interface::status_snapshot().unwrap();
    assert_eq!(loading.account, "selected@example.test");
    assert_eq!(
        loading.weekly, None,
        "no previous widget quota is copied into a new native cache"
    );
    assert_eq!(loading.usage_checked_at, None);
    assert!(
        !app.chat_widget.hold_rate_limit_recovery(),
        "the old widget's recovery hold is not transferred"
    );
    app.chat_widget.finish_rate_limit_recovery();
    let refresh = take_refresh(&mut events);
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    app.handle_event(&mut tui, &mut server, refresh).await?;
    assert!(
        app.rate_limit_refresh_state
            .start(
                crate::app_event::RateLimitRefreshOrigin::Periodic,
                &mut app.rate_limit_hard_stop_generation,
            )
            .is_none(),
        "the original in-flight native read is still the single read"
    );
    app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::RateLimitsLoaded {
            request_id,
            origin: crate::app_event::RateLimitRefreshOrigin::Periodic,
            hard_stop_generation,
            result: Ok(GetAccountRateLimitsResponse {
                ordinary_usage_allowed: Some(true),
                account_id: None,
                rate_limit_upsell: None,
                rate_limits: usage(61),
                rate_limits_by_limit_id: None,
                rate_limit_reset_credits: None,
            }),
        },
    )
    .await?;
    let refreshed = interface::status_snapshot().unwrap();
    assert_eq!(refreshed.weekly.as_deref(), Some("weekly 39% left"));
    assert!(refreshed.usage_checked_at.is_some());
    assert_eq!(refreshed.account, loading.account);
    assert!(ops.try_recv().is_err());
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn replacement_does_not_add_quota_reads_to_native_off_or_api_key_sessions()
-> color_eyre::eyre::Result<()> {
    let _scope = scope();
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    for chatgpt in [true, false] {
        app.chat_widget.requires_openai_auth = true;
        app.chat_widget.update_account_state(
            if chatgpt {
                account("selected@example.test")
            } else {
                Some(StatusAccountDisplay::ApiKey)
            },
            None,
            chatgpt,
            chatgpt,
        );
        interface::set_enabled(!chatgpt);
        while events.try_recv().is_ok() {}
        let init =
            app.chatwidget_init_for_forked_or_resumed_thread(&mut tui, app.config.clone(), None);
        app.replace_chat_widget(ChatWidget::new_with_app_event(init));
        assert!(
            !std::iter::from_fn(|| events.try_recv().ok())
                .any(|event| matches!(event, AppEvent::RefreshRateLimits { .. }))
        );
    }
    Ok(())
}
