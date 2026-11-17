// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original overlay controls. The bool mask below is lower DATA only;
//! no CallbackObservation, Source acceptance or Native grant is fabricated.
use super::*;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use tokio::sync::mpsc::unbounded_channel;
const BUDGET: usize = 4 * 1024 * 1024;
fn request(item: &str) -> ToolRequestUserInputParams {
    ToolRequestUserInputParams {
        thread_id: "00000000-0000-0000-0000-000000000001".into(),
        turn_id: "turn".into(),
        item_id: item.into(),
        is_blocking: true,
        auto_resolution_ms: None,
        questions: vec![ToolRequestUserInputQuestion {
            id: "question".into(),
            header: "Notes".into(),
            question: "Original question".into(),
            is_other: false,
            is_secret: false,
            options: None,
        }],
    }
}
fn overlay(
    params: ToolRequestUserInputParams,
) -> (
    RequestUserInputOverlay,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) {
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
fn closed_queued_request_does_not_discard_current_editor_or_new_appended_request() {
    let (mut old, mut old_events) = overlay(request("A"));
    old.queue.extend([request("B"), request("C")]);
    old.composer
        .set_text_content("saved exact A notes".into(), Vec::new(), Vec::new());
    while old_events.try_recv().is_ok() {} // discard only original test setup events, before capture
    let raw = old.alab_capture_retained_view(BUDGET).unwrap();
    let kept = retain_request_editing(&raw, &[true, false, true], BUDGET, 1024)
        .unwrap()
        .unwrap();
    let (mut current, mut events) = overlay(request("A"));
    current.queue.extend([request("C"), request("new-D")]);
    PreparedUserInput::decode(&kept, BUDGET, 1024)
        .unwrap()
        .install(&mut current, &RuntimeKeymap::defaults(), BUDGET, 1024)
        .unwrap();
    assert_eq!(
        current.composer.recovery_snapshot().text,
        "saved exact A notes"
    );
    assert_eq!(
        current
            .queue
            .iter()
            .map(|request| request.item_id.as_str())
            .collect::<Vec<_>>(),
        ["C", "new-D"]
    );
    assert!(events.try_recv().is_err());
    assert!(old_events.try_recv().is_err());
}
#[test]
fn closed_current_request_never_transplants_its_editor_into_real_successor() {
    let (mut old, _old_events) = overlay(request("A"));
    old.queue.push_back(request("B"));
    old.composer
        .set_text_content("A private notes".into(), Vec::new(), Vec::new());
    let raw = old.alab_capture_retained_view(BUDGET).unwrap();
    let (mut actual_b, mut events) = overlay(request("B"));
    actual_b
        .composer
        .set_text_content("new real B editing".into(), Vec::new(), Vec::new());
    while events.try_recv().is_ok() {} // original new-B setup, before the tested no-transfer decision
    assert!(
        retain_request_editing(&raw, &[false, true], BUDGET, 1024)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        actual_b.composer.recovery_snapshot().text,
        "new real B editing"
    );
    assert!(events.try_recv().is_err());
}
#[test]
fn actual_queue_order_and_existing_input_cannot_be_overwritten_by_saved_prefix() {
    let (mut old, _old_events) = overlay(request("A"));
    old.queue.push_back(request("B"));
    let raw = old.alab_capture_retained_view(BUDGET).unwrap();
    assert!(retain_request_editing(&raw, &[true], BUDGET, 1024).is_err());
    let (mut changed, mut events) = overlay(request("A"));
    changed.queue.extend([request("new-D"), request("B")]);
    assert!(
        PreparedUserInput::decode(&raw, BUDGET, 1024)
            .unwrap()
            .install(&mut changed, &RuntimeKeymap::defaults(), BUDGET, 1024)
            .is_err()
    );
    assert_eq!(changed.queue.front().unwrap().item_id, "new-D");
    assert!(events.try_recv().is_err());
}
