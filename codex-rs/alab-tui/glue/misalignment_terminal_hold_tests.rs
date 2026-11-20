// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::MisalignmentErrorDetails;
use codex_app_server_protocol::MisalignmentSteer;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn replay_restores_only_the_unresolved_hold_after_its_notification_is_evicted() {
    let thread_id = ThreadId::new();
    let error = AppServerTurnError {
        message: "Live-only message".into(),
        codex_error_info: Some(AppServerCodexErrorInfo::MisalignmentPolicyViolation),
        additional_details: Some("Live-only context".into()),
        misalignment: Some(MisalignmentErrorDetails {
            detailed_explanation: Some("Live-only findings".into()),
            error_type: None,
            steer: Some(MisalignmentSteer {
                message: "Live-only continuation instruction".into(),
            }),
            review_target: Some("Live-only acknowledgement".into()),
        }),
    };
    let refused = test_turn("refused", TurnStatus::Failed, Vec::new());
    let notifications = [
        ServerNotification::Error(ErrorNotification {
            thread_id: thread_id.to_string(),
            turn_id: refused.id.clone(),
            error: error.clone(),
            will_retry: false,
        }),
        ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: thread_id.to_string(),
            turn: Turn {
                error: Some(error),
                ..refused
            },
        }),
    ];
    for notification in &notifications {
        let mut store = ThreadEventStore::new(/*capacity*/ 1);
        let mut app = make_test_app().await;
        app.chat_widget
            .set_feature_enabled(Feature::MisalignmentReview, true);
        store.set_session(
            test_thread_session(thread_id, app.config.cwd.to_path_buf()),
            vec![test_turn("older", TurnStatus::Completed, Vec::new())],
        );
        store.push_notification(turn_started_notification(thread_id, "refused"));
        store.push_notification_ref(notification);
        // While buffered, the live notification already accounts for the hold.
        assert_eq!(store.snapshot().events.len(), 1);

        store.push_notification(ServerNotification::Warning(WarningNotification {
            thread_id: Some(thread_id.to_string()),
            message: "An unrelated notice".into(),
        }));
        let snapshot = store.snapshot();
        assert!(matches!(
            snapshot.events.as_slice(),
            [ThreadBufferedEvent::Notification(notice), ThreadBufferedEvent::Notification(restored)]
                if matches!(notice.as_ref(), ServerNotification::Warning(_))
                    && matches!(restored.as_ref(), ServerNotification::Error(n)
                        if n.turn_id == "refused" && !n.will_retry
                            && n.error.codex_error_info == Some(AppServerCodexErrorInfo::MisalignmentPolicyViolation)
                            && n.error.message.is_empty() && n.error.additional_details.is_none()
                            && n.error.misalignment.is_none())
        ));
        app.replay_thread_snapshot(snapshot, false);
        assert!(app.chat_widget.has_misalignment_policy_violation());
        assert!(render_bottom_popup(&app.chat_widget, 80).contains("Chat stopped"));

        store.set_active_turn_id("continued".into());
        store.push_notification(turn_completed_notification(
            thread_id,
            "continued",
            TurnStatus::Completed,
        ));
        let mut resumed = make_test_app().await;
        resumed
            .chat_widget
            .set_feature_enabled(Feature::MisalignmentReview, true);
        resumed.replay_thread_snapshot(store.snapshot(), false);
        assert!(!resumed.chat_widget.has_misalignment_policy_violation());
    }
}

#[tokio::test]
async fn a_live_start_releases_only_the_replayed_hold_of_an_earlier_turn() {
    let thread_id = ThreadId::new();
    let error = AppServerTurnError {
        message: "Chat stopped".into(),
        codex_error_info: Some(AppServerCodexErrorInfo::MisalignmentPolicyViolation),
        additional_details: None,
        misalignment: None,
    };
    let refused = test_turn("refused", TurnStatus::Failed, Vec::new());
    for notification in [
        ServerNotification::Error(ErrorNotification {
            thread_id: thread_id.to_string(),
            turn_id: refused.id.clone(),
            error: error.clone(),
            will_retry: false,
        }),
        ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: thread_id.to_string(),
            turn: Turn {
                error: Some(error),
                ..refused
            },
        }),
    ] {
        for side_hold in [false, true] {
            let mut app = make_test_app().await;
            app.chat_widget
                .set_feature_enabled(Feature::MisalignmentReview, true);
            let store = ThreadEventStore::new_with_session(
                1,
                test_thread_session(thread_id, app.config.cwd.to_path_buf()),
                vec![test_turn("older", TurnStatus::Completed, Vec::new())],
            );
            app.replay_thread_snapshot(store.snapshot(), false);
            if side_hold {
                app.chat_widget
                    .on_side_thread_misalignment_policy_violation();
            }
            // The latest refusal arrived after the stored history; its start was missed.
            app.chat_widget
                .handle_server_notification(notification.clone(), Some(ReplayKind::ThreadSnapshot));
            assert!(app.chat_widget.has_misalignment_policy_violation());

            app.chat_widget
                .handle_server_notification(turn_started_notification(thread_id, "refused"), None);
            assert!(app.chat_widget.has_misalignment_policy_violation());
            app.chat_widget
                .handle_server_notification(turn_started_notification(thread_id, "human"), None);
            assert_eq!(
                app.chat_widget.has_misalignment_policy_violation(),
                side_hold
            );
        }
    }
}

