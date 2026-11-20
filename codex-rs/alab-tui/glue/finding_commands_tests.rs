// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn passive_finding_preserves_the_actual_late_read_result() {
    for exit_code in [0, 42] {
        check_late_read_after_finding(false, exit_code).await;
    }
}

#[tokio::test]
async fn interactive_finding_preserves_the_actual_late_read_result_and_review_hold() {
    for exit_code in [0, 42] {
        check_late_read_after_finding(true, exit_code).await;
    }
}

fn fixture_read(
    chat: &ChatWidget,
    id: &str,
    process: &str,
    file: &str,
) -> AppServerThreadItemPayload {
    let command = format!("cat {file}");
    AppServerThreadItemPayload::CommandExecution {
        model_context: None,
        sandbox_type: None,
        id: id.to_owned(),
        command: command.clone(),
        cwd: chat.config.cwd.clone().into(),
        process_id: Some(process.to_owned()),
        plugin_id: None,
        script_path: None,
        source: ExecCommandSource::UnifiedExecStartup,
        status: AppServerCommandExecutionStatus::InProgress,
        command_actions: vec![AppServerCommandAction::Read {
            command,
            name: file.to_owned(),
            path: chat.config.cwd.join(file).into(),
        }],
        aggregated_output: None,
        exit_code: None,
        duration_ms: None,
    }
}

fn take_history(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<Box<dyn HistoryCell>> {
    let mut cells = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            cells.push(cell);
        }
    }
    cells
}

async fn check_late_read_after_finding(interactive: bool, exit_code: i32) {
    const CALL: &str = "owned-call";
    const FILE: &str = "command-observation.fifo";
    const OUTPUT: &str = "output from the completed reader";

    let (mut chat, mut events, mut ops) = make_chatwidget_manual(None).await;
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    chat.set_feature_enabled(Feature::MisalignmentReview, interactive);
    handle_turn_started(&mut chat, "turn-1");
    let item = fixture_read(&chat, CALL, "418", FILE);
    handle_exec_begin(&mut chat, item.clone());
    let active = chat.transcript.active_cell.as_ref().unwrap();
    let exec = active.as_any().downcast_ref::<ExecCell>().unwrap();
    assert!(exec.is_exploring_cell() && exec.is_active());
    assert!(exec.iter_calls().any(|call| call.call_id == CALL));
    assert!(active_blob(&chat).contains(FILE));

    let error = AppServerTurnError {
        message: ERROR_MESSAGE.to_owned(),
        codex_error_info: Some(CodexErrorInfo::MisalignmentPolicyViolation),
        misalignment: Some(review_details()),
        additional_details: None,
    };
    // Some servers send both the terminal error and the matching failed turn.
    // The passive path below also exercises a failed turn without a preceding error.
    if interactive {
        chat.handle_server_notification(
            ServerNotification::Error(ErrorNotification {
                thread_id: thread.to_string(),
                turn_id: "turn-1".to_owned(),
                error: error.clone(),
                will_retry: false,
            }),
            None,
        );
    }
    chat.handle_server_notification(
        ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: thread.to_string(),
            turn: app_server_turn("turn-1", AppServerTurnStatus::Failed, None, Some(error)),
        }),
        None,
    );

    // Keep the emitted cells so both append-only and retained in-place completion can be seen.
    let mut history: Vec<std::sync::Arc<dyn HistoryCell>> = Vec::new();
    let mut collect_and_render = |chat: &ChatWidget| {
        while let Ok(event) = events.try_recv() {
            match event {
                AppEvent::InsertHistoryCell(cell) => history.push(cell.into()),
                AppEvent::AlabFindingCommandFinished {
                    thread_id,
                    turn_id,
                    call_id,
                    cell,
                    ..
                } => {
                    let row = history
                        .iter_mut()
                        .find(|row| {
                            row.as_any()
                                .downcast_ref::<AlabPendingCommand>()
                                .is_some_and(|pending| {
                                    pending.matches(thread_id, &turn_id, &call_id)
                                })
                        })
                        .expect("the late result owns a pending row");
                    *row = cell;
                }
                _ => {}
            }
        }
        let mut compact = String::new();
        let mut transcript = String::new();
        for cell in history
            .iter()
            .map(|cell| cell.as_ref())
            .chain(chat.transcript.active_cell.as_deref())
        {
            compact.push_str(&lines_to_single_string(&cell.display_lines(90)));
            transcript.push_str(&lines_to_single_string(&cell.transcript_lines(90)));
        }
        (compact, transcript)
    };
    let (compact, transcript) = collect_and_render(&chat);
    assert!(!compact.contains("(exit "), "{compact}");
    assert!(!transcript.lines().any(is_read_result), "{transcript}");
    assert!(!chat.bottom_pane.is_task_running());
    assert_eq!(chat.has_misalignment_policy_violation(), interactive);
    assert_eq!(chat.bottom_pane.composer_input_enabled(), !interactive);
    if !interactive {
        assert!(compact.contains("Misalignment policy finding"), "{compact}");
    }
    assert_no_submit_op(&mut ops);

    end_exec(&mut chat, item, OUTPUT, "", exit_code);

    let (compact, transcript) = collect_and_render(&chat);
    let results: Vec<_> = transcript
        .lines()
        .filter(|line| is_read_result(line))
        .collect();
    assert_eq!(results.len(), 1, "{transcript}");
    assert_eq!(transcript.matches(OUTPUT).count(), 1, "{transcript}");
    assert!(transcript.contains(FILE), "{transcript}");
    if exit_code == 0 {
        assert!(results[0].trim_start().starts_with('✓'), "{transcript}");
        assert!(!compact.contains("(exit "), "{compact}");
    } else {
        assert!(
            results[0].trim_start().starts_with("✗ (42)"),
            "{transcript}"
        );
        assert!(compact.contains("(exit 42)"), "{compact}");
    }
    assert!(chat.unified_exec_processes.is_empty());
    assert!(!chat.bottom_pane.is_task_running());
    assert_eq!(chat.has_misalignment_policy_violation(), interactive);
    assert_eq!(chat.bottom_pane.composer_input_enabled(), !interactive);
    if interactive {
        assert!(!chat.submit_op(Op::Compact));
    }
    assert_no_submit_op(&mut ops);
}

