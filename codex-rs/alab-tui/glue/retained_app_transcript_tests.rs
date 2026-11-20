// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual App fixture and original queue/tail operations; this is presentation
//! capture coverage, not a constructed Native admission or complete-App claim.
use super::*;
use crate::bottom_pane::alab_retained_editors as editors;
const LIMIT: usize = 8 * 1024 * 1024;

#[tokio::test]
async fn retained_actual_app_transcript_capture_preserves_queue_tail_and_replay_without_draining()
-> io::Result<()> {
    let (mut app, _events, _ops) =
        crate::boxed_future!(crate::app::tests::make_test_app_with_channels()).await;
    let source: Arc<dyn HistoryCell> = Arc::new(crate::history_cell::AgentMarkdownCell::new(
        "actual App history source".into(),
        std::path::Path::new("."),
    ));
    app.transcript_cells.push(source.clone());
    app.native_history.defer(&source);
    let line =
        crate::terminal_hyperlinks::HyperlinkLine::from(Line::from("pending original native line"));
    app.last_rendered_history_tail = Some(history_ui::RenderedHistoryTail {
        cell: Arc::downgrade(&source),
        lines: vec![line.clone()],
    });
    app.deferred_history_lines.push(line.clone());
    app.initial_history_replay_buffer = Some(InitialHistoryReplayBuffer {
        retained_lines: VecDeque::from([line]),
        render_from_transcript_tail: true,
        was_truncated: true,
    });
    app.pending_thread_switch_resets = 7;
    app.scrollback_has_older_history = true;
    app.transcript_reflow.note_width(71);
    app.transcript_reflow.schedule_debounced(Some(42));
    let mut cells = cells::Capture::default();
    let mut layouts = views::layouts::Capture::default();
    let mut graph = views::CaptureGraph::new(&mut cells, &mut layouts);
    let mut scopes = ScopeCapture::default();
    // This native fixture has no selection/copy guard; no live channel or Native
    // permission is fabricated to capture its presentation-only component.
    let mut guards = guards::Capture::default();
    let data = app.alab_capture_transcript(&mut graph, &mut scopes, &mut guards)?;
    let wire: serde_json::Value =
        serde_json::from_slice(&editors::encode(&data, LIMIT)?).map_err(invalid)?;
    let decoded: Data<TextArea> =
        editors::decode(&serde_json::to_vec(&wire).map_err(invalid)?, LIMIT, 128)?;
    assert_eq!(decoded.transcript_cells.len(), 1);
    assert_eq!(decoded.pending_thread_switch_resets, 7);
    assert!(decoded.scrollback_has_older_history);
    assert_eq!(decoded.deferred_history_lines.len(), 1);
    assert!(
        decoded
            .initial_history_replay_buffer
            .as_ref()
            .unwrap()
            .was_truncated
    );
    assert!(app.native_history.contains(&source));
    assert!(Arc::ptr_eq(&source, &app.transcript_cells[0]));
    assert!(
        app.last_rendered_history_tail
            .as_ref()
            .unwrap()
            .cell
            .ptr_eq(&Arc::downgrade(&source))
    );
    assert_eq!(
        wire["body"]["last_rendered_history_tail"]["cell"],
        wire["body"]["transcript_cells"][0]
    );
    assert_eq!(
        wire["body"]["native_history"]["pending"][0]["source"],
        wire["body"]["transcript_cells"][0]
    );
    assert_eq!(
        wire["body"]["native_history"]["pending"][0]["owner"],
        wire["body"]["transcript_cells"][0]
    );
    for field in [
        "last_rendered_history_tail",
        "last_thread_usage_status_cell",
        "initial_history_replay_buffer",
    ] {
        let mut omitted = wire.clone();
        omitted["body"].as_object_mut().unwrap().remove(field);
        assert!(
            editors::decode::<Data<TextArea>>(
                &serde_json::to_vec(&omitted).map_err(invalid)?,
                LIMIT,
                128
            )
            .is_err(),
            "{field}"
        );
    }
    Ok(())
}

#[path = "retained_provider_work_tests.rs"]
mod provider_work_controls;

#[path = "retained_managed_worktree_work_tests.rs"]
mod managed_worktree_work_controls;

#[path = "retained_feature_save_work_tests.rs"]
mod feature_save_work_controls;

#[path = "retained_feature_fetch_work_tests.rs"]
mod feature_fetch_work_controls;
