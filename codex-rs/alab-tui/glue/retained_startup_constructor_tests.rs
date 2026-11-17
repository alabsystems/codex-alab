// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower real-widget/DATA controls. No actual entered resource, Native Go,
//! configured completion, or Source callback admission is fabricated.
use super::super::user_messages::MessageDelivery;
use super::super::user_messages::QueuedUserMessage;
use super::super::user_messages::UserMessageHistoryRecord;
use super::*;
use crate::bottom_pane::QueuedInputAction;
use crate::owned_local_entered_readers::constructor::Constructor;
use crate::owned_local_entered_readers::constructor::InitialMessageData;
use crate::owned_local_entered_readers::constructor::{self};
use crate::startup_draft::alab_configured::hold_original_initial_message;
use std::cell::Cell;

#[tokio::test]
async fn consumed_cli_prompt_is_not_regenerated_before_actual_configured_hold() {
    let original_prompt = Some("same preserved original CLI prompt".to_string());
    assert!(
        super::super::create_initial_user_message(original_prompt.clone(), Vec::new(), Vec::new())
            .is_some()
    );
    let (mut source, _source_events, _source_ops) = super::test_widget().await;
    source.initial_user_message = None;
    source.apply_external_edit("original confirmed startup draft".into());
    source.input_queue.startup_submission = Some(source.bottom_pane.composer_recovery_snapshot());
    source
        .input_queue
        .queued_user_messages
        .push_back(QueuedUserMessage {
            user_message: UserMessage::from("already accepted original response"),
            prepared_images: None,
            action: QueuedInputAction::Plain,
            delivery: MessageDelivery::Unconfirmed(Some("original-client-id".into())),
            pending_pastes: Vec::new(),
            source: UserMessageSource::Prompt,
        });
    source
        .input_queue
        .queued_user_message_history_records
        .push_back(UserMessageHistoryRecord::UserMessageText);
    let raw = source.alab_capture_retained_messages().unwrap();
    let prepared = PreparedMessages::decode(&raw).unwrap();
    let (mut destination, mut events, mut operations) = super::test_widget().await;
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}
    destination.initial_user_message = constructor::selected_message_for_tests(&prepared, || {
        panic!("must not regenerate captured consumed prompt")
    });
    let mut actual_configured_slot = None;
    hold_original_initial_message(&mut actual_configured_slot, &mut destination).unwrap();
    assert_eq!(actual_configured_slot, Some(None));
    assert!(destination.initial_user_message.is_none());
    // Borrowing for constructor did not consume/rewrite the non-clone graph.
    prepared.install(&mut destination).unwrap();
    assert_eq!(
        destination.alab_capture_retained_messages().unwrap().get(),
        raw.get()
    );
    assert!(destination.input_queue.startup_submission.is_some());
    assert!(
        matches!(destination.input_queue.queued_user_messages[0].delivery,
        MessageDelivery::Unconfirmed(Some(ref id)) if id == "original-client-id")
    );
    assert!(events.try_recv().is_err());
    assert!(operations.try_recv().is_err());
}

#[tokio::test]
async fn pending_original_cli_prompt_moves_once_into_the_real_shared_hold_function() {
    let (mut source, _events, _ops) = super::test_widget().await;
    let pending = UserMessage::from("original pending CLI prompt");
    source.initial_user_message = Some(pending.clone());
    let prepared =
        PreparedMessages::decode(&source.alab_capture_retained_messages().unwrap()).unwrap();
    let (mut destination, mut events, mut operations) = super::test_widget().await;
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}
    destination.initial_user_message = constructor::selected_message_for_tests(&prepared, || {
        panic!("captured pending prompt must remain authoritative")
    });
    let mut slot = None;
    hold_original_initial_message(&mut slot, &mut destination).unwrap();
    assert_eq!(slot, Some(Some(pending.clone())));
    assert!(destination.initial_user_message.is_none());
    assert!(hold_original_initial_message(&mut slot, &mut destination).is_err());
    assert_eq!(slot, Some(Some(pending.clone())));
    // The separately owned full graph remains available; reading did not mean
    // applied or authorize a second install over this actual held placement.
    assert_eq!(prepared.initial_message_for_constructor(), Some(pending));
    assert!(events.try_recv().is_err());
    assert!(operations.try_recv().is_err());
}