fn is_read_result(line: &str) -> bool {
    line.trim_start().starts_with('✓') || line.trim_start().starts_with('✗')
}

#[tokio::test]
async fn late_finding_completion_preserves_grouped_results_and_the_new_active_turn() {
    let (mut chat, mut events, mut ops) = make_chatwidget_manual(None).await;
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    chat.set_feature_enabled(Feature::MisalignmentReview, false);
    handle_turn_started(&mut chat, "old-turn");
    let earlier = begin_exec_with_source(
        &mut chat,
        "earlier",
        "cat earlier.txt",
        ExecCommandSource::UnifiedExecStartup,
    );
    end_exec(&mut chat, earlier, "earlier output", "", 17);
    let mut late = begin_exec_with_source(
        &mut chat,
        "late",
        "cat pending.txt",
        ExecCommandSource::UnifiedExecStartup,
    );
    let active = chat
        .transcript
        .active_cell
        .as_ref()
        .unwrap()
        .as_any()
        .downcast_ref::<ExecCell>()
        .unwrap();
    assert_eq!(active.iter_calls().count(), 2);
    handle_error(
        &mut chat,
        ERROR_MESSAGE,
        Some(CodexErrorInfo::MisalignmentPolicyViolation),
    );

    let retained = take_history(&mut events);
    let before: String = retained
        .iter()
        .map(|row| lines_to_single_string(&row.display_lines(80)))
        .collect();
    assert!(before.contains("earlier.txt (exit 17)"), "{before}");
    assert_eq!(before.matches("pending.txt").count(), 1, "{before}");
    handle_turn_started(&mut chat, "new-turn");
    begin_exec_with_source(
        &mut chat,
        "new-work",
        "cat fresh.txt",
        ExecCommandSource::UnifiedExecStartup,
    );

    let AppServerThreadItemPayload::CommandExecution {
        status,
        exit_code,
        duration_ms,
        aggregated_output,
        ..
    } = &mut late
    else {
        panic!("expected original command");
    };
    *status = AppServerCommandExecutionStatus::Failed;
    *exit_code = Some(34);
    *duration_ms = Some(7);
    *aggregated_output = Some("genuine late output".to_owned());
    for (owner, turn, expected) in [
        (ThreadId::new(), "old-turn", false),
        (thread, "unrelated-turn", false),
        (thread, "old-turn", true),
        (thread, "old-turn", false),
    ] {
        chat.handle_server_notification(
            ServerNotification::ItemCompleted(ItemCompletedNotification::new(
                owner.to_string(),
                turn.to_owned(),
                late.clone(),
                None,
                0,
            )),
            None,
        );
        let mut corrections = 0;
        while let Ok(event) = events.try_recv() {
            match event {
                AppEvent::AlabFindingCommandFinished {
                    thread_id,
                    turn_id,
                    call_id,
                    cell,
                    ..
                } => {
                    assert_eq!(
                        (thread_id, turn_id.as_str(), call_id.as_str()),
                        (thread, "old-turn", "late")
                    );
                    let transcript = lines_to_single_string(&cell.transcript_lines(80));
                    assert!(transcript.contains("genuine late output"));
                    assert!(transcript.contains("✗ (34)"));
                    corrections += 1;
                }
                AppEvent::InsertHistoryCell(cell) => {
                    assert!(
                        !lines_to_single_string(&cell.display_lines(80)).contains("pending.txt")
                    );
                }
                _ => {}
            }
        }
        assert_eq!(corrections, usize::from(expected));
        assert!(active_blob(&chat).contains("fresh.txt"));
        assert!(chat.bottom_pane.is_task_running());
    }
    assert_no_submit_op(&mut ops);
}

