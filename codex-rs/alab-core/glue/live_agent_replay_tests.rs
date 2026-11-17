// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use codex_protocol::ThreadId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::protocol::AgentMessageContentDeltaEvent;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ItemStartedEvent;

#[test]
fn actual_command_events_preserve_process_identity_output_and_retirement() {
    use codex_protocol::protocol::ExecCommandBeginEvent;
    use codex_protocol::protocol::ExecCommandEndEvent;
    use codex_protocol::protocol::ExecCommandOutputDeltaEvent;
    use codex_protocol::protocol::ExecOutputStream;
    let begin: ExecCommandBeginEvent = serde_json::from_value(serde_json::json!({
        "call_id":"command", "process_id":"1000", "turn_id":"turn", "started_at_ms":1234,
        "command":["/bin/sh","-c","cat"], "cwd":"file:///tmp", "parsed_cmd":[]
    }))
    .unwrap();
    let mut state = State::default();
    start(&mut state, "turn");
    state.track(
        "turn",
        &EventMsg::ExecCommandBegin(begin.clone()),
        MAX_BYTES,
    );
    let output = EventMsg::ExecCommandOutputDelta(ExecCommandOutputDeltaEvent {
        call_id: "command".into(),
        stream: ExecOutputStream::Stdout,
        chunk: b"before\n\xff".to_vec(),
    });
    state.track("another-turn", &output, MAX_BYTES);
    state.track("turn", &output, MAX_BYTES);
    state.track("turn", &EventMsg::ExecCommandBegin(begin), MAX_BYTES);
    let items = state
        .for_resume(
            ThreadId::new(),
            Some(&metadata("turn")),
            Some(&TurnItemsView::Full),
        )
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].started_at_ms, 1234);
    assert!(
        matches!(&items[0].item.payload, ThreadItemPayload::CommandExecution {
        id, process_id:Some(process), aggregated_output:Some(output), status:codex_app_server_protocol::CommandExecutionStatus::InProgress, ..
    } if id=="command" && process=="1000" && output=="before\n\u{fffd}")
    );
    let end: ExecCommandEndEvent = serde_json::from_value(serde_json::json!({
        "call_id":"command", "process_id":"1000", "turn_id":"turn", "completed_at_ms":1300,
        "command":["/bin/sh","-c","cat"], "cwd":"file:///tmp", "parsed_cmd":[],
        "exit_code":0, "duration":{"secs":0,"nanos":66000000}, "status":"completed"
    }))
    .unwrap();
    state.track("turn", &EventMsg::ExecCommandEnd(end), MAX_BYTES);
    assert!(state.snapshot("turn").unwrap().is_empty());
    assert_eq!(state.bytes, 0);
}
fn start(state: &mut State, turn: &str) {
    state.track(
        turn,
        &EventMsg::TurnStarted(
            serde_json::from_value(serde_json::json!({"turn_id":turn,"model_context_window":null}))
                .unwrap(),
        ),
        MAX_BYTES,
    );
}
fn agent(id: &str, text: &str) -> CoreItem {
    CoreItem::AgentMessage(AgentMessageItem {
        id: id.into(),
        content: vec![AgentMessageContent::Text { text: text.into() }],
        moderation: None,
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    })
}
fn began(turn: &str, id: &str) -> EventMsg {
    EventMsg::ItemStarted(ItemStartedEvent {
        thread_id: ThreadId::new(),
        turn_id: turn.into(),
        item: agent(id, ""),
        started_at_ms: 1234,
    })
}
fn delta(turn: &str, id: &str, text: &str) -> EventMsg {
    EventMsg::AgentMessageContentDelta(AgentMessageContentDeltaEvent {
        thread_id: "thread".into(),
        turn_id: turn.into(),
        item_id: id.into(),
        delta: text.into(),
    })
}
fn metadata(turn: &str) -> ThreadHistoryTurnMetadata {
    ThreadHistoryTurnMetadata {
        turn_id: turn.into(),
        root_turn_id: None,
        status: TurnStatus::InProgress,
        error: None,
        started_at: Some(1),
        completed_at: None,
        duration_ms: None,
    }
}
fn texts(items: &[ThreadItem]) -> Vec<&str> {
    items
        .iter()
        .filter_map(|item| match &item.payload {
            ThreadItemPayload::AgentMessage { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
#[test]
fn canonical_live_prefix_keeps_identity_timing_and_duplicate_start_does_not_reset_it() {
    let mut state = State::default();
    start(&mut state, "turn");
    state.track("turn", &began("turn", "message"), MAX_BYTES);
    state.track("turn", &delta("turn", "message", "α\"\n"), MAX_BYTES);
    state.track("turn", &began("turn", "message"), MAX_BYTES);
    state.track("turn", &delta("stale", "message", "wrong"), MAX_BYTES);
    state.track("turn", &delta("turn", "unknown", "wrong"), MAX_BYTES);
    let result = state.snapshot("turn").unwrap();
    assert_eq!(texts(&result), ["α\"\n"]);
    assert_eq!(result[0].id(), "message");
    assert_eq!(result[0].started_at_ms, Some(1234));
    assert_eq!(result[0].completed_at_ms, None);
}
#[test]
fn unfinished_items_keep_start_order_without_duplicating_completed_history() {
    let mut state = State::default();
    start(&mut state, "turn");
    for id in ["z", "b", "a"] {
        state.track("turn", &began("turn", id), MAX_BYTES);
        state.track("turn", &delta("turn", id, id), MAX_BYTES);
    }
    let complete = ItemCompletedEvent {
        thread_id: ThreadId::new(),
        turn_id: "turn".into(),
        item: agent("b", "finished-b"),
        content_token_count: None,
        started_at_ms: Some(1234),
        completed_at_ms: 2000,
    };
    state.track(
        "turn",
        &EventMsg::ItemCompleted(complete.clone()),
        MAX_BYTES,
    );
    let result = state.snapshot("turn").unwrap();
    assert_eq!(texts(&result), ["z", "a"]);
    assert_eq!(ThreadItem::from(&complete).completed_at_ms, Some(2000));
}
#[test]
fn views_listener_clear_and_new_turns_never_revive_old_partial_text() {
    let mut state = State::default();
    start(&mut state, "turn");
    state.track("turn", &began("turn", "a"), MAX_BYTES);
    let thread = ThreadId::new();
    for view in [TurnItemsView::Summary, TurnItemsView::NotLoaded] {
        assert!(
            state
                .for_resume(thread, Some(&metadata("turn")), Some(&view))
                .unwrap()
                .is_empty()
        );
    }
    let full = state
        .for_resume(thread, Some(&metadata("turn")), Some(&TurnItemsView::Full))
        .unwrap();
    assert_eq!(full.len(), 1);
    assert_eq!(full[0].thread_id, thread.to_string());
    assert_eq!(full[0].started_at_ms, 1234);
    let mut completed = metadata("turn");
    completed.status = TurnStatus::Completed;
    assert!(
        state
            .for_resume(thread, Some(&completed), Some(&TurnItemsView::Full))
            .unwrap()
            .is_empty()
    );
    start(&mut state, "next");
    assert!(state.snapshot("turn").unwrap().is_empty());
    state.track("next", &began("next", "b"), MAX_BYTES);
    state.clear();
    assert!(state.snapshot("next").unwrap().is_empty());
}

#[test]
fn size_accounting_handles_json_escaping_and_overflow_refuses_instead_of_truncating() {
    let text = "α\"\\\n\t\u{0001}";
    assert_eq!(
        escaped_bytes(text),
        serde_json::to_string(text).unwrap().len() - 2
    );
    let mut state = State::default();
    start(&mut state, "turn");
    state.track("turn", &began("turn", "a"), MAX_BYTES);
    let limit = state.bytes + 4;
    state.track("turn", &delta("turn", "a", "12345"), limit);
    assert!(state.live.is_empty());
    assert!(state.snapshot("turn").is_err());
    start(&mut state, "next");
    state.track("next", &began("next", "b"), MAX_BYTES);
    assert_eq!(state.snapshot("next").unwrap().len(), 1);
}
#[test]
fn completed_text_releases_the_live_budget_and_bounded_encoding_never_allocates_a_frame() {
    let mut state = State::default();
    start(&mut state, "turn");
    state.track("turn", &began("turn", "a"), MAX_BYTES);
    state.track(
        "turn",
        &delta("turn", "a", "long unfinished text"),
        MAX_BYTES,
    );
    let before = state.bytes;
    state.track(
        "turn",
        &EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id: ThreadId::new(),
            turn_id: "turn".into(),
            item: agent("a", "finished"),
            content_token_count: None,
            started_at_ms: Some(1234),
            completed_at_ms: 2000,
        }),
        MAX_BYTES,
    );
    assert!(state.bytes < before);
    assert!(state.live.is_empty());
    assert_eq!(encoded_len(&"a", 2), None);
    assert_eq!(encoded_len(&"a", 3), Some(3));
}
