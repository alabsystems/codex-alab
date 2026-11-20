// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#[test]
fn live_history_preserves_missing_exit_until_an_observed_exit_arrives() {
    use codex_protocol::protocol::HasLegacyEvent;

    let mut builder = ThreadHistoryBuilder::new();
    let thread_id = ThreadId::new();
    let turn_id = "turn-1";
    builder.handle_live_paginated_event(&EventMsg::TurnStarted(TurnStartedEvent {
        turn_attribution: None,
        turn_id: turn_id.into(),
        root_turn_id: None,
        trace_id: None,
        started_at: None,
        model_context_window: None,
        collaboration_mode_kind: Default::default(),
    }));
    let zero = Some(Duration::ZERO);
    let elapsed = Some(Duration::from_millis(17));
    for (status, code, expected_integer, duration) in [
        (CoreCommandExecutionStatus::Declined, None, -1, None),
        (CoreCommandExecutionStatus::Failed, None, -1, zero),
        (CoreCommandExecutionStatus::Completed, Some(0), 0, None),
        (CoreCommandExecutionStatus::Completed, None, 0, None),
        (CoreCommandExecutionStatus::Failed, Some(-1), -1, zero),
        (CoreCommandExecutionStatus::Completed, Some(0), 0, elapsed),
    ] {
        let canonical = EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id,
            turn_id: turn_id.into(),
            started_at_ms: Some(10),
            completed_at_ms: 20,
            content_token_count: None,
            item: CoreTurnItem::CommandExecution(CoreCommandExecutionItem {
                model_context: None,
                sandbox_type: None,
                id: "process".into(),
                plugin_id: None,
                script_path: None,
                process_id: Some("12".into()),
                command: vec!["cat".into()],
                cwd: test_path_buf("/tmp").abs().into(),
                parsed_cmd: vec![],
                source: ExecCommandSource::Agent,
                interaction_input: None,
                status,
                aggregated_output: None,
                exit_code: code,
                duration,
            }),
        });
        let expected_status = CommandExecutionStatus::from(status);
        let compat = canonical.as_legacy_events(false);
        let [EventMsg::ExecCommandEnd(end)] = compat.as_slice() else {
            panic!("each completion needs one legacy end event");
        };
        let json = serde_json::to_value(end).unwrap();
        assert_eq!(
            json.get("exit_code").and_then(serde_json::Value::as_i64),
            Some(i64::from(expected_integer))
        );
        assert_eq!(
            json.get("exit_code_unavailable")
                .and_then(serde_json::Value::as_bool),
            code.is_none().then_some(true)
        );
        assert_eq!(
            json.get("duration"),
            Some(&serde_json::to_value(duration.unwrap_or_default()).unwrap())
        );
        assert_eq!(
            json.get("duration_unavailable").cloned(),
            duration.is_none().then_some(serde_json::Value::Bool(true))
        );
        let roundtrip: ExecCommandEndEvent = serde_json::from_value(json).unwrap();
        assert_eq!(roundtrip.observed_duration(), duration);
        assert_eq!(roundtrip.observed_exit_code(), code);
        // A subsequent raw PTY exit can arrive on its own after an earlier unreported
        // outcome. It must still be allowed to complete the same item.
        let canonical = code.is_none().then_some(canonical);
        for event in canonical
            .into_iter()
            .chain([EventMsg::ExecCommandEnd(roundtrip)])
        {
            builder.handle_live_paginated_event(&event);
            let turn = builder.active_turn_snapshot().unwrap();
            let [item] = turn.items.as_slice() else {
                panic!("one retained command");
            };
            let ThreadItemPayload::CommandExecution {
                exit_code,
                duration_ms,
                status: actual,
                ..
            } = &item.payload
            else {
                panic!("expected command");
            };
            assert_eq!(*exit_code, code);
            assert_eq!(
                *duration_ms,
                duration.map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap())
            );
            assert_eq!(*actual, expected_status);
        }
    }
}