#[tokio::test]
async fn a_late_exit_leaves_a_reused_item_and_background_terminal_owned_by_the_new_turn() {
    for new_process in ["41", "87"] {
        let (mut chat, mut events, _ops) = make_chatwidget_manual(None).await;
        let thread = ThreadId::new();
        chat.thread_id = Some(thread);
        chat.set_feature_enabled(Feature::MisalignmentReview, false);
        handle_turn_started(&mut chat, "old");
        let mut old = fixture_read(&chat, "shared-item", "41", "old.fifo");
        handle_exec_begin(&mut chat, old.clone());
        handle_error(
            &mut chat,
            ERROR_MESSAGE,
            Some(CodexErrorInfo::MisalignmentPolicyViolation),
        );
        let retained = take_history(&mut events);
        assert!(
            retained
                .iter()
                .any(|row| row.as_any().is::<AlabPendingCommand>())
        );

        handle_turn_started(&mut chat, "new");
        let new = fixture_read(&chat, "shared-item", new_process, "new.fifo");
        handle_exec_begin(&mut chat, new.clone());
        chat.begin_background_terminal_restore();
        let AppServerThreadItemPayload::CommandExecution {
            status, exit_code, ..
        } = &mut old
        else {
            panic!("expected old command");
        };
        *status = AppServerCommandExecutionStatus::Completed;
        *exit_code = Some(0);
        chat.handle_server_notification(
            ServerNotification::ItemCompleted(ItemCompletedNotification::new(
                thread.to_string(),
                "old".to_owned(),
                old,
                None,
                0,
            )),
            None,
        );
        assert!(active_blob(&chat).contains("new.fifo"));
        assert_eq!(chat.unified_exec_processes.len(), 1);
        assert_eq!(chat.unified_exec_processes[0].key, new_process);
        assert!(
            !chat
                .pending_terminal_restore
                .as_ref()
                .unwrap()
                .1
                .contains("shared-item")
        );
        let mut old_results = 0;
        while let Ok(event) = events.try_recv() {
            if let AppEvent::AlabFindingCommandFinished { turn_id, cell, .. } = event {
                assert_eq!(turn_id, "old");
                assert!(lines_to_single_string(&cell.display_lines(80)).contains("old.fifo"));
                old_results += 1;
            }
        }
        assert_eq!(old_results, 1);
        end_exec(&mut chat, new, "new result", "", 19);
        assert!(active_blob(&chat).contains("new.fifo (exit 19)"));
        assert!(chat.unified_exec_processes.is_empty());
    }
}

#[tokio::test]
async fn saved_finding_replays_the_pending_read_and_accepts_its_genuine_outputless_exit() {
    assert_saved_finding_outcome(AppServerCommandExecutionStatus::Failed, Some(75), "✗ (75)").await;
}

#[tokio::test]
async fn saved_finding_keeps_late_terminal_status_without_an_exit() {
    for (status, expected) in [
        (AppServerCommandExecutionStatus::Declined, "Declined"),
        (AppServerCommandExecutionStatus::Failed, "Failed"),
        (AppServerCommandExecutionStatus::Completed, "Completed"),
    ] {
        assert_saved_finding_outcome(status, None, expected).await;
    }
}

