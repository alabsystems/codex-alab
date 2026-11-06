// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual App backend dispatch controls. Session selection uses the native widget's session
//! handler; lifecycle notifications enter through handle_polled_backend_event, never State.

use super::*;
use crate::app::tests::make_test_app_with_channels;
use crate::app::tests::test_thread_session;
use crate::app_server_session::ThreadParamsMode;
use codex_alab_tui::boxes;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::CloudClient;
use codex_app_server_protocol::ThreadClosedNotification;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalClearedNotification;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadGoalUpdatedNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;

fn backend() -> AppServerSession {
    let (client, _, _) = CloudClient::channel(/*capacity*/ 4);
    AppServerSession::new(
        BackendId::new(),
        AppServerClient::Cloud(client),
        ThreadParamsMode::Cloud,
    )
}

fn select(app: &mut App, owner: BackendId, thread: ThreadId) {
    app.alab_backend_bookkeeping.displayed(owner);
    let session = test_thread_session(thread, app.config.cwd.to_path_buf());
    app.active_thread_id = Some(thread);
    app.primary_thread_id = Some(thread);
    app.chat_widget.handle_thread_session_quiet(session);
}

fn turn(id: &str, status: TurnStatus) -> Turn {
    Turn {
        id: id.to_owned(),
        root_turn_id: None,
        items_view: TurnItemsView::Full,
        items: Vec::new(),
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn started(thread: ThreadId, id: &str) -> AppServerEvent {
    AppServerEvent::ServerNotification(Box::new(ServerNotification::TurnStarted(
        TurnStartedNotification {
            thread_id: thread.to_string(),
            turn: turn(id, TurnStatus::InProgress),
        },
    )))
}

fn completed(thread: ThreadId, id: &str) -> AppServerEvent {
    AppServerEvent::ServerNotification(Box::new(ServerNotification::TurnCompleted(
        TurnCompletedNotification {
            thread_id: thread.to_string(),
            turn: turn(id, TurnStatus::Completed),
        },
    )))
}

async fn poll(
    app: &mut App,
    session: &AppServerSession,
    displayed: BackendId,
    owner: BackendId,
    event: AppServerEvent,
) {
    app.handle_polled_backend_event(
        session,
        displayed,
        AppServerSessionEvent {
            backend_id: owner,
            event,
        },
    )
    .await;
}

#[tokio::test]
async fn parked_completion_matches_backend_and_exact_turn_without_changing_the_displayed_ui() {
    let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
    let _scope = boxes::scoped(shared);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let a = backend();
    let b = backend();
    let shown_a = ThreadId::new();
    let side_a = ThreadId::new();
    select(&mut app, a.backend_id(), shown_a);
    app.ensure_thread_channel(side_a);
    for id in ["older", "newer"] {
        poll(
            &mut app,
            &a,
            a.backend_id(),
            a.backend_id(),
            started(side_a, id),
        )
        .await;
    }

    // The main loop now supplies B as displayed; A's retained side channel still belongs to A.
    let shown_b = ThreadId::new();
    select(&mut app, b.backend_id(), shown_b);
    app.chat_widget.insert_str("keep B's draft");
    let before = app.chat_widget.composer_text_with_pending();
    assert!(shared.turn_seen_besides(Some(&shown_b.to_string())));

    // An envelope claiming A on B's actual session is refused before ALab bookkeeping too.
    for id in ["older", "newer"] {
        poll(
            &mut app,
            &b,
            b.backend_id(),
            a.backend_id(),
            completed(side_a, id),
        )
        .await;
    }
    assert!(shared.turn_seen_besides(Some(&shown_b.to_string())));
    poll(
        &mut app,
        &a,
        b.backend_id(),
        a.backend_id(),
        completed(side_a, "older"),
    )
    .await;
    assert!(
        shared.turn_seen_besides(Some(&shown_b.to_string())),
        "an older completion must not retire the newer turn"
    );
    poll(
        &mut app,
        &a,
        b.backend_id(),
        a.backend_id(),
        completed(side_a, "newer"),
    )
    .await;
    assert!(!shared.turn_seen_besides(Some(&shown_b.to_string())));
    assert_eq!(app.current_displayed_thread_id(), Some(shown_b));
    assert_eq!(app.chat_widget.composer_text_with_pending(), before);
    assert!(app.alab_backend_bookkeeping.seen.borrow().is_empty());
}

#[tokio::test]
async fn parked_close_lag_and_disconnect_retire_only_their_observed_backend() {
    for loss in [
        AppServerEvent::Lagged { skipped: 1 },
        AppServerEvent::Disconnected {
            message: "owned parked stream ended".into(),
        },
    ] {
        let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
        let _scope = boxes::scoped(shared);
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        let a = backend();
        let b = backend();
        let side_a = ThreadId::new();
        select(&mut app, a.backend_id(), ThreadId::new());
        app.ensure_thread_channel(side_a);
        poll(
            &mut app,
            &a,
            a.backend_id(),
            a.backend_id(),
            started(side_a, "A"),
        )
        .await;
        let shown_b = ThreadId::new();
        let side_b = ThreadId::new();
        select(&mut app, b.backend_id(), shown_b);
        app.ensure_thread_channel(side_b);
        poll(
            &mut app,
            &b,
            b.backend_id(),
            b.backend_id(),
            started(side_b, "B"),
        )
        .await;
        app.chat_widget.insert_str("B remains selected");
        let before = app.chat_widget.composer_text_with_pending();
        assert!(shared.turn_seen_besides(Some(&side_b.to_string())));
        poll(&mut app, &a, b.backend_id(), a.backend_id(), loss).await;
        assert!(
            !shared.turn_seen_besides(Some(&side_b.to_string())),
            "A's stream loss retires A"
        );
        assert!(
            shared.turn_seen_besides(Some(&shown_b.to_string())),
            "B's turn survives A's loss"
        );
        assert_eq!(app.current_displayed_thread_id(), Some(shown_b));
        assert_eq!(app.chat_widget.composer_text_with_pending(), before);
        poll(
            &mut app,
            &b,
            b.backend_id(),
            b.backend_id(),
            completed(side_b, "B"),
        )
        .await;
        assert!(!shared.turn_seen_besides(None));
        assert!(app.alab_backend_bookkeeping.seen.borrow().is_empty());
    }
}

#[tokio::test]
async fn reused_native_thread_and_turn_ids_keep_the_other_backends_seen_owner() {
    let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
    let _scope = boxes::scoped(shared);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let a = backend();
    let b = backend();
    let same_thread = ThreadId::new();
    select(&mut app, a.backend_id(), ThreadId::new());
    app.ensure_thread_channel(same_thread);
    poll(
        &mut app,
        &a,
        a.backend_id(),
        a.backend_id(),
        started(same_thread, "same-turn"),
    )
    .await;
    select(&mut app, b.backend_id(), ThreadId::new());
    poll(
        &mut app,
        &b,
        b.backend_id(),
        b.backend_id(),
        started(same_thread, "same-turn"),
    )
    .await;
    poll(
        &mut app,
        &a,
        b.backend_id(),
        a.backend_id(),
        AppServerEvent::ServerNotification(Box::new(ServerNotification::ThreadClosed(
            ThreadClosedNotification {
                thread_id: same_thread.to_string(),
            },
        ))),
    )
    .await;
    assert!(
        shared.turn_seen_besides(None),
        "A close cannot clear the identical key still owned by B"
    );
    poll(
        &mut app,
        &b,
        b.backend_id(),
        b.backend_id(),
        completed(same_thread, "same-turn"),
    )
    .await;
    assert!(!shared.turn_seen_besides(None));
    assert!(app.alab_backend_bookkeeping.seen.borrow().is_empty());
}

fn active_goal(thread: ThreadId, objective: &str) -> ServerNotification {
    goal_status(thread, objective, ThreadGoalStatus::Active)
}

fn goal_status(thread: ThreadId, objective: &str, status: ThreadGoalStatus) -> ServerNotification {
    ServerNotification::ThreadGoalUpdated(ThreadGoalUpdatedNotification {
        thread_id: thread.to_string(),
        turn_id: None,
        goal: ThreadGoal {
            thread_id: thread.to_string(),
            objective: objective.to_owned(),
            status,
            token_budget: None,
            tokens_used: 0,
            time_used_seconds: 0,
            created_at: 0,
            updated_at: 0,
        },
    })
}

fn closed(thread: ThreadId) -> AppServerEvent {
    AppServerEvent::ServerNotification(Box::new(ServerNotification::ThreadClosed(
        ThreadClosedNotification {
            thread_id: thread.to_string(),
        },
    )))
}

#[tokio::test]
async fn parked_close_retires_snapshot_goals_without_erasing_the_displayed_owner() {
    for collision in [false, true] {
        let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
        let _scope = boxes::scoped(shared);
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        app.chat_widget
            .set_feature_enabled(crate::Feature::Goals, /*enabled*/ true);
        let a = backend();
        let b = backend();
        let thread_a = ThreadId::new();
        let thread_b = if collision { thread_a } else { ThreadId::new() };
        // These goals already stand in the native widget when its frame observes them;
        // no routed GoalUpdated has populated backend bookkeeping for either one.
        select(&mut app, a.backend_id(), thread_a);
        app.chat_widget
            .handle_server_notification(active_goal(thread_a, "same active objective"), None);
        crate::app::alab_session::observe(&app);
        assert_eq!(
            shared.goal(&thread_a.to_string()).as_deref(),
            Some("same active objective")
        );
        select(&mut app, b.backend_id(), thread_b);
        app.chat_widget
            .handle_server_notification(active_goal(thread_b, "same active objective"), None);
        crate::app::alab_session::observe(&app);
        app.chat_widget.insert_str("B's goal and draft remain");
        let before = app.chat_widget.composer_text_with_pending();

        poll(
            &mut app,
            &b,
            b.backend_id(),
            a.backend_id(),
            closed(thread_a),
        )
        .await;
        assert_eq!(
            shared.goal(&thread_a.to_string()).as_deref(),
            Some("same active objective"),
            "a mismatched source owns no closure"
        );
        poll(
            &mut app,
            &a,
            b.backend_id(),
            a.backend_id(),
            closed(thread_a),
        )
        .await;
        if !collision {
            assert!(shared.goal(&thread_a.to_string()).is_none());
        }
        assert_eq!(
            shared.goal(&thread_b.to_string()).as_deref(),
            Some("same active objective"),
            "B survives even with the identical native key and objective"
        );
        assert_eq!(app.current_displayed_thread_id(), Some(thread_b));
        assert_eq!(app.chat_widget.composer_text_with_pending(), before);
        poll(
            &mut app,
            &b,
            b.backend_id(),
            b.backend_id(),
            closed(thread_b),
        )
        .await;
        assert!(shared.goal(&thread_b.to_string()).is_none());
        assert!(app.alab_backend_bookkeeping.goals.borrow().is_empty());
    }
}

#[tokio::test]
async fn routed_goal_clear_restores_the_other_backend_owner_before_its_close() {
    let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
    let _scope = boxes::scoped(shared);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    app.chat_widget
        .set_feature_enabled(crate::Feature::Goals, /*enabled*/ true);
    let a = backend();
    let b = backend();
    let thread = ThreadId::new();
    select(&mut app, a.backend_id(), thread);
    poll(
        &mut app,
        &a,
        a.backend_id(),
        a.backend_id(),
        AppServerEvent::ServerNotification(Box::new(active_goal(thread, "A's active goal"))),
    )
    .await;
    select(&mut app, b.backend_id(), thread);
    poll(
        &mut app,
        &b,
        b.backend_id(),
        b.backend_id(),
        AppServerEvent::ServerNotification(Box::new(active_goal(thread, "B's active goal"))),
    )
    .await;
    assert_eq!(
        shared.goal(&thread.to_string()).as_deref(),
        Some("B's active goal")
    );
    poll(
        &mut app,
        &b,
        b.backend_id(),
        b.backend_id(),
        AppServerEvent::ServerNotification(Box::new(ServerNotification::ThreadGoalCleared(
            ThreadGoalClearedNotification {
                thread_id: thread.to_string(),
            },
        ))),
    )
    .await;
    assert_eq!(
        shared.goal(&thread.to_string()).as_deref(),
        Some("A's active goal")
    );
    poll(&mut app, &a, b.backend_id(), a.backend_id(), closed(thread)).await;
    assert!(shared.goal(&thread.to_string()).is_none());
    assert!(app.alab_backend_bookkeeping.goals.borrow().is_empty());
}

#[tokio::test]
async fn parked_goal_clear_and_completion_cannot_be_restored_by_a_later_displayed_clear() {
    for collision in [false, true] {
        for clear in [false, true] {
            let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
            let _scope = boxes::scoped(shared);
            let (mut app, _events, _ops) = make_test_app_with_channels().await;
            app.chat_widget
                .set_feature_enabled(crate::Feature::Goals, /*enabled*/ true);
            let a = backend();
            let b = backend();
            let thread_a = ThreadId::new();
            let thread_b = if collision { thread_a } else { ThreadId::new() };
            select(&mut app, a.backend_id(), thread_a);
            poll(
                &mut app,
                &a,
                a.backend_id(),
                a.backend_id(),
                AppServerEvent::ServerNotification(Box::new(active_goal(thread_a, "A original"))),
            )
            .await;
            select(&mut app, b.backend_id(), thread_b);
            poll(
                &mut app,
                &b,
                b.backend_id(),
                b.backend_id(),
                AppServerEvent::ServerNotification(Box::new(active_goal(thread_b, "B active"))),
            )
            .await;
            app.chat_widget
                .insert_str("B's draft survives parked lifecycle");
            let before = app.chat_widget.composer_text_with_pending();
            let ending = |thread: ThreadId| {
                if clear {
                    ServerNotification::ThreadGoalCleared(ThreadGoalClearedNotification {
                        thread_id: thread.to_string(),
                    })
                } else {
                    goal_status(thread, "completed objective", ThreadGoalStatus::Complete)
                }
            };
            // The forged envelope must not retire A, including when B currently hides its
            // shared key. Clearing B exposes the retained A contribution as a negative control.
            poll(
                &mut app,
                &b,
                b.backend_id(),
                a.backend_id(),
                AppServerEvent::ServerNotification(Box::new(ending(thread_a))),
            )
            .await;
            poll(
                &mut app,
                &b,
                b.backend_id(),
                b.backend_id(),
                AppServerEvent::ServerNotification(Box::new(
                    ServerNotification::ThreadGoalCleared(ThreadGoalClearedNotification {
                        thread_id: thread_b.to_string(),
                    }),
                )),
            )
            .await;
            assert_eq!(
                shared.goal(&thread_a.to_string()).as_deref(),
                Some("A original")
            );
            poll(
                &mut app,
                &b,
                b.backend_id(),
                b.backend_id(),
                AppServerEvent::ServerNotification(Box::new(active_goal(thread_b, "B active"))),
            )
            .await;

            poll(
                &mut app,
                &a,
                b.backend_id(),
                a.backend_id(),
                AppServerEvent::ServerNotification(Box::new(active_goal(
                    thread_a,
                    "A updated while parked",
                ))),
            )
            .await;
            assert_eq!(
                shared.goal(&thread_b.to_string()).as_deref(),
                Some("B active")
            );
            let unadmitted = ThreadId::new();
            poll(
                &mut app,
                &a,
                b.backend_id(),
                a.backend_id(),
                AppServerEvent::ServerNotification(Box::new(active_goal(
                    unadmitted,
                    "must not create a parked goal",
                ))),
            )
            .await;
            assert!(shared.goal(&unadmitted.to_string()).is_none());

            poll(
                &mut app,
                &a,
                b.backend_id(),
                a.backend_id(),
                AppServerEvent::ServerNotification(Box::new(ending(thread_a))),
            )
            .await;
            assert_eq!(
                shared.goal(&thread_b.to_string()).as_deref(),
                Some("B active")
            );
            if !collision {
                assert!(shared.goal(&thread_a.to_string()).is_none());
            }
            assert_eq!(app.current_displayed_thread_id(), Some(thread_b));
            assert_eq!(app.chat_widget.composer_text_with_pending(), before);
            poll(
                &mut app,
                &b,
                b.backend_id(),
                b.backend_id(),
                AppServerEvent::ServerNotification(Box::new(
                    ServerNotification::ThreadGoalCleared(ThreadGoalClearedNotification {
                        thread_id: thread_b.to_string(),
                    }),
                )),
            )
            .await;
            assert!(
                shared.goal(&thread_a.to_string()).is_none(),
                "clearing B must not restore A's retired goal"
            );
            assert!(shared.goal(&thread_b.to_string()).is_none());
            assert!(app.alab_backend_bookkeeping.goals.borrow().is_empty());
        }
    }
}
