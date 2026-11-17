// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower ORIGINAL cell/store UI controls only. These construct no Native
//! origin/frontier, no Core, no Pending/replay receipt and no UI ACK grant.
use super::*;
use crate::app::alab_side_history_data::Provenance;
use crate::app::side_companion::SideCompanion;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnItemsView;
use codex_utils_absolute_path::AbsolutePathBuf;
use ratatui::text::Line;

fn message(item: &str, text: &str) -> ThreadItemPayload {
    ThreadItemPayload::AgentMessage {
        moderation: None,
        id: item.into(),
        text: text.into(),
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    }
}
fn key(turn: &str, item: &str) -> Source {
    Source::Item {
        turn: turn.into(),
        item: item.into(),
    }
}
fn plain(text: &str) -> Arc<dyn HistoryCell> {
    Arc::new(crate::history_cell::PlainHistoryCell::new(vec![
        Line::from(text.to_owned()),
    ]))
}
fn delta(pane: &mut SideCompanion, store: &ThreadEventStore, turn: &str, item: &str, text: &str) {
    let notification = ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
        thread_id: ThreadId::new().to_string(),
        turn_id: turn.into(),
        item_id: item.into(),
        delta: text.into(),
    });
    assert!(
        pane.apply_notification(&notification, store, || unreachable!(
            "answer delta never constructs status"
        ))
    );
    pane.flush_answer();
}
fn complete(
    pane: &mut SideCompanion,
    store: &ThreadEventStore,
    turn: &str,
    item: &str,
    text: &str,
) {
    let notification = ServerNotification::ItemCompleted(ItemCompletedNotification::new(
        ThreadId::new().to_string(),
        turn.into(),
        message(item, text),
        None,
        0,
    ));
    assert!(
        pane.apply_notification(&notification, store, || unreachable!(
            "answer completion never constructs status"
        ))
    );
}
#[test]
fn original_answer_update_rebinds_real_cell_range_without_losing_draft_or_view() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut pane = SideCompanion::empty(AbsolutePathBuf::from_absolute_path(home.path())?);
    pane.display
        .draft
        .set_text_clearing_elements("keep exact original draft");
    pane.view.history = TranscriptHistoryState::Partial;
    let store = ThreadEventStore::new(32);
    delta(&mut pane, &store, "turn-a", "same", "partial");
    let old = pane.cells[0].clone();
    let ledger = pane.display.alab_history.clone();
    assert_eq!(
        ledger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .range(&key("turn-a", "same"), &pane.cells)?,
        Some(0..1)
    );
    complete(
        &mut pane,
        &store,
        "turn-a",
        "same",
        "authoritative completed answer",
    );
    assert_eq!(pane.cells.len(), 1);
    assert!(!Arc::ptr_eq(&old, &pane.cells[0]));
    assert!(Arc::ptr_eq(&ledger, &pane.display.alab_history));
    let guard = ledger.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(
        guard.payloads(&key("turn-a", "same")),
        Some([message("same", "authoritative completed answer")].as_slice())
    );
    guard.checkpoint(&pane.cells, 0)?;
    assert_eq!(pane.display.draft.text(), "keep exact original draft");
    assert_eq!(pane.view.history, TranscriptHistoryState::Partial);
    Ok(())
}
#[test]
fn actual_splices_move_scoped_ranges_and_retire_only_removed_real_cells() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut pane = SideCompanion::empty(AbsolutePathBuf::from_absolute_path(home.path())?);
    let store = ThreadEventStore::new(32);
    delta(&mut pane, &store, "first", "reused", "one");
    complete(&mut pane, &store, "first", "reused", "one");
    delta(&mut pane, &store, "second", "reused", "two");
    complete(&mut pane, &store, "second", "reused", "two");
    let ledger = pane.display.alab_history.clone();
    let before = plain("actual older projected cell");
    pane.splice_older_cells(0..0, vec![before.clone()]);
    ledger
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .record(vec![key("older", "reused")], Vec::new(), &[before])?;
    assert_eq!(
        ledger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .range(&key("second", "reused"), &pane.cells)?,
        Some(2..3)
    );
    pane.splice_older_cells(1..2, Vec::new());
    let guard = ledger.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(guard.range(&key("first", "reused"), &pane.cells)?, None);
    assert_eq!(
        guard.range(&key("second", "reused"), &pane.cells)?,
        Some(1..2)
    );
    guard.checkpoint(&pane.cells, 0)?;
    Ok(())
}
#[test]
fn grouped_source_range_cannot_erase_an_uncovered_real_item() -> Result<()> {
    let a = plain("grouped tools");
    let cells = vec![a.clone()];
    let mut ledger = Provenance::default();
    ledger.record(vec![key("t", "one"), key("t", "two")], Vec::new(), &cells)?;
    assert!(ledger.replacement(&[key("t", "one")], &cells).is_err());
    assert!(
        ledger
            .record(vec![key("t", "one")], Vec::new(), &[plain("replacement")])
            .is_err()
    );
    assert_eq!(
        ledger.replacement(&[key("t", "one"), key("t", "two")], &cells)?,
        Some(0..1)
    );
    assert!(Arc::ptr_eq(&a, &cells[0]));
    Ok(())
}
#[test]
fn transferred_data_or_same_text_new_arc_does_not_manufacture_original_provenance() -> Result<()> {
    let mut ledger = Provenance::default();
    let cells = vec![plain("same text")];
    ledger.record(vec![key("real", "item")], Vec::new(), &cells)?;
    let checkpoint = ledger.checkpoint(&cells, 0)?;
    let wire = serde_json::to_value(&checkpoint)?;
    let decoded = serde_json::from_value(wire.clone())?;
    Provenance::validate_transfer(&decoded, &cells)?;
    assert!(Provenance::default().checkpoint(&cells, 0).is_err());
    assert!(ledger.checkpoint(&[plain("same text")], 0).is_err());
    let mut malformed = wire;
    malformed["groups"][0]["end"] = serde_json::json!(99);
    assert!(Provenance::validate_transfer(&serde_json::from_value(malformed)?, &cells).is_err());
    Ok(())
}
#[tokio::test]
async fn original_historical_projector_populates_same_passive_ledger_and_preserves_focus()
-> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    let id = ThreadId::new();
    let original_focus = app.chat_widget.thread_id();
    let cwd = app.config.cwd.clone();
    let mut pane = SideCompanion::empty(cwd.clone());
    pane.display
        .draft
        .set_text_clearing_elements("passive draft stays");
    pane.view.history = TranscriptHistoryState::Partial;
    let ledger = pane.display.alab_history.clone();
    app.side_companions.insert(id, pane);
    let turns = ["a", "b"]
        .into_iter()
        .map(|turn| Turn {
            id: turn.into(),
            root_turn_id: None,
            items: vec![ThreadItem::untimed(message("same-item", turn))],
            items_view: TurnItemsView::Full,
            status: codex_app_server_protocol::TurnStatus::InProgress,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
        })
        .collect::<Vec<_>>();
    let items = turns
        .iter()
        .flat_map(|turn| {
            turn.items.iter().map(|item| HistoryPageItem {
                turn_id: turn.id.clone(),
                payload: item.payload.clone(),
            })
        })
        .collect();
    let hidden = HashSet::new();
    let cells = app.project_older_history_cells(
        items,
        &turns,
        &hidden,
        id,
        &cwd,
        RawReasoningVisibility::Hidden,
    );
    let pane = app
        .side_companions
        .get_mut(&id)
        .ok_or_else(|| eyre!("actual companion disappeared"))?;
    pane.splice_older_cells(0..0, cells);
    let guard = ledger.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(guard.range(&key("a", "same-item"), &pane.cells)?.is_some());
    assert!(guard.range(&key("b", "same-item"), &pane.cells)?.is_some());
    guard.checkpoint(&pane.cells, 0)?;
    assert!(Arc::ptr_eq(&ledger, &pane.display.alab_history));
    assert_eq!(pane.display.draft.text(), "passive draft stays");
    assert_eq!(pane.view.history, TranscriptHistoryState::Partial);
    assert_eq!(app.chat_widget.thread_id(), original_focus);
    Ok(())
}