#[tokio::test]
async fn live_refusal_after_an_idle_turn_can_review_the_new_turn_without_its_start() -> Result<()> {
    async fn deliver(
        app: &mut App,
        thread: ThreadId,
        notification: ServerNotification,
    ) -> Result<()> {
        app.enqueue_thread_notification(thread, notification)
            .await?;
        while let Ok(event) = app.active_thread_rx.as_mut().unwrap().try_recv() {
            app.handle_thread_event_now_recovering_file_changes(event)
                .await;
        }
        Ok(())
    }
    let thread = ThreadId::new();
    let error = AppServerTurnError {
        message: "Chat stopped".into(),
        codex_error_info: Some(AppServerCodexErrorInfo::MisalignmentPolicyViolation),
        additional_details: None,
        misalignment: Some(MisalignmentErrorDetails {
            detailed_explanation: Some("The action exceeded this request.".into()),
            error_type: None,
            steer: None,
            review_target: Some("new-turn-target".into()),
        }),
    };
    let error_for = |thread: ThreadId, turn: &str| {
        ServerNotification::Error(ErrorNotification {
            thread_id: thread.to_string(),
            turn_id: turn.into(),
            error: error.clone(),
            will_retry: false,
        })
    };
    for refusal in [
        error_for(thread, "new"),
        ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: thread.to_string(),
            turn: Turn {
                error: Some(error.clone()),
                ..test_turn("new", TurnStatus::Failed, Vec::new())
            },
        }),
    ] {
        let (mut app, mut rx, _) = make_test_app_with_channels().await;
        app.chat_widget
            .set_feature_enabled(Feature::MisalignmentReview, true);
        let session = test_thread_session(thread, app.config.cwd.to_path_buf());
        app.chat_widget.handle_thread_session(session.clone());
        app.ensure_thread_channel(thread)
            .store
            .lock()
            .await
            .set_session(session, Vec::new());
        app.activate_thread_channel(thread).await;
        deliver(
            &mut app,
            thread,
            turn_started_notification(thread, "previous"),
        )
        .await?;
        // A non-current terminal must not claim the running turn.
        deliver(&mut app, thread, error_for(thread, "unrelated")).await?;
        assert_eq!(app.chat_widget.last_turn_id(), Some("previous"));
        assert!(!app.chat_widget.has_misalignment_policy_violation());
        deliver(
            &mut app,
            thread,
            turn_completed_notification(thread, "previous", TurnStatus::Completed),
        )
        .await?;
        assert!(!app.chat_widget.is_user_turn_pending_or_running());

        // Model a misrouted FIFO message whose turn ID matches this store.
        app.handle_thread_event_now_recovering_file_changes(ThreadBufferedEvent::Notification(
            Box::new(error_for(ThreadId::new(), "previous")),
        ))
        .await;
        assert!(!app.chat_widget.has_misalignment_policy_violation());

        deliver(&mut app, thread, refusal).await?;
        assert_eq!(app.chat_widget.last_turn_id(), Some("new"));
        assert!(app.chat_widget.has_misalignment_policy_violation());
        app.chat_widget
            .handle_key_event(KeyEvent::from(KeyCode::Enter));
        let review = std::iter::from_fn(|| rx.try_recv().ok())
            .find_map(|event| match event {
                AppEvent::ReviewMisalignment(review) => Some(review),
                _ => None,
            })
            .expect("the live finding belongs to the current turn");
        assert_eq!(review.thread_id, thread);
        assert_eq!(review.turn_id, "new");
        assert_eq!(
            review.details.review_target.as_deref(),
            Some("new-turn-target")
        );
        assert!(app.chat_widget.is_current_misalignment_review(&review));
    }
    Ok(())
}

