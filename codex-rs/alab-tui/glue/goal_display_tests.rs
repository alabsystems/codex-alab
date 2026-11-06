// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Drive actual goal notifications through the displayed widget and frame hook.

use crate::app::tests::make_test_app_with_channels;
use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Theme;
use codex_alab_tui::interface;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadGoalUpdatedNotification;
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::Rect;

fn shown() -> String {
    let area = Rect::new(0, 0, 100, interface::status_height());
    let mut buf = Buffer::empty(area);
    interface::render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    buf.content.iter().map(Cell::symbol).collect()
}

#[tokio::test]
async fn active_goal_row_tracks_native_updates_and_every_stopped_status() {
    let state = Box::leak(Box::new(interface::State::new()));
    let _scope = interface::scoped(state);
    interface::configure(true);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    app.chat_widget
        .set_feature_enabled(crate::Feature::Goals, true);
    for status in [
        ThreadGoalStatus::Active,
        ThreadGoalStatus::Paused,
        ThreadGoalStatus::Blocked,
        ThreadGoalStatus::UsageLimited,
        ThreadGoalStatus::BudgetLimited,
        ThreadGoalStatus::Complete,
    ] {
        app.chat_widget.handle_server_notification(
            ServerNotification::ThreadGoalUpdated(ThreadGoalUpdatedNotification {
                thread_id: "thread-1".into(),
                turn_id: None,
                goal: ThreadGoal {
                    thread_id: "thread-1".into(),
                    objective: "Build Trust with Trust".into(),
                    status,
                    token_budget: None,
                    tokens_used: 0,
                    time_used_seconds: 0,
                    created_at: 0,
                    updated_at: 0,
                },
            }),
            None,
        );
        super::observe(&app);
        assert_eq!(
            shown().contains("Goal: Build Trust with Trust"),
            status == ThreadGoalStatus::Active,
            "{status:?}"
        );
    }
}

#[tokio::test]
async fn active_goal_row_clears_when_goals_are_disabled_or_the_displayed_widget_changes() {
    let state = Box::leak(Box::new(interface::State::new()));
    let _scope = interface::scoped(state);
    interface::configure(true);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    app.chat_widget
        .set_feature_enabled(crate::Feature::Goals, true);
    app.chat_widget.handle_server_notification(
        ServerNotification::ThreadGoalUpdated(ThreadGoalUpdatedNotification {
            thread_id: "thread-1".into(),
            turn_id: None,
            goal: ThreadGoal {
                thread_id: "thread-1".into(),
                objective: "First thread objective".into(),
                status: ThreadGoalStatus::Active,
                token_budget: None,
                tokens_used: 0,
                time_used_seconds: 0,
                created_at: 0,
                updated_at: 0,
            },
        }),
        None,
    );
    super::observe(&app);
    assert!(shown().contains("First thread objective"));
    app.chat_widget
        .set_feature_enabled(crate::Feature::Goals, false);
    super::observe(&app);
    assert!(!shown().contains("First thread objective"));

    interface::set_goal(Some("Prior displayed thread"));
    let (mut other, _events, _ops) = make_test_app_with_channels().await;
    std::mem::swap(&mut app.chat_widget, &mut other.chat_widget);
    super::observe(&app);
    assert!(!shown().contains("Prior displayed thread"));
}