#[test]
fn buffered_item_without_real_passive_render_does_not_count_as_applied_snapshot() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut pane = SideCompanion::empty(AbsolutePathBuf::from_absolute_path(home.path())?);
    let store = ThreadEventStore::new(32);
    let ledger = pane.display.alab_history.clone();
    let before = ledger
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .revision()?;
    let item = ThreadItemPayload::UserMessage {
        id: "real-user-item".into(),
        client_id: None,
        content: vec![codex_app_server_protocol::UserInput::Text {
            text: "visible saved user input".into(),
            text_elements: Vec::new(),
        }],
    };
    let notification = ServerNotification::ItemCompleted(ItemCompletedNotification::new(
        ThreadId::new().to_string(),
        "user-turn".into(),
        item,
        None,
        0,
    ));
    assert!(
        !pane.apply_notification(&notification, &store, || unreachable!(
            "user item does not create work"
        ))
    );
    assert!(
        ledger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .changed_since(before)?
            .is_empty()
    );
    // A later genuine historical projector must still apply this item. The
    // mere existence of its buffered notification does not suppress it.
    assert!(pane.cells.is_empty());
    Ok(())
}

#[tokio::test]
async fn missing_history_is_merged_around_a_real_newer_b_group_in_original_order() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    let id = ThreadId::new();
    let focus = app.chat_widget.thread_id();
    let cwd = app.config.cwd.clone();
    let mut pane = SideCompanion::empty(cwd.clone());
    pane.display
        .draft
        .set_text_clearing_elements("retain passive draft");
    let ledger = pane.display.alab_history.clone();
    let baseline = ledger
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .revision()?;
    let store = ThreadEventStore::new(32);
    delta(&mut pane, &store, "shared", "b", "B live");
    complete(&mut pane, &store, "shared", "b", "B live");
    let original_b = pane.cells[0].clone();
    let changed = ledger
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .changed_since(baseline)?;
    assert!(changed.contains(&key("shared", "b")));
    app.side_companions.insert(id, pane);
    let turns = vec![Turn {
        id: "shared".into(),
        root_turn_id: None,
        items: vec![
            ThreadItem::untimed(message("a", "A snapshot")),
            ThreadItem::untimed(message("b", "B old snapshot")),
            ThreadItem::untimed(message("c", "C snapshot")),
        ],
        items_view: TurnItemsView::Full,
        status: codex_app_server_protocol::TurnStatus::InProgress,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }];
    let runs = {
        let history = ledger.lock().unwrap_or_else(PoisonError::into_inner);
        ordered_runs(
            &turns,
            &changed,
            &history,
            &app.side_companions
                .get(&id)
                .ok_or_else(|| eyre!("actual pane missing"))?
                .cells,
        )?
    };
    for run in runs {
        app.alab_side_history_projection.active = Some((id, Vec::new()));
        let _original_cells = app.project_older_history_cells(
            run.items,
            &turns,
            &HashSet::new(),
            id,
            &cwd,
            RawReasoningVisibility::Hidden,
        );
        let (_, groups) = app
            .alab_side_history_projection
            .active
            .take()
            .ok_or_else(|| eyre!("actual historical producer missing"))?;
        apply_ordered_groups(
            app.side_companions
                .get_mut(&id)
                .ok_or_else(|| eyre!("actual pane missing"))?,
            run.after.as_ref(),
            run.before.as_ref(),
            groups,
        )?;
    }
    let pane = app
        .side_companions
        .get(&id)
        .ok_or_else(|| eyre!("actual pane missing"))?;
    let text = pane
        .cells
        .iter()
        .filter_map(|cell| cell.copy_source().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(text, vec!["A snapshot", "B live", "C snapshot"]);
    assert!(Arc::ptr_eq(&pane.cells[1], &original_b));
    ledger
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .checkpoint(&pane.cells, 0)?;
    assert_eq!(pane.display.draft.text(), "retain passive draft");
    assert_eq!(app.chat_widget.thread_id(), focus);
    Ok(())
}
