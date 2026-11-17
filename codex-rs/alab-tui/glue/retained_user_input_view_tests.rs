// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::bottom_pane::chat_composer_history::HistoryEntry;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::unbounded_channel;

const BUDGET: usize = 4 * 1024 * 1024;
fn request(item: &str) -> ToolRequestUserInputParams {
    ToolRequestUserInputParams {
        thread_id: "00000000-0000-0000-0000-000000000001".into(),
        item_id: item.into(),
        turn_id: "original-turn".into(),
        questions: vec![ToolRequestUserInputQuestion {
            id: "original-question".into(),
            header: "Notes".into(),
            question: "Original prompt".into(),
            is_other: false,
            is_secret: false,
            options: None,
        }],
        is_blocking: true,
        auto_resolution_ms: None,
    }
}
fn view(
    params: ToolRequestUserInputParams,
) -> (RequestUserInputOverlay, UnboundedReceiver<AppEvent>) {
    let (tx, rx) = unbounded_channel();
    (
        RequestUserInputOverlay::new_with_keymap(
            params,
            AppEventSender::new(tx),
            true,
            false,
            true,
            RuntimeKeymap::defaults(),
        ),
        rx,
    )
}

#[test]
fn actual_form_search_cancellation_and_answer_use_original_new_view_sender() {
    let params = request("actual-request");
    let (mut source, mut old_events) = view(params.clone());
    source.focus = Focus::Notes;
    source
        .composer
        .record_local_user_message_history(HistoryEntry::new("needle previous notes".into()));
    source
        .composer
        .set_text_content("original draft under search".into(), Vec::new(), Vec::new());
    source.composer.move_cursor_to_end();
    source
        .composer
        .handle_key_event(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    source.composer.handle_key_event(KeyCode::Char('n').into());
    assert!(source.composer.history_search_active());
    while old_events.try_recv().is_ok() {}
    let raw = source.alab_capture_retained_view(BUDGET).unwrap();
    assert!(
        old_events.try_recv().is_err(),
        "capture must not answer the old callback"
    );

    let (mut destination, mut new_events) = view(params);
    PreparedUserInput::decode(&raw, BUDGET, 1024)
        .unwrap()
        .install(&mut destination, &RuntimeKeymap::defaults(), BUDGET, 1024)
        .unwrap();
    assert!(
        new_events.try_recv().is_err(),
        "restoration itself must send no answer"
    );
    assert!(destination.composer.history_search_active());
    // Real original search behavior must restore the suspended draft, rather
    // than leave only a text preview which happened to look correct on decode.
    source.composer.handle_key_event(KeyCode::Esc.into());
    destination.composer.handle_key_event(KeyCode::Esc.into());
    assert_eq!(
        destination.composer.recovery_snapshot().text,
        "original draft under search"
    );
    assert_eq!(
        destination.composer.recovery_snapshot().text,
        source.composer.recovery_snapshot().text
    );
    assert!(!destination.composer.history_search_active());
    destination.handle_key_event(KeyCode::Enter.into());
    assert!(
        std::iter::from_fn(|| new_events.try_recv().ok()).any(|event| matches!(
            event,
            AppEvent::SubmitThreadOp {
                op: crate::app_command::AppCommand::UserInputAnswer { .. },
                ..
            }
        ))
    );
    assert!(old_events.try_recv().is_err());
}

#[test]
fn changed_replayed_request_or_new_human_draft_is_retained_on_refusal() {
    let (mut source, _old_events) = view(request("original-request"));
    source
        .composer
        .set_text_content("saved notes".into(), Vec::new(), Vec::new());
    let raw = source.alab_capture_retained_view(BUDGET).unwrap();
    let (mut foreign, mut events) = view(request("different-actual-request"));
    assert!(
        PreparedUserInput::decode(&raw, BUDGET, 1024)
            .unwrap()
            .install(&mut foreign, &RuntimeKeymap::defaults(), BUDGET, 1024)
            .is_err()
    );
    assert!(foreign.composer.is_empty());
    assert!(events.try_recv().is_err());
    let (mut destination, mut events) = view(request("original-request"));
    destination
        .composer
        .set_text_content("new real human notes".into(), Vec::new(), Vec::new());
    assert!(
        PreparedUserInput::decode(&raw, BUDGET, 1024)
            .unwrap()
            .install(&mut destination, &RuntimeKeymap::defaults(), BUDGET, 1024)
            .is_err()
    );
    assert_eq!(
        destination.composer.recovery_snapshot().text,
        "new real human notes"
    );
    assert!(events.try_recv().is_err());
}

#[test]
fn exact_form_graph_and_bounds_are_required_before_view_mutation() {
    let (source, _events) = view(request("original-request"));
    let raw = source.alab_capture_retained_view(BUDGET).unwrap();
    assert!(source.alab_capture_retained_view(1).is_err());
    let mut value: serde_json::Value = serde_json::from_str(raw.get()).unwrap();
    value["current_idx"] = serde_json::json!(1);
    let changed = serde_json::value::to_raw_value(&value).unwrap();
    assert!(PreparedUserInput::decode(&changed, BUDGET, 1024).is_err());
    let mut value: serde_json::Value = serde_json::from_str(raw.get()).unwrap();
    value["callback"] = serde_json::json!({"authorized":true});
    let changed = serde_json::value::to_raw_value(&value).unwrap();
    assert!(PreparedUserInput::decode(&changed, BUDGET, 1024).is_err());
}