async fn assert_saved_finding_outcome(
    result: AppServerCommandExecutionStatus,
    code: Option<i32>,
    expected: &str,
) {
    let (mut chat, mut events, mut ops) = make_chatwidget_manual(None).await;
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    chat.set_feature_enabled(Feature::MisalignmentReview, true);
    let mut item = fixture_read(&chat, "restored-read", "26", "saved.fifo");
    let AppServerThreadItemPayload::CommandExecution {
        aggregated_output, ..
    } = &mut item
    else {
        panic!("expected read");
    };
    *aggregated_output = Some("preview retained across the finding".to_owned());
    let mut turn = app_server_turn(
        "turn-1",
        AppServerTurnStatus::Failed,
        None,
        Some(AppServerTurnError {
            message: ERROR_MESSAGE.to_owned(),
            misalignment: None,
            codex_error_info: Some(CodexErrorInfo::MisalignmentPolicyViolation),
            additional_details: None,
        }),
    );
    turn.items
        .push(codex_app_server_protocol::ThreadItem::untimed(item.clone()));
    chat.replay_thread_turns(vec![turn], ReplayKind::ThreadSnapshot);
    let mut observations = 0;
    let retained = take_history(&mut events);
    for cell in &retained {
        if let Some(pending) = cell.as_any().downcast_ref::<AlabPendingCommand>() {
            assert!(pending.matches(thread, "turn-1", "restored-read"));
            assert!(
                lines_to_single_string(&cell.transcript_lines(80))
                    .contains("preview retained across the finding")
            );
            observations += 1;
        }
    }
    assert_eq!(observations, 1);
    assert!(chat.has_misalignment_policy_violation());
    let AppServerThreadItemPayload::CommandExecution {
        status,
        exit_code,
        aggregated_output,
        ..
    } = &mut item
    else {
        panic!("expected read");
    };
    *status = result;
    *exit_code = code;
    *aggregated_output = None;
    chat.handle_server_notification(
        ServerNotification::ItemCompleted(ItemCompletedNotification::new(
            thread.to_string(),
            "turn-1".to_owned(),
            item,
            None,
            0,
        )),
        None,
    );
    let mut corrections = 0;
    while let Ok(event) = events.try_recv() {
        if let AppEvent::AlabFindingCommandFinished { cell, .. } = event {
            let transcript = lines_to_single_string(&cell.transcript_lines(80));
            assert!(transcript.contains("preview retained across the finding"));
            assert!(
                transcript.lines().any(|line| line.trim() == expected),
                "{transcript}"
            );
            assert!(
                !transcript.contains("Command result pending"),
                "{transcript}"
            );
            if code.is_none() {
                assert!(
                    !transcript.contains('✗') && !transcript.contains('✓'),
                    "{transcript}"
                );
            }
            corrections += 1;
        }
    }
    assert_eq!(corrections, 1);
    assert!(chat.has_misalignment_policy_violation());
    assert_no_submit_op(&mut ops);
}

#[tokio::test]
async fn evicting_a_pending_row_releases_its_output_and_still_retires_the_live_terminal() {
    let (mut chat, mut events, mut ops) = make_chatwidget_manual(None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.set_feature_enabled(Feature::MisalignmentReview, true);
    handle_turn_started(&mut chat, "turn-1");
    let item = fixture_read(&chat, "evicted", "58", "discarded.fifo");
    handle_exec_begin(&mut chat, item.clone());
    let exec = chat
        .transcript
        .active_cell
        .as_mut()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<ExecCell>()
        .unwrap();
    exec.append_output("evicted", "partial output while the command is running");
    handle_error(
        &mut chat,
        ERROR_MESSAGE,
        Some(CodexErrorInfo::MisalignmentPolicyViolation),
    );
    let retained = take_history(&mut events);
    let pending = retained
        .iter()
        .find_map(|row| row.as_any().downcast_ref::<AlabPendingCommand>())
        .unwrap();
    assert!(lines_to_single_string(&pending.transcript_lines(80)).contains("partial output"));
    let original = std::sync::Arc::downgrade(pending.original().unwrap());
    assert!(original.upgrade().is_some());
    drop(retained);
    assert!(original.upgrade().is_none());
    assert_eq!(chat.unified_exec_processes.len(), 1);
    assert_eq!(chat.unified_exec_processes[0].key, "58");
    end_exec(&mut chat, item, "genuine result after eviction", "", 21);
    assert!(chat.unified_exec_processes.is_empty());
    assert!(chat.has_misalignment_policy_violation());
    assert!(!chat.bottom_pane.composer_input_enabled());
    while let Ok(event) = events.try_recv() {
        assert!(!matches!(
            event,
            AppEvent::AlabFindingCommandFinished { .. } | AppEvent::InsertHistoryCell(_)
        ));
    }
    assert_no_submit_op(&mut ops);
}