#[tokio::test]
async fn enter_on_a_running_turn_checks_its_outcome_before_starting_another() -> Result<()> {
    use crate::app_server_session::ThreadParamsMode;
    use serde_json::json;
    use tokio::net::TcpListener;

    fn client_id(op: &Op) -> &str {
        let Op::UserTurn {
            client_user_message_id,
            ..
        } = op
        else {
            unreachable!("expected user turn");
        };
        client_user_message_id
    }

    for (steer_error, may_start) in [
        ("a provider finding requires a new user message", false),
        ("turn is finishing and cannot accept steer input", false),
        ("no active turn to steer", true),
    ] {
        let (mut app, _events, mut ops) = make_test_app_with_channels().await;
        let thread = ThreadId::new();
        app.active_thread_id = Some(thread);
        app.primary_thread_id = Some(thread);
        app.chat_widget
            .handle_thread_session_quiet(test_thread_session(thread, app.config.cwd.to_path_buf()));
        let started = turn_started_notification(thread, "observed");
        app.ensure_thread_channel(thread)
            .store
            .lock()
            .await
            .push_notification(started.clone());
        app.chat_widget.handle_server_notification(started, None);
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
        app.app_server_target = AppServerTarget::Remote {
            endpoint: endpoint.clone(),
        };
        let (tx, wire) = std::sync::mpsc::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            super::disconnect::serve_reconnect_requests(
                tokio_tungstenite::accept_async(stream).await?,
                |request| {
                    let response = match request.method.as_str() {
                        "turn/steer" => json!({"error": {"code": -32600, "message": steer_error}}),
                        "turn/start" => json!({"result": {
                            "turn": test_turn("next", TurnStatus::InProgress, Vec::new())
                        }}),
                        _ => json!({"error": {"code": -32601, "message": "unknown request"}}),
                    };
                    tx.send((request.method, request.params.unwrap())).unwrap();
                    std::future::ready(Some(response))
                },
            )
            .await
        });
        let mut session = AppServerSession::new(
            crate::backend_id::BackendId::new(),
            crate::connect_remote_app_server(endpoint).await?,
            ThreadParamsMode::Remote,
        );
        let mut tui = crate::tui::test_support::make_test_tui()?;
        app.chat_widget
            .apply_external_edit("before completion".into());
        app.chat_widget
            .handle_key_event(KeyEvent::from(KeyCode::Enter));
        let op = next_user_turn_op(&mut ops);
        let original_id = client_id(&op).to_owned();

        // A terminal notification can enter the store after Enter emitted the op
        // and before the app dispatches it. The widget receives it from the FIFO.
        let completed = turn_completed_notification(thread, "observed", TurnStatus::Completed);
        app.ensure_thread_channel(thread)
            .store
            .lock()
            .await
            .push_notification(completed.clone());
        assert_eq!(app.active_turn_id_for_thread(thread).await, None);
        app.handle_event(&mut tui, &mut session, AppEvent::CodexOp(op))
            .await?;
        let (method, params) = wire.try_recv()?;
        assert_eq!(method, "turn/steer", "{steer_error}");
        assert_eq!(params["expectedTurnId"], "observed");
        assert_eq!(params["clientUserMessageId"], original_id.as_str());
        assert_eq!(params["input"][0]["text"], "before completion");

        if may_start {
            let (method, params) = wire.try_recv()?;
            assert_eq!(method, "turn/start");
            assert_eq!(params["turnTrigger"], "user_steer_fallback");
            assert_eq!(params["clientUserMessageId"], original_id.as_str());
            assert_eq!(params["input"][0]["text"], "before completion");
        } else {
            assert!(wire.try_recv().is_err(), "{steer_error}");
            assert_eq!(app.active_turn_id_for_thread(thread).await, None);
            assert_eq!(
                app.chat_widget.composer_text_with_pending(),
                "before completion"
            );

            app.chat_widget.handle_server_notification(completed, None);
            app.chat_widget
                .apply_external_edit("fresh correction".into());
            app.chat_widget
                .handle_key_event(KeyEvent::from(KeyCode::Enter));
            let fresh = next_user_turn_op(&mut ops);
            let fresh_id = client_id(&fresh).to_owned();
            assert_ne!(fresh_id, original_id);
            app.handle_event(&mut tui, &mut session, AppEvent::CodexOp(fresh))
                .await?;
            let (method, params) = wire.try_recv()?;
            assert_eq!(method, "turn/start");
            assert_eq!(params["turnTrigger"], "user");
            assert_eq!(params["clientUserMessageId"], fresh_id.as_str());
            assert_eq!(params["input"][0]["text"], "fresh correction");
        }
        session.shutdown().await?;
        server.await??;
        assert!(wire.try_recv().is_err());
    }
    Ok(())
}
