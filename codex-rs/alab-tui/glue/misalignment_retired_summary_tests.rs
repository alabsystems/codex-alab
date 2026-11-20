// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_app_server_protocol::ItemCompletedNotification;
use pretty_assertions::assert_eq;

fn final_message() -> ThreadItemPayload {
    ThreadItemPayload::AgentMessage {
        id: "final-item".into(),
        text: "The historical answer.".into(),
        phase: Some(codex_protocol::models::MessagePhase::FinalAnswer),
        questions: None,
        moderation: None,
        memory_citation: None,
        delivery: None,
    }
}

fn completed_item(thread: ThreadId) -> ServerNotification {
    ServerNotification::ItemCompleted(ItemCompletedNotification::new(
        thread.to_string(),
        "old".into(),
        final_message(),
        Some(1),
        2,
    ))
}

#[tokio::test]
async fn misalignment_retired_summary_recovers_only_the_unseen_final_item() -> Result<()> {
    for (refused, already_had_item) in [(true, false), (false, false), (false, true)] {
        let (mut app, _events, mut ops) = make_test_app_with_channels().await;
        app.chat_widget
            .set_feature_enabled(Feature::MisalignmentReview, true);
        let thread = ThreadId::new();
        app.primary_thread_id = Some(thread);
        app.active_thread_id = Some(thread);
        let mut store = ThreadEventStore::new_with_session(
            1,
            test_thread_session(thread, app.config.cwd.to_path_buf()),
            Vec::new(),
        );
        store.push_notification(turn_started_notification(thread, "old"));
        if already_had_item {
            store.push_notification(completed_item(thread));
        }
        store.push_notification(turn_started_notification(thread, "current"));
        if refused {
            let error = AppServerTurnError {
                message: "Chat stopped".into(),
                codex_error_info: Some(AppServerCodexErrorInfo::MisalignmentPolicyViolation),
                additional_details: None,
                misalignment: None,
            };
            store.push_notification(ServerNotification::TurnCompleted(
                TurnCompletedNotification {
                    thread_id: thread.to_string(),
                    turn: Turn {
                        error: Some(error),
                        ..test_turn("current", TurnStatus::Failed, Vec::new())
                    },
                },
            ));
            app.replay_thread_snapshot(store.snapshot(), false);
        }
        let old_summary = ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: thread.to_string(),
            turn: test_turn("old", TurnStatus::Completed, vec![final_message()]),
        });
        let channel = app.ensure_thread_channel(thread);
        let shared = Arc::clone(&channel.store);
        *shared.lock().await = store;
        let old_length = app.transcript_cells.len();
        // The assertion below measures only what the delayed deliveries send.
        while ops.try_recv().is_ok() {}
        app.enqueue_thread_notification(thread, old_summary.clone())
            .await?;
        app.enqueue_thread_notification(thread, old_summary).await?;
        if !already_had_item {
            app.enqueue_thread_notification(thread, completed_item(thread))
                .await?;
        }
        let expected = usize::from(!already_had_item);
        assert_eq!(app.transcript_cells.len() - old_length, expected);
        if expected == 1 {
            assert!(
                lines_to_single_string(&app.transcript_cells[old_length].display_lines(80))
                    .contains("The historical answer.")
            );
        }
        assert_eq!(app.chat_widget.has_misalignment_policy_violation(), refused);
        if let Ok(op) = ops.try_recv() {
            panic!("delayed turn delivery sent {op:?}");
        }
        let store = shared.lock().await;
        assert_eq!(store.active_turn_id(), (!refused).then_some("current"));
        assert_eq!(store.latest_turn_id.as_deref(), Some("current"));
        assert_eq!(store.recap_progress().completed_turns, 0);
        let snapshot = store.snapshot();
        assert_eq!(
            snapshot
                .turns
                .iter()
                .filter(|t| t.id == "old")
                .flat_map(Turn::item_payloads)
                .filter(|item| item.id() == "final-item")
                .count(),
            expected
        );
    }
    Ok(())
}

#[test]
fn misalignment_retired_summary_respects_the_order_of_an_inline_voice_handoff() {
    let thread = ThreadId::new();
    let marker = ThreadItemPayload::UserMessage {
        id: "handoff".into(),
        client_id: None,
        content: vec![UserInput::Text {
            text: "<realtime_delegation><input>check</input></realtime_delegation>".into(),
            text_elements: Vec::new(),
        }],
    };
    let private = ThreadItemPayload::AgentMessage {
        id: "private".into(),
        text: "[ANALYSIS] private reasoning".into(),
        phase: None,
        questions: None,
        moderation: None,
        memory_citation: None,
        delivery: None,
    };
    for public in [false, true] {
        let mut store = ThreadEventStore::new(1);
        store.push_notification(turn_started_notification(thread, "old"));
        store.push_notification(turn_started_notification(thread, "current"));
        let items = public
            .then(final_message)
            .into_iter()
            .chain([marker.clone(), private.clone()])
            .collect();
        store.push_notification(ServerNotification::TurnCompleted(
            TurnCompletedNotification {
                thread_id: thread.to_string(),
                turn: test_turn("old", TurnStatus::Completed, items),
            },
        ));
        let snapshot = store.snapshot();
        let ids = snapshot
            .turns
            .iter()
            .flat_map(Turn::item_payloads)
            .map(ThreadItemPayload::id)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            if public {
                vec!["final-item"]
            } else {
                Vec::new()
            }
        );
        assert_eq!(store.active_turn_id(), Some("current"));
    }
}
