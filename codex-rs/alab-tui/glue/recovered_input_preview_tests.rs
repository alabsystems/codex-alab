// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn refuse(chat: &mut ChatWidget) {
    handle_error(
        chat,
        "Request stopped",
        Some(CodexErrorInfo::MisalignmentPolicyViolation),
    );
}

#[tokio::test]
async fn finding_shows_delivery_and_manual_recall_for_each_input() {
    let (mut chat, _events, mut ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.set_feature_enabled(Feature::MisalignmentReview, false);
    handle_turn_started(&mut chat, "original");

    let mut keymap = RuntimeKeymap::defaults();
    let binding = crate::key_hint::alt(KeyCode::Up);
    keymap.chat.edit_queued_message = vec![binding];
    chat.chat_keymap = keymap.chat.clone();
    chat.bottom_pane.set_keymap_bindings(&keymap);
    let hint = format!(
        "{} edit last queued message",
        crate::key_hint::ShortcutHint::from(binding).display_label()
    );

    chat.input_queue
        .pending_steers
        .push_back(pending_steer("waiting for delivery"));
    chat.refresh_pending_input_preview();
    let healthy = render_bottom_popup(&chat, 105);
    assert!(
        healthy.contains("Messages to be submitted after next tool call"),
        "{healthy}"
    );
    assert!(healthy.contains("to interrupt and send immediately"));
    assert!(!healthy.contains(&hint));

    refuse(&mut chat);
    let pending = render_bottom_popup(&chat, 105);
    assert!(
        pending.contains("Messages awaiting delivery confirmation"),
        "{pending}"
    );
    assert!(pending.contains("waiting for delivery"));
    assert!(!pending.contains("interrupt and send immediately"));
    assert!(!pending.contains(&hint));
    assert!(!chat.maybe_send_next_queued_input());

    chat.alab_recover_undispatched_steer("test-submission");
    let held = render_bottom_popup(&chat, 105);
    assert!(
        held.contains("Held messages (automatic sending paused)"),
        "{held}"
    );
    assert!(held.contains("waiting for delivery"));
    assert!(!held.contains("submitted at end of turn"));
    assert!(held.contains(&hint));
    assert!(ops.try_recv().is_err());

    let saved = chat.capture_thread_input_state();
    let (mut reopened, _events, _ops) = make_chatwidget_manual(None).await;
    reopened.restore_thread_input_state(
        saved,
        ThreadInputStateRestoreMode {
            preserve_in_flight_turn: false,
        },
    );
    let first_frame = render_bottom_popup(&reopened, 105);
    assert!(
        first_frame.contains("Held messages (automatic sending paused)"),
        "{first_frame}"
    );
    assert!(first_frame.contains("waiting for delivery"));

    chat.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(
        chat.bottom_pane.composer_text_with_pending(),
        "waiting for delivery"
    );
    let recalled = render_bottom_popup(&chat, 105);
    assert!(!recalled.contains("automatic sending paused"));
    assert!(!recalled.contains(&hint));
    assert!(ops.try_recv().is_err());
}

#[tokio::test]
async fn fresh_image_preparation_stays_separate_from_held_input() {
    let (mut chat, _events, mut ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.set_feature_enabled(Feature::MisalignmentReview, false);
    handle_turn_started(&mut chat, "original");
    chat.input_queue
        .pending_steers
        .push_back(pending_steer("earlier steer"));
    chat.refresh_pending_input_preview();
    refuse(&mut chat);
    chat.prepare_image_submission(
        "new picture".into(),
        UserMessageHistoryRecord::UserMessageText,
        UserMessageSource::DirectEnter,
    );
    let pending = render_bottom_popup(&chat, 100);
    assert!(
        pending.contains("Preparing images: new picture"),
        "{pending}"
    );
    assert!(!pending.contains("automatic sending paused"));
    assert!(!pending.contains("edit last queued message"));

    chat.input_queue
        .queued_user_messages
        .push_back(UserMessage::from("earlier follow-up").into());
    chat.refresh_pending_input_preview();
    let mixed = render_bottom_popup(&chat, 100);
    let (live, held) = mixed
        .split_once("Follow-up inputs (automatic sending paused)")
        .unwrap_or_else(|| panic!("{mixed}"));
    assert!(live.contains("Preparing images: new picture"));
    assert!(held.contains("earlier follow-up"));
    assert!(held.contains("edit last queued message"));
    assert!(ops.try_recv().is_err());
}