#[tokio::test]
async fn no_primary_initial_message_leaf_preserves_actual_empty_and_pending_state() {
    let (mut actual, _events, _ops) = super::test_widget().await;
    actual.initial_user_message = None;
    let empty = InitialMessageData::from_actual(&actual.initial_user_message)
        .section()
        .unwrap();
    assert!(
        constructor::no_primary_message_for_tests(empty.body(), || panic!(
            "Empty never consults argv"
        ))
        .unwrap()
        .is_none()
    );
    actual.initial_user_message = Some(UserMessage::from("actual NoPrimary held prompt"));
    let pending = InitialMessageData::from_actual(&actual.initial_user_message)
        .section()
        .unwrap();
    assert_eq!(
        constructor::no_primary_message_for_tests(pending.body(), || panic!(
            "Pending never consults argv"
        ))
        .unwrap(),
        actual.initial_user_message
    );
    let mut slot = None;
    hold_original_initial_message(&mut slot, &mut actual).unwrap();
    let from_configured = InitialMessageData::from_actual(slot.as_ref().unwrap())
        .section()
        .unwrap();
    assert_eq!(
        constructor::no_primary_message_for_tests(from_configured.body(), || panic!(
            "configured holder is authoritative"
        ))
        .unwrap(),
        slot.unwrap()
    );
}

#[test]
fn missing_or_malformed_no_primary_submission_cannot_fall_back_to_cli() {
    let calls = Cell::new(0);
    for text in [
        r#"{"schema":1}"#,
        r#"{"schema":1,"submission":null}"#,
        r#"{"schema":1,"submission":{"state":"empty","message":null}}"#,
        r#"{"schema":2,"submission":{"state":"empty"}}"#,
    ] {
        let raw = serde_json::value::RawValue::from_string(text.into()).unwrap();
        assert!(
            constructor::no_primary_message_for_tests(&raw, || {
                calls.set(calls.get() + 1);
                Some(UserMessage::from("must not regenerate"))
            })
            .is_err()
        );
    }
    assert_eq!(calls.get(), 0);
}

