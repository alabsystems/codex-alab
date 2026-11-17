// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Canonical display policy controls only, never Native/UI frontier receipts.
use super::*;
use codex_protocol::items::ReasoningItem;
use codex_protocol::items::TurnItem as CoreItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::WarningEvent;
#[test]
fn actual_canonical_mode_keeps_items_and_filters_private_transient_records()
-> Result<(), &'static str> {
    let turn = "actual-memory-turn".to_owned();
    let completed = ItemCompletedEvent {
        content_token_count: None,
        thread_id: ThreadId::default(),
        turn_id: turn.clone(),
        item: CoreItem::Reasoning(ReasoningItem {
            id: "reasoning".into(),
            summary_text: vec!["visible summary".into()],
            raw_content: vec!["visible raw policy".into()],
        }),
        started_at_ms: Some(1),
        completed_at_ms: 2,
    };
    let records = vec![
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: turn,
            turn_attribution: None,
            root_turn_id: None,
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
        RolloutItem::EventMsg(EventMsg::Warning(WarningEvent {
            message: "private-transient-control-marker".into(),
        })),
        RolloutItem::EventMsg(EventMsg::ItemCompleted(completed.clone())),
    ];
    let actual = memory_turns(&records, ThreadHistoryMode::Paginated);
    let only = actual.first().ok_or("missing actual canonical turn")?;
    assert_eq!(
        only.items,
        vec![codex_app_server_protocol::ThreadItem::from(&completed)]
    );
    let encoded = serde_json::to_string(&actual).map_err(|_| "display encode failed")?;
    assert!(!encoded.contains("private-transient-control-marker"));
    // The ordinary Legacy persistence policy intentionally excludes this
    // canonical reasoning item. Applying it to Paginated memory lost the item.
    assert!(
        memory_turns(&records, ThreadHistoryMode::Legacy)
            .first()
            .is_none_or(|turn| turn.items.is_empty())
    );
    Ok(())
}
