// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::super::user_messages::MessageDelivery;
use super::super::user_messages::QueuedUserMessage;
use super::super::user_messages::UserMessageHistoryRecord;
use super::*;
use crate::bottom_pane::QueuedInputAction;

#[tokio::test]
async fn real_widget_initial_submission_and_unconfirmed_queue_survive_without_dispatch() {
    let (mut source, mut source_events, mut source_operations) = super::test_widget().await;
    source.initial_user_message = Some(UserMessage::from("original CLI prompt"));
    source.suppress_initial_user_message_submit = true;
    source.apply_external_edit("confirmed startup draft".into());
    source.input_queue.startup_submission = Some(source.bottom_pane.composer_recovery_snapshot());
    source
        .input_queue
        .queued_user_messages
        .push_back(QueuedUserMessage {
            user_message: UserMessage::from("already accepted input"),
            prepared_images: None,
            action: QueuedInputAction::Literal,
            delivery: MessageDelivery::Unconfirmed(Some("actual-original-client-id".into())),
            pending_pastes: vec![("paste-slot".into(), "original full paste".into())],
            source: UserMessageSource::DirectEnter,
        });
    source
        .input_queue
        .queued_user_message_history_records
        .push_back(UserMessageHistoryRecord::UserMessageText);
    source.input_queue.suppress_queue_autosend = true;
    source.input_queue.user_turn_pending_start = true;
    source.input_queue.pending_user_message_client_id = Some("actual-original-client-id".into());
    while source_events.try_recv().is_ok() {}
    while source_operations.try_recv().is_ok() {}

    let bytes = source.alab_capture_retained_messages().unwrap();
    assert_eq!(
        source.alab_capture_retained_messages().unwrap().get(),
        bytes.get()
    );
    assert!(source_events.try_recv().is_err());
    assert!(source_operations.try_recv().is_err());

    let (mut destination, mut events, mut operations) = super::test_widget().await;
    // Original CLI parsing may have created the SAME pending message. It must
    // remain pending rather than be cleared by legacy runtime editor restore.
    destination.initial_user_message = Some(UserMessage::from("original CLI prompt"));
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}
    PreparedMessages::decode(&bytes)
        .unwrap()
        .install(&mut destination)
        .unwrap();
    assert_eq!(
        destination.alab_capture_retained_messages().unwrap().get(),
        bytes.get()
    );
    assert!(destination.initial_user_message.is_some());
    assert!(destination.suppress_initial_user_message_submit);
    assert!(destination.input_queue.startup_submission.is_some());
    assert!(
        matches!(destination.input_queue.queued_user_messages[0].delivery,
        MessageDelivery::Unconfirmed(Some(ref id)) if id == "actual-original-client-id")
    );
    assert!(events.try_recv().is_err());
    assert!(operations.try_recv().is_err());
}

#[tokio::test]
async fn different_original_cli_message_is_kept_on_refused_install() {
    let (mut source, _events, _operations) = super::test_widget().await;
    source.initial_user_message = Some(UserMessage::from("saved original"));
    let bytes = source.alab_capture_retained_messages().unwrap();
    let (mut destination, _events, _operations) = super::test_widget().await;
    destination.initial_user_message = Some(UserMessage::from("different pending original"));
    let before = destination.alab_capture_retained_messages().unwrap();
    assert!(
        PreparedMessages::decode(&bytes)
            .unwrap()
            .install(&mut destination)
            .is_err()
    );
    assert_eq!(
        destination.alab_capture_retained_messages().unwrap().get(),
        before.get()
    );
}

#[tokio::test]
async fn consumed_cli_message_does_not_reappear_from_preserved_argv() {
    let original_prompt = Some("same original CLI prompt".to_string());
    let from_argv = super::super::user_messages::create_initial_user_message(
        original_prompt,
        Vec::new(),
        Vec::new(),
    );
    assert!(
        from_argv.is_some(),
        "the preserved argv would ordinarily create a prompt"
    );

    let (mut source, _events, _operations) = super::test_widget().await;
    source.initial_user_message = None; // Actual old widget already consumed it.
    source.apply_external_edit("separately confirmed startup draft".into());
    source.input_queue.startup_submission = Some(source.bottom_pane.composer_recovery_snapshot());
    source
        .input_queue
        .queued_user_messages
        .push_back(QueuedUserMessage {
            user_message: UserMessage::from("accepted before image transition"),
            prepared_images: None,
            action: QueuedInputAction::Plain,
            delivery: MessageDelivery::Unconfirmed(Some("original-in-flight-id".into())),
            pending_pastes: Vec::new(),
            source: UserMessageSource::Prompt,
        });
    let bytes = source.alab_capture_retained_messages().unwrap();
    let prepared = PreparedMessages::decode(&bytes).unwrap();
    // This is the value the C2 original ChatWidgetInit must receive, before any
    // fresh initial-message enqueue. It does not reuse the derived from_argv.
    let constructor_message = prepared.initial_message_for_constructor();
    assert!(constructor_message.is_none());
    let (mut destination, mut events, mut operations) = super::test_widget().await;
    destination.initial_user_message = constructor_message;
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}
    prepared.install(&mut destination).unwrap();
    assert!(destination.initial_user_message.is_none());
    assert!(destination.input_queue.startup_submission.is_some());
    assert!(
        matches!(destination.input_queue.queued_user_messages[0].delivery,
        MessageDelivery::Unconfirmed(Some(ref id)) if id == "original-in-flight-id")
    );
    assert_eq!(
        destination.alab_capture_retained_messages().unwrap().get(),
        bytes.get()
    );
    assert!(events.try_recv().is_err());
    assert!(operations.try_recv().is_err());
}

#[tokio::test]
async fn closed_native_message_codec_rejects_new_delivery_and_foreign_fields() {
    let (mut source, _events, _operations) = super::test_widget().await;
    source
        .input_queue
        .queued_user_messages
        .push_back(QueuedUserMessage::new(
            UserMessage::from("pending"),
            QueuedInputAction::Plain,
        ));
    let bytes = source.alab_capture_retained_messages().unwrap();
    let mut value: serde_json::Value = serde_json::from_str(bytes.get()).unwrap();
    value["input_queue"]["queued_user_messages"][0]["delivery"] =
        serde_json::json!({"Resend":true});
    let raw = serde_json::value::to_raw_value(&value).unwrap();
    assert!(PreparedMessages::decode(&raw).is_err());
    let mut value: serde_json::Value = serde_json::from_str(bytes.get()).unwrap();
    value["initial_message_was_sent"] = serde_json::json!(true);
    let raw = serde_json::value::to_raw_value(&value).unwrap();
    assert!(PreparedMessages::decode(&raw).is_err());
}
