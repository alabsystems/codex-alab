// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::owned_transcript::tests::attach_thread;
use crate::app::test_support::make_test_app;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::ExecCell;
use crate::exec_cell::new_active_exec_command;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_protocol::parse_command::ParsedCommand;
use std::time::Duration;

fn completed(call_id: &str) -> Arc<dyn HistoryCell> {
    let source = CommandExecutionSource::UnifiedExecStartup;
    let mut cell = new_active_exec_command(
        call_id.to_owned(),
        vec!["cat".into(), "answer.txt".into()],
        vec![ParsedCommand::Read {
            cmd: "cat answer.txt".into(),
            name: "answer.txt".into(),
            path: "answer.txt".into(),
        }],
        source,
        None,
        false,
    );
    let output = CommandOutput::from_execution(
        CommandExecutionStatus::Completed,
        Some(0),
        source,
        Some(format!("completed output: {call_id}")),
    );
    assert!(cell.complete_call(call_id, output, Duration::from_secs(1)));
    Arc::new(cell)
}

fn deliver(app: &mut App, tui: &mut Tui, thread: ThreadId, turn: &str, call: &str) -> Result<()> {
    app.alab_finish_finding_command(
        tui,
        thread,
        turn,
        call,
        completed(call),
        #[cfg(unix)]
        None,
    )
}

