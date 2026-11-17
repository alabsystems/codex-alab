// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original canonical reducer + occurrence DATA controls only. No recorded
//! Core resource, Source acceptance, Native frontier or UI ACK is fabricated.
use super::*;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AgentMessageEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_rollout::ResponseItemEnvelope;

fn start(id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: id.into(),
        turn_attribution: None,
        root_turn_id: None,
        trace_id: None,
        started_at: Some(1),
        model_context_window: None,
        collaboration_mode_kind: Default::default(),
    }))
}
fn complete(id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
        root_turn_id: None,
        turn_id: id.into(),
        last_agent_message: None,
        error: None,
        started_at: Some(1),
        completed_at: Some(2),
        duration_ms: Some(1000),
        time_to_first_token_ms: None,
    }))
}
fn message(text: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::AgentMessage(AgentMessageEvent {
        moderation: None,
        message: text.into(),
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    }))
}
fn marker_item(marker: &RetainedSideMarker) -> Result<RolloutItem, Box<dyn std::error::Error>> {
    let item: ResponseItem = serde_json::from_value(serde_json::json!({
        "type":"message","role":"user","content":[{"type":"input_text","text":"Side conversation boundary. Ordinary text is not this structured occurrence."}]
    }))?;
    let mut envelope = ResponseItemEnvelope::from(item);
    envelope.metadata.get_or_insert_default().alab_side_boundary = Some(marker.clone());
    Ok(RolloutItem::ResponseItem(envelope))
}
fn ordinary(records: &[RolloutItem], mode: ThreadHistoryMode) -> Vec<Turn> {
    let mut builder = codex_app_server_protocol::ThreadHistoryBuilder::new();
    for record in records {
        if let Some(item) = codex_rollout::persisted_rollout_item(record, mode) {
            builder.handle_retained_history_item(item.as_ref(), mode);
        }
    }
    builder.finish()
}

#[test]
fn legacy_cut_keeps_original_full_reducer_ids_without_showing_inherited_prefix()
-> Result<(), Box<dyn std::error::Error>> {
    let marker = RetainedSideMarker::new_local("1".repeat(32))?;
    let records = vec![
        start("parent"),
        message("inherited parent answer"),
        complete("parent"),
        marker_item(&marker)?,
        start("child"),
        message("actual child answer"),
        complete("child"),
    ];
    let full = ordinary(&records, ThreadHistoryMode::Legacy);
    let actual = build_data(
        &records,
        ThreadHistoryMode::Legacy,
        &marker,
        "unused",
        false,
    )?;
    let expected = full
        .iter()
        .find(|turn| turn.id == "child")
        .ok_or("original child turn missing")?;
    assert_eq!(actual, vec![expected.clone()]);
    assert_eq!(actual[0].items[0].payload.id(), "item-2");
    assert!(!serde_json::to_string(&actual)?.contains("inherited parent answer"));
    Ok(())
}

#[test]
fn mid_turn_cut_retains_actual_turn_state_and_post_cut_original_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let marker = RetainedSideMarker::new_local("2".repeat(32))?;
    let records = vec![
        start("same-actual-turn"),
        message("before actual occurrence"),
        marker_item(&marker)?,
        message("after actual occurrence"),
    ];
    let full = ordinary(&records, ThreadHistoryMode::Legacy);
    let mut expected = full.first().ok_or("original active turn missing")?.clone();
    expected.items.remove(0);
    let actual = build_data(
        &records,
        ThreadHistoryMode::Legacy,
        &marker,
        "same-actual-turn",
        true,
    )?;
    assert_eq!(actual, vec![expected]);
    assert_eq!(actual[0].items[0].payload.id(), "item-2");
    let empty = &records[..3];
    let actual = build_data(
        empty,
        ThreadHistoryMode::Legacy,
        &marker,
        "same-actual-turn",
        true,
    )?;
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].id, "same-actual-turn");
    assert!(actual[0].items.is_empty());
    assert!(
        build_data(
            empty,
            ThreadHistoryMode::Legacy,
            &marker,
            "same-actual-turn",
            false
        )?
        .is_empty()
    );
    Ok(())
}

#[test]
fn paginated_first_seen_cut_hides_updates_to_pre_boundary_items_and_keeps_real_new_item()
-> Result<(), Box<dyn std::error::Error>> {
    use codex_protocol::items::ReasoningItem;
    use codex_protocol::items::TurnItem;
    use codex_protocol::protocol::ItemCompletedEvent;
    let marker = RetainedSideMarker::new_local("3".repeat(32))?;
    let item = |id: &str, text: &str| {
        RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
            content_token_count: None,
            thread_id: ThreadId::default(),
            turn_id: "actual-active".into(),
            item: TurnItem::Reasoning(ReasoningItem {
                id: id.into(),
                summary_text: vec![text.into()],
                raw_content: Vec::new(),
            }),
            started_at_ms: Some(1),
            completed_at_ms: 2,
        }))
    };
    let records = vec![
        start("actual-active"),
        item("old", "before"),
        marker_item(&marker)?,
        item("old", "later completion of the old item"),
        item("new", "actual post-boundary item"),
    ];
    let full = ordinary(&records, ThreadHistoryMode::Paginated);
    let expected = full
        .first()
        .ok_or("original canonical turn missing")?
        .items
        .iter()
        .find(|item| item.payload.id() == "new")
        .ok_or("original new canonical item missing")?
        .clone();
    let actual = build_data(
        &records,
        ThreadHistoryMode::Paginated,
        &marker,
        "actual-active",
        true,
    )?;
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].items, vec![expected]);
    Ok(())
}

#[test]
fn text_or_absence_or_ambiguous_occurrence_never_becomes_an_empty_cut_success()
-> Result<(), Box<dyn std::error::Error>> {
    let marker = RetainedSideMarker::new_local("4".repeat(32))?;
    let text_only = vec![
        start("old"),
        message("Side conversation boundary."),
        complete("old"),
    ];
    assert!(build_data(&text_only, ThreadHistoryMode::Legacy, &marker, "old", false).is_err());
    assert!(build_data(&[], ThreadHistoryMode::Legacy, &marker, "old", false).is_err());
    let duplicate = vec![marker_item(&marker)?, marker_item(&marker)?];
    assert!(build_data(&duplicate, ThreadHistoryMode::Legacy, &marker, "old", false).is_err());
    Ok(())
}
