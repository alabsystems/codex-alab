// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::app_event_sender::AppEventSender;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

const BUDGET: usize = 1024 * 1024;
fn pane() -> BottomPane {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut pane = crate::bottom_pane::tests::test_pane(AppEventSender::new(tx));
    pane.composer.set_disable_paste_burst(true);
    pane.composer.set_vim_enabled(true);
    pane
}
fn fill(pane: &mut BottomPane, text: &str) {
    pane.composer
        .set_text_content(format!("{text} "), Vec::new(), Vec::new());
    pane.composer.move_cursor_to_end();
    pane.composer
        .insert_selected_mention(0..0, "$skill", Some("/original/skill/SKILL.md"));
    pane.composer.attach_image(PathBuf::from("draft.png"));
    pane.composer
        .handle_paste("actual multi-line draft\n".repeat(400));
    pane.composer.popups.active = ActivePopup::None;
    pane.composer
        .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    pane.composer
        .handle_key_event(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE));
}
fn snapshot(pane: &BottomPane) -> serde_json::Value {
    serde_json::to_value(pane.composer.snapshot_draft()).unwrap()
}
#[test]
fn provisional_rich_draft_and_real_stash_round_trip_without_primary_identity() {
    let mut source = pane();
    fill(&mut source, "stashed");
    let saved = source.composer.alab_take_prompt(&source.keymap);
    fill(&mut source, "visible");
    let before = snapshot(&source);
    let slots = HashMap::from([(ThreadId::new(), saved)]);
    let bytes = capture_no_primary(&source, &slots, BUDGET).unwrap();
    assert_eq!(snapshot(&source), before);
    let prepared = BottomPane::alab_prepare_no_primary_checkpoint(&bytes, BUDGET, 1024).unwrap();
    let mut destination = pane();
    let mut restored = HashMap::new();
    prepared
        .install_into(&mut destination, &mut restored)
        .unwrap();
    assert_eq!(snapshot(&destination), before);
    assert_eq!(restored.len(), 1);
    assert!(destination.thread_id.is_none());
}
#[test]
fn primary_and_foreign_payloads_are_refused_before_editor_mutation() {
    let mut source = pane();
    fill(&mut source, "visible");
    let bytes = capture_no_primary(&source, &HashMap::new(), BUDGET).unwrap();
    let mut destination = pane();
    destination.set_history_metadata(ThreadId::new(), 0, 0);
    let before = snapshot(&destination);
    let mut empty = HashMap::new();
    assert!(
        BottomPane::alab_prepare_no_primary_checkpoint(&bytes, BUDGET, 1024)
            .unwrap()
            .install_into(&mut destination, &mut empty)
            .is_err()
    );
    assert_eq!(snapshot(&destination), before);
    assert!(capture_no_primary(&destination, &HashMap::new(), BUDGET).is_err());
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("primary".into(), serde_json::json!(ThreadId::new()));
    assert!(
        BottomPane::alab_prepare_no_primary_checkpoint(
            &serde_json::to_vec(&value).unwrap(),
            BUDGET,
            1024
        )
        .is_err()
    );
}
#[test]
fn occupied_stash_and_size_bounds_do_not_replace_actual_editor_owners() {
    let mut source = pane();
    fill(&mut source, "visible");
    assert!(capture_no_primary(&source, &HashMap::new(), 1).is_err());
    let bytes = capture_no_primary(&source, &HashMap::new(), BUDGET).unwrap();
    let mut destination = pane();
    fill(&mut destination, "saved");
    let saved = destination.composer.alab_take_prompt(&destination.keymap);
    fill(&mut destination, "destination");
    let before = snapshot(&destination);
    let mut slots = HashMap::from([(ThreadId::new(), saved)]);
    assert!(
        BottomPane::alab_prepare_no_primary_checkpoint(&bytes, BUDGET, 1024)
            .unwrap()
            .install_into(&mut destination, &mut slots)
            .is_err()
    );
    assert_eq!(snapshot(&destination), before);
    assert_eq!(slots.len(), 1);
}
