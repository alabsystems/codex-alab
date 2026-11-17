// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real original widget/data controls, not primary-commit permission tests.
use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn held_confirmation_keeps_original_snapshot_without_overwriting_new_editor_text() {
    let (mut widget, _sender, mut events, mut operations) =
        crate::boxed_future!(crate::chatwidget::tests::make_chatwidget_manual_with_sender()).await;
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}
    assert!(!widget.is_session_configured());
    widget.apply_external_edit("actual originally confirmed draft".into());
    let original = widget.bottom_pane.composer_draft_snapshot();
    widget.apply_external_edit("new actual human edits".into());
    widget
        .alab_validate_configured_input_destination(true, true)
        .unwrap();
    widget.alab_install_configured_inputs(Some(None), Some(original.clone()));
    assert_eq!(widget.bottom_pane.composer_text(), "new actual human edits");
    assert_eq!(
        widget.input_queue.startup_submission.as_ref(),
        Some(&original)
    );
    assert!(widget.initial_user_message.is_none());
    assert!(operations.try_recv().is_err());
}

#[tokio::test]
async fn newer_real_initial_message_is_preserved_before_taking_held_inputs() {
    let (mut widget, _sender, _events, _operations) =
        crate::boxed_future!(crate::chatwidget::tests::make_chatwidget_manual_with_sender()).await;
    widget.initial_user_message = Some(UserMessage::from("new actual message"));
    let before = widget.alab_capture_retained_messages().unwrap();
    assert!(
        widget
            .alab_validate_configured_input_destination(true, false)
            .is_err()
    );
    assert_eq!(
        widget.alab_capture_retained_messages().unwrap().get(),
        before.get()
    );
    assert_eq!(
        widget.initial_user_message,
        Some(UserMessage::from("new actual message"))
    );
}

#[tokio::test]
async fn original_escape_with_unchanged_text_is_not_revived_by_later_initial_message_release() {
    let (mut widget, _sender, _events, mut operations) =
        crate::boxed_future!(crate::chatwidget::tests::make_chatwidget_manual_with_sender()).await;
    while operations.try_recv().is_ok() {}
    widget.apply_external_edit("same actually confirmed text".into());
    widget.input_queue.startup_submission = Some(widget.bottom_pane.composer_draft_snapshot());
    assert!(
        widget.handle_startup_submission_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE
        ))
    );
    assert!(widget.input_queue.startup_submission.is_none());
    let current = widget.bottom_pane.composer_text();
    widget.alab_install_configured_inputs(Some(None), None);
    assert_eq!(widget.bottom_pane.composer_text(), current);
    assert!(widget.input_queue.startup_submission.is_none());
    assert!(operations.try_recv().is_err());
}