#[tokio::test]
async fn finding_command_corrects_one_original_row_after_intervening_history() -> Result<()> {
    for owned in [true, false] {
        let mut app = make_test_app().await;
        let thread = ThreadId::new();
        attach_thread(&mut app, thread);
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(owned)?;

        let pending: Arc<dyn HistoryCell> = Arc::new(AlabPendingCommand::new(
            thread,
            "turn-one".into(),
            "read".into(),
            "cat answer.txt".into(),
        ));
        let sibling = completed("earlier");
        let finding: Arc<dyn HistoryCell> = Arc::new(crate::history_cell::new_error_event(
            "Provider refused this turn".into(),
        ));
        app.transcript_cells = vec![sibling.clone(), pending.clone(), finding.clone()];
        if owned {
            app.open_transcript_overlay(&mut tui);
        } else {
            app.overlay = Some(Overlay::new_transcript(
                app.transcript_cells.clone(),
                app.keymap.pager.clone(),
                false,
                1.0,
            ));
            app.native_history.defer(&pending);
            app.native_history.defer(&finding);
        }

        deliver(&mut app, &mut tui, ThreadId::new(), "turn-one", "read")?;
        deliver(&mut app, &mut tui, thread, "turn-two", "read")?;
        deliver(&mut app, &mut tui, thread, "turn-one", "other")?;
        assert!(Arc::ptr_eq(&app.transcript_cells[1], &pending));
        deliver(&mut app, &mut tui, thread, "turn-one", "read")?;
        let final_cell = app.transcript_cells[1].clone();
        let exec = final_cell.as_any().downcast_ref::<ExecCell>().unwrap();
        let calls: Vec<_> = exec.iter_calls().collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].call_id, "read");
        assert_eq!(calls[0].output.as_ref().unwrap().exit_code, Some(0));
        if !owned {
            assert!(!app.native_history.contains(&pending));
            assert!(app.native_history.contains(&final_cell));
        }
        deliver(&mut app, &mut tui, thread, "turn-one", "read")?;
        assert_eq!(app.transcript_cells.len(), 3);
        assert!(Arc::ptr_eq(&sibling, &app.transcript_cells[0]));
        assert!(Arc::ptr_eq(&final_cell, &app.transcript_cells[1]));
        assert!(Arc::ptr_eq(&finding, &app.transcript_cells[2]));

        let area = ratatui::layout::Rect::new(0, 0, 90, 25);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        if owned {
            app.transcript_view
                .render(area, &mut buffer, &app.transcript_cells);
        } else if let Some(Overlay::Transcript(overlay)) = &mut app.overlay {
            overlay.render(area, &mut buffer);
        }
        let visible = crate::app::owned_transcript::tests::buffer_text(&buffer);
        assert_eq!(
            visible.matches("completed output: read").count(),
            1,
            "{visible}"
        );
        assert!(visible.contains("Provider refused this turn"), "{visible}");
        assert!(!visible.contains("Command result pending"), "{visible}");

        app.transcript_cells.clear();
        deliver(&mut app, &mut tui, thread, "turn-one", "read")?;
        assert!(app.transcript_cells.is_empty());
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn restored_pending_row_accepts_only_its_genuine_late_completion() -> Result<()> {
    use crate::app_event::AppEvent;
    use codex_app_server_protocol::{
        ItemCompletedNotification, ServerNotification, ThreadItemPayload,
    };

    let (mut app, mut events, mut ops) =
        crate::boxed_future!(crate::app::tests::make_test_app_with_channels()).await;
    let thread = ThreadId::new();
    attach_thread(&mut app, thread);
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let source = CommandExecutionSource::UnifiedExecStartup;
    let command = "cat answer.txt";
    let native = new_active_exec_command(
        "read".into(),
        vec!["cat".into(), "answer.txt".into()],
        vec![],
        source,
        None,
        false,
    );
    let pending: Arc<dyn HistoryCell> = Arc::new(
        AlabPendingCommand::new(thread, "old-turn".into(), "read".into(), command.into())
            .with_original(Arc::new(native), Some("418".into())),
    );
    let finding: Arc<dyn HistoryCell> = Arc::new(crate::history_cell::new_error_event(
        "Provider refused this turn".into(),
    ));
    app.transcript_cells = vec![pending.clone(), finding.clone()];
    app.native_history.defer(&pending);
    app.alab_rebind_finding_commands();
    assert!(events.try_recv().is_err());
    assert!(ops.try_recv().is_err());

    let make_item = |process: &str| ThreadItemPayload::CommandExecution {
        model_context: None,
        sandbox_type: None,
        id: "read".into(),
        command: command.into(),
        cwd: app.config.cwd.clone().into(),
        process_id: Some(process.into()),
        plugin_id: None,
        script_path: None,
        source,
        status: CommandExecutionStatus::Completed,
        command_actions: vec![],
        aggregated_output: Some("actual read outcome".into()),
        exit_code: Some(0),
        duration_ms: Some(42),
    };
    let item = make_item("418");
    let mismatch = make_item("other-process");
    for (payload, expected) in [(mismatch, false), (item.clone(), true), (item, false)] {
        app.chat_widget.handle_server_notification(
            ServerNotification::ItemCompleted(ItemCompletedNotification::new(
                thread.to_string(),
                "old-turn".into(),
                payload,
                None,
                100,
            )),
            None,
        );
        let mut corrected = 0;
        while let Ok(event) = events.try_recv() {
            match event {
                AppEvent::AlabFindingCommandFinished {
                    thread_id,
                    turn_id,
                    call_id,
                    cell,
                    alab_source,
                } => {
                    let actual = cell.clone();
                    app.alab_finish_finding_command(
                        &mut tui,
                        thread_id,
                        &turn_id,
                        &call_id,
                        cell,
                        alab_source,
                    )?;
                    assert!(Arc::ptr_eq(&actual, &app.transcript_cells[0]));
                    corrected += 1;
                }
                AppEvent::InsertHistoryCell(_) => panic!("completion appended an unrelated row"),
                _ => {}
            }
        }
        assert_eq!(corrected, usize::from(expected));
    }
    assert_eq!(app.transcript_cells.len(), 2);
    assert!(Arc::ptr_eq(&finding, &app.transcript_cells[1]));
    let exec = app.transcript_cells[0]
        .as_any()
        .downcast_ref::<ExecCell>()
        .unwrap();
    let output = exec.iter_calls().next().unwrap().output.as_ref().unwrap();
    assert_eq!(output.exit_code, Some(0));
    assert!(
        exec.raw_lines()
            .iter()
            .any(|line| line.to_string().contains("actual read outcome"))
    );
    assert!(ops.try_recv().is_err());
    Ok(())
}
