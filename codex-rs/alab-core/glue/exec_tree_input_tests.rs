// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[tokio::test]
async fn a_sealed_tree_refuses_input_before_settings_or_reservation_change() {
    let (session, _) = crate::session::tests::make_session_and_context().await;
    let session = Arc::new(session);
    let original_model = session.get_config().await.model.clone();
    let mut seal = session
        .services
        .local_agent_runtime
        .seal_exec_tree_starts()
        .await
        .unwrap();
    seal.commit();
    for mode in [
        TurnInputMode::StartIfIdle,
        TurnInputMode::StartOrSteer,
        TurnInputMode::Steer {
            expected_turn_id: "old-turn".into(),
        },
    ] {
        let request = TurnInputRequest::user_input(vec![UserInput::Text {
            text: "must not be accepted".into(),
            text_elements: Vec::new(),
        }])
        .with_thread_settings(ThreadSettingsOverrides {
            model: Some("must-not-change".into()),
            ..Default::default()
        });
        let error = handle(&session, request, mode, "late-input".into())
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("agent tree is being transferred")
        );
        assert!(session.active_turn.lock().await.is_none());
        assert_eq!(session.get_config().await.model, original_model);
    }
    let error = handle_recovery(
        &session,
        ThreadSettingsOverrides::default(),
        TurnStartOptions::default(),
        "old-turn".into(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("agent tree is being transferred")
    );
    assert!(session.active_turn.lock().await.is_none());
}

#[tokio::test]
async fn a_sealed_tree_leaves_automatic_wake_mail_unconsumed() {
    let (session, _) = crate::session::tests::make_session_and_context().await;
    let session = Arc::new(session);
    let mail = codex_protocol::protocol::InterAgentCommunication::new(
        codex_protocol::AgentPath::root(),
        codex_protocol::AgentPath::root(),
        Vec::new(),
        "already accepted".into(),
        true,
    );
    session
        .input_queue
        .enqueue_mailbox_communication(mail.clone().into(), TurnStartOptions::default())
        .await;
    let mut seal = session
        .services
        .local_agent_runtime
        .seal_exec_tree_starts()
        .await
        .unwrap();
    seal.commit();
    session
        .maybe_start_turn_for_pending_work_with_sub_id("late-wake".into())
        .await;
    assert!(session.active_turn.lock().await.is_none());
    let queued = session.input_queue.drain_mailbox().await;
    assert_eq!(queued.len(), 1);
    assert_eq!(*queued[0].communication, mail);
}
