// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_app_server_protocol::ErrorNotification;
use pretty_assertions::assert_eq;

fn misalignment_error() -> AppServerTurnError {
    AppServerTurnError {
        message: "Chat stopped".into(),
        codex_error_info: Some(AppServerCodexErrorInfo::MisalignmentPolicyViolation),
        additional_details: None,
        misalignment: None,
    }
}

fn refusal_notification(thread_id: ThreadId, turn_id: &str) -> ServerNotification {
    ServerNotification::Error(ErrorNotification {
        thread_id: thread_id.to_string(),
        turn_id: turn_id.into(),
        error: misalignment_error(),
        will_retry: false,
    })
}

#[tokio::test]
async fn misalignment_replay_keeps_the_latest_restored_refusal() {
    let thread_id = ThreadId::new();
    let error = misalignment_error();
    let older = test_turn("older", TurnStatus::Completed, Vec::new());
    let refused = Turn {
        error: Some(error.clone()),
        ..test_turn("refused", TurnStatus::Failed, Vec::new())
    };

    for (continued, delayed) in [
        (
            false,
            turn_completed_notification(thread_id, &older.id, TurnStatus::Completed),
        ),
        (false, refusal_notification(thread_id, &older.id)),
        (true, refusal_notification(thread_id, &refused.id)),
    ] {
        let mut app = make_test_app().await;
        app.chat_widget
            .set_feature_enabled(Feature::MisalignmentReview, /*enabled*/ true);
        let mut store = ThreadEventStore::new_with_session(
            /*capacity*/ 1,
            test_thread_session(thread_id, app.config.cwd.to_path_buf()),
            vec![older.clone(), refused.clone()],
        );
        if continued {
            store.push_notification(turn_started_notification(thread_id, "continued"));
            store.push_notification(turn_completed_notification(
                thread_id,
                "continued",
                TurnStatus::Completed,
            ));
        }
        store.push_notification(delayed);

        let snapshot = store.snapshot();
        let restored = snapshot
            .turns
            .iter()
            .find(|turn| turn.id == refused.id)
            .unwrap();
        assert_eq!(restored.error.as_ref(), (!continued).then_some(&error));
        app.replay_thread_snapshot(snapshot, /*resume_restored_queue*/ false);
        assert_eq!(
            app.chat_widget.has_misalignment_policy_violation(),
            !continued
        );
    }
}

#[tokio::test]
async fn misalignment_replay_keeps_the_live_finding_after_older_events_are_evicted() {
    let thread_id = ThreadId::new();
    let mut app = make_test_app().await;
    app.chat_widget
        .set_feature_enabled(Feature::MisalignmentReview, /*enabled*/ true);
    let mut store = ThreadEventStore::new_with_session(
        /*capacity*/ 1,
        test_thread_session(thread_id, app.config.cwd.to_path_buf()),
        Vec::new(),
    );
    store.push_notification(turn_started_notification(thread_id, "older"));
    store.push_notification(turn_completed_notification(
        thread_id,
        "older",
        TurnStatus::Completed,
    ));
    store.push_notification(turn_started_notification(thread_id, "refused"));
    store.push_notification(ServerNotification::TurnCompleted(
        TurnCompletedNotification {
            thread_id: thread_id.to_string(),
            turn: Turn {
                error: Some(misalignment_error()),
                ..test_turn("refused", TurnStatus::Failed, Vec::new())
            },
        },
    ));
    store.push_notification(turn_completed_notification(
        thread_id,
        "older",
        TurnStatus::Completed,
    ));
    app.replay_thread_snapshot(store.snapshot(), /*resume_restored_queue*/ false);
    assert!(app.chat_widget.has_misalignment_policy_violation());
}

#[tokio::test]
async fn misalignment_parent_routing_interrupts_the_side_turn_only_for_a_current_finding()
-> Result<()> {
    let (mut app, _events, mut ops) = make_test_app_with_channels().await;
    app.chat_widget
        .set_feature_enabled(Feature::MisalignmentReview, /*enabled*/ true);
    let parent = ThreadId::new();
    let side = ThreadId::new();
    app.primary_thread_id = Some(parent);
    app.active_thread_id = Some(side);
    app.side_threads.insert(side, SideThreadState::new(parent));
    app.chat_widget
        .handle_thread_session(test_thread_session(side, app.config.cwd.to_path_buf()));
    for turn in ["older", "later"] {
        app.enqueue_thread_notification(parent, turn_started_notification(parent, turn))
            .await?;
        app.enqueue_thread_notification(
            parent,
            turn_completed_notification(parent, turn, TurnStatus::Completed),
        )
        .await?;
    }
    app.chat_widget
        .restore_user_message_to_composer("pending side turn".into());
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    while ops.try_recv().is_ok() {}

    app.enqueue_thread_notification(parent, refusal_notification(parent, "older"))
        .await?;
    assert!(!app.chat_widget.has_misalignment_policy_violation());
    assert!(ops.try_recv().is_err());

    app.enqueue_thread_notification(parent, refusal_notification(parent, "unseen"))
        .await?;
    assert!(app.chat_widget.has_misalignment_policy_violation());
    assert_matches!(ops.try_recv(), Ok(Op::Interrupt));
    Ok(())
}