#[test]
fn original_cli_builder_and_actual_original_cancel_policy_remain_distinct() {
    let calls = Cell::new(0);
    let expected = super::super::create_initial_user_message(
        Some("original CLI".into()),
        vec![std::path::PathBuf::from("original image.png")],
        Vec::new(),
    );
    let message = constructor::original_message_for_tests(|| {
        calls.set(calls.get() + 1);
        expected.clone()
    });
    assert_eq!(message, expected);
    assert_eq!(calls.get(), 1);
    let owner = Constructor::direct_original(|| expected.clone()).unwrap();
    owner.cancel_from_original_policy().unwrap();
    assert!(
        owner
            .initial_message_for_constructor(|| panic!(
                "original actual cancellation cannot regenerate its prompt"
            ))
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn missing_selected_initial_message_field_cannot_impersonate_consumed_none() {
    let (actual, _events, _ops) = super::test_widget().await;
    let raw = actual.alab_capture_retained_messages().unwrap();
    let mut data: serde_json::Value = serde_json::from_str(raw.get()).unwrap();
    assert!(
        data.as_object_mut()
            .unwrap()
            .remove("initial_user_message")
            .is_some()
    );
    let missing = serde_json::value::to_raw_value(&data).unwrap();
    assert!(PreparedMessages::decode(&missing).is_err());
    assert!(PreparedMessages::decode(&raw).is_ok());
}

#[tokio::test]
async fn no_primary_widget_graph_preserves_confirmation_and_unconfirmed_queue_with_consumed_cli() {
    let (mut source, _events, _ops) = super::test_widget().await;
    source.initial_user_message = None;
    source.apply_external_edit("NoPrimary confirmed draft".into());
    source.input_queue.startup_submission = Some(source.bottom_pane.composer_recovery_snapshot());
    source
        .input_queue
        .queued_user_messages
        .push_back(QueuedUserMessage {
            user_message: UserMessage::from("same unconfirmed original"),
            prepared_images: None,
            action: QueuedInputAction::Plain,
            delivery: MessageDelivery::Unconfirmed(Some("same-original-client".into())),
            pending_pastes: Vec::new(),
            source: UserMessageSource::Prompt,
        });
    let graph = InitialMessageData::from_widget(&source, &None).unwrap();
    let raw = graph.section().unwrap();
    let (mut destination, mut events, mut ops) = super::test_widget().await;
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    destination.initial_user_message =
        constructor::no_primary_message_for_tests(raw.body(), || {
            panic!("consumed CLI must not regenerate")
        })
        .unwrap();
    let data: InitialMessageData = serde_json::from_str(raw.body().get()).unwrap();
    // Production decoder validates the same closed messages before install.
    let prepared = PreparedMessages::decode(data.widget_messages_for_tests().unwrap()).unwrap();
    prepared.install(&mut destination).unwrap();
    assert_eq!(
        destination.alab_capture_retained_messages().unwrap().get(),
        source.alab_capture_retained_messages().unwrap().get()
    );
    assert!(
        matches!(destination.input_queue.queued_user_messages[0].delivery,MessageDelivery::Unconfirmed(Some(ref id)) if id=="same-original-client")
    );
    assert!(ops.try_recv().is_err());
}

#[tokio::test]
async fn no_primary_graph_requires_explicit_widget_presence_and_matching_original_initial_message()
{
    let (mut source, _events, _ops) = super::test_widget().await;
    source.initial_user_message = Some(UserMessage::from("original pending CLI value"));
    let graph = InitialMessageData::from_widget(&source, &source.initial_user_message).unwrap();
    let section = graph.section().unwrap();
    let mut missing: serde_json::Value = serde_json::from_str(section.body().get()).unwrap();
    missing.as_object_mut().unwrap().remove("widgetMessages");
    let raw = serde_json::value::to_raw_value(&missing).unwrap();
    assert!(constructor::no_primary_message_for_tests(&raw, || None).is_err());
    let mut split: serde_json::Value = serde_json::from_str(section.body().get()).unwrap();
    split["submission"] = serde_json::json!({"state":"empty"});
    let raw = serde_json::value::to_raw_value(&split).unwrap();
    assert!(constructor::no_primary_message_for_tests(&raw, || None).is_err());
}

#[tokio::test]
async fn actual_post_filter_none_cannot_be_overwritten_by_saved_cli_some() {
    let (mut source, _events, _ops) = super::test_widget().await;
    source.initial_user_message = Some(UserMessage::from("captured original CLI"));
    let prepared =
        PreparedMessages::decode(&source.alab_capture_retained_messages().unwrap()).unwrap();
    let (mut destination, _events, _ops) = super::test_widget().await;
    destination.initial_user_message = None; // Actual original cancelled Resume/Fork filter.
    prepared
        .install_after_original_policy(&mut destination, false)
        .unwrap();
    assert!(destination.initial_user_message.is_none());
}
#[tokio::test]
async fn original_confirmation_cancellation_keeps_same_unconfirmed_messages() {
    let (mut source, _events, _ops) = super::test_widget().await;
    source.apply_external_edit("original confirmed draft".into());
    source.input_queue.startup_submission = Some(source.bottom_pane.composer_recovery_snapshot());
    source
        .input_queue
        .queued_user_messages
        .push_back(QueuedUserMessage {
            user_message: UserMessage::from("same unconfirmed value"),
            prepared_images: None,
            action: QueuedInputAction::Plain,
            delivery: MessageDelivery::Unconfirmed(Some("original-operation".into())),
            pending_pastes: Vec::new(),
            source: UserMessageSource::Prompt,
        });
    let prepared =
        PreparedMessages::decode(&source.alab_capture_retained_messages().unwrap()).unwrap();
    let (mut destination, _events, mut ops) = super::test_widget().await;
    while ops.try_recv().is_ok() {}
    prepared
        .install_after_original_policy(&mut destination, true)
        .unwrap();
    assert!(destination.input_queue.startup_submission.is_none());
    assert!(
        matches!(destination.input_queue.queued_user_messages[0].delivery,MessageDelivery::Unconfirmed(Some(ref id)) if id=="original-operation")
    );
    assert!(ops.try_recv().is_err());
}

#[tokio::test]
async fn original_message_graph_requires_optional_queue_and_prompt_fields_to_be_explicit() {
    let (mut source, _events, _ops) = super::test_widget().await;
    source
        .input_queue
        .queued_user_messages
        .push_back(QueuedUserMessage {
            user_message: UserMessage::from("original unconfirmed value"),
            prepared_images: None,
            action: QueuedInputAction::Plain,
            delivery: MessageDelivery::Unconfirmed(Some("same-original-client".into())),
            pending_pastes: Vec::new(),
            source: UserMessageSource::Prompt,
        });
    let raw = source.alab_capture_retained_messages().unwrap();
    for path in [
        "/safety_buffering_prompt",
        "/input_queue/startup_submission",
        "/input_queue/pending_user_message_client_id",
        "/input_queue/queued_user_messages/0/prepared_images",
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(raw.get()).unwrap();
        let (parent, key) = path.rsplit_once('/').unwrap();
        wire.pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        let incomplete = serde_json::value::to_raw_value(&wire).unwrap();
        assert!(
            PreparedMessages::decode(&incomplete).is_err(),
            "omitted {path}"
        );
    }
    assert!(PreparedMessages::decode(&raw).is_ok());
}
