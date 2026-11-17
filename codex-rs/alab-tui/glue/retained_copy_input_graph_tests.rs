// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native guard/queue/channel controls. No whole-App installation,
//! clipboard OS result, Native/Source grant, model input or UI ACK is fabricated.
use super::*;
use crate::app_event_sender::AppEventSender;
use crate::copy_input_guard::CopyInputGuard;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::unbounded_channel;

const BYTES: usize = 1024 * 1024;
fn closed(receiver: &mut UnboundedReceiver<AppEvent>) -> usize {
    std::iter::from_fn(|| receiver.try_recv().ok())
        .filter(|event| matches!(event, AppEvent::TranscriptCopyClosed))
        .count()
}
fn original() -> (
    guards::Capture,
    guards::SenderRef,
    guards::GuardRef,
    Arc<CopyInputGuard>,
    UnboundedReceiver<AppEvent>,
) {
    let (tx, rx) = unbounded_channel();
    let sender = AppEventSender::new(tx);
    let original = Arc::new(CopyInputGuard(sender.clone()));
    let mut capture = guards::Capture::default();
    let owner = capture.sender(&sender).unwrap();
    let guard = capture.strong(&original).unwrap();
    (capture, owner, guard, original, rx)
}

#[tokio::test]
async fn real_widget_picker_guard_and_weak_queue_bind_to_actual_new_widget_sender() -> io::Result<()>
{
    let (mut source, _source_sender, mut source_events, _source_ops) =
        crate::boxed_future!(crate::chatwidget::tests::make_chatwidget_manual_with_sender()).await;
    source.local_settings.transcript_mode = crate::transcript_mode::TranscriptMode::Owned;
    source.show_copy_picker();
    let original = std::iter::from_fn(|| source_events.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::SelectTranscriptCopy { guard } => Some(guard),
            _ => None,
        })
        .expect("original native copy picker must own its real guard");
    assert!(source.input_queue.submissions_paused());
    let mut capture = guards::Capture::default();
    let sender = source.alab_capture_copy_sender(&mut capture)?;
    let id = capture.strong(&original)?;
    assert_eq!(capture.strong(&original)?, id);
    let weak = source.alab_capture_copy_pause(&capture)?;
    assert_eq!(weak, Some(id));
    let (mut destination, _destination_sender, mut destination_events, _destination_ops) =
        crate::boxed_future!(crate::chatwidget::tests::make_chatwidget_manual_with_sender()).await;
    let mut prepared = guards::Prepared::decode(&capture.encode(BYTES)?, BYTES)?;
    destination.alab_bind_copy_sender(&mut prepared, sender)?;
    let restored = prepared.restore()?;
    // The actual Widget's weak pause and the actual native strong guard root
    // are installed together. This is a local lower seam, not whole-App proof.
    let retained = restored.install_with(|actual| {
        let strong = actual.strong(id)?;
        destination.alab_restore_copy_pause(actual, weak)?;
        assert!(destination.input_queue.submissions_paused());
        assert!(Weak::ptr_eq(
            &destination.input_queue.transcript_copy,
            &Arc::downgrade(&strong)
        ));
        Ok(strong)
    })?;
    assert_eq!(closed(&mut destination_events), 0);
    assert!(source.input_queue.submissions_paused());
    assert_eq!(closed(&mut source_events), 0);
    drop(retained);
    assert!(!destination.input_queue.submissions_paused());
    assert_eq!(closed(&mut destination_events), 1);
    // C1's original native guard is still genuinely rooted throughout C2 DATA
    // construction; no old close was emitted as a side effect of decoding.
    assert!(source.input_queue.submissions_paused());
    Ok(())
}

#[test]
fn abandoned_and_failed_installation_drop_dormant_without_close_events() -> io::Result<()> {
    let (capture, sender, _id, _original, _source_events) = original();
    let (tx, mut events) = unbounded_channel();
    let actual = AppEventSender::new(tx);
    for stage in 0..3 {
        let mut prepared = guards::Prepared::decode(&capture.encode(BYTES)?, BYTES)?;
        prepared.bind(sender, &actual)?;
        let restored = prepared.restore()?;
        match stage {
            0 => drop(restored),
            1 => assert!(
                restored
                    .install_with(|_| Err::<(), _>(io::Error::other(
                        "actual graph installation refused"
                    )))
                    .is_err()
            ),
            _ => assert!(restored.install_with(|_| Ok(())).is_err()), // temporary root alone never counts
        }
        assert_eq!(closed(&mut events), 0);
    }
    // Ordinary native guards keep their original unconditional local close.
    drop(CopyInputGuard(actual));
    assert_eq!(closed(&mut events), 1);
    Ok(())
}

#[test]
fn distinct_equal_guards_share_sender_but_never_merge_root_identity() -> io::Result<()> {
    let (mut capture, sender, first, original, _source_events) = original();
    let separate = Arc::new(CopyInputGuard(original.0.clone()));
    let second = capture.strong(&separate)?;
    assert_ne!(first, second);
    let (tx, mut events) = unbounded_channel();
    let actual = AppEventSender::new(tx);
    let mut prepared = guards::Prepared::decode(&capture.encode(BYTES)?, BYTES)?;
    prepared.bind(sender, &actual)?;
    let (a, b) = prepared.restore()?.install_with(|restored| {
        let a = restored.strong(first)?;
        assert!(Arc::ptr_eq(&a, &restored.strong(first)?));
        let b = restored.strong(second)?;
        assert!(!Arc::ptr_eq(&a, &b));
        Ok((a, b))
    })?;
    assert_eq!(closed(&mut events), 0);
    drop(a);
    assert_eq!(closed(&mut events), 1);
    drop(b);
    assert_eq!(closed(&mut events), 1);
    Ok(())
}

#[test]
fn actual_sender_generations_and_voice_policy_are_required_not_decoded_channels() -> io::Result<()>
{
    let (tx, _source_events) = unbounded_channel();
    let first = AppEventSender::new(tx.clone());
    let second = AppEventSender::new(tx);
    second.voice_only.store(true, Ordering::Release);
    let mut capture = guards::Capture::default();
    let a = capture.sender(&first)?;
    let b = capture.sender(&second)?;
    assert_ne!(a, b);
    let guard_a = Arc::new(CopyInputGuard(first));
    let guard_b = Arc::new(CopyInputGuard(second));
    let a_id = capture.strong(&guard_a)?;
    let b_id = capture.strong(&guard_b)?;
    let (tx, mut events) = unbounded_channel();
    let actual_a = AppEventSender::new(tx.clone());
    let actual_b = AppEventSender::new(tx);
    let mut prepared = guards::Prepared::decode(&capture.encode(BYTES)?, BYTES)?;
    prepared.bind(a, &actual_a)?;
    assert!(prepared.bind(b, &actual_a).is_err());
    assert!(prepared.bind(b, &actual_b).is_err());
    actual_b.voice_only.store(true, Ordering::Release);
    prepared.bind(b, &actual_b)?;
    let (a, b) = prepared
        .restore()?
        .install_with(|restored| Ok((restored.strong(a_id)?, restored.strong(b_id)?)))?;
    drop(b);
    assert_eq!(
        closed(&mut events),
        0,
        "original parked voice sender policy must still suppress the local event"
    );
    drop(a);
    assert_eq!(closed(&mut events), 1);
    Ok(())
}

#[test]
fn missing_live_strong_roots_foreign_references_and_closed_wire_are_refused() -> io::Result<()> {
    let (capture, sender, _id, original, _source_events) = original();
    let empty = guards::Capture::default();
    assert!(empty.weak(&Arc::downgrade(&original)).is_err());
    let dead = Weak::<CopyInputGuard>::new();
    assert_eq!(empty.weak(&dead)?, None);
    let bytes = capture.encode(BYTES)?;
    let mut raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    raw["guards"][0] = 99.into();
    assert!(guards::Prepared::decode(&serde_json::to_vec(&raw).unwrap(), BYTES).is_err());
    raw = serde_json::from_slice(&bytes).unwrap();
    raw["nativeCurrent"] = true.into();
    assert!(guards::Prepared::decode(&serde_json::to_vec(&raw).unwrap(), BYTES).is_err());
    assert!(guards::Prepared::decode(&bytes, BYTES)?.restore().is_err());
    let (tx, mut events) = unbounded_channel();
    let actual = AppEventSender::new(tx);
    let mut prepared = guards::Prepared::decode(&bytes, BYTES)?;
    prepared.bind(sender, &actual)?;
    let restored = prepared.restore()?;
    let invalid: guards::GuardRef = serde_json::from_str("99").unwrap();
    assert!(restored.strong(invalid).is_err());
    assert!(restored.weak(Some(invalid)).is_err());
    drop(restored);
    assert_eq!(closed(&mut events), 0);
    Ok(())
}

#[test]
fn actual_pair_census_is_weak_and_never_keeps_an_app_receiver_open() -> io::Result<()> {
    let (tx, mut receiver, census) = guards::channel();
    let actual = AppEventSender::new(tx);
    let capture = guards::Capture::for_app(&census, &actual)?;
    drop(capture);
    drop(actual);
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
    ));
    let (other, _other_receiver, _other_census) = guards::channel();
    assert!(guards::Capture::for_app(&census, &AppEventSender::new(other)).is_err());
    Ok(())
}

#[test]
fn real_retired_sender_aliases_bind_only_to_actual_new_app_census() -> io::Result<()> {
    let (source, mut source_events, source_census) = guards::channel();
    let source_app = AppEventSender::new(source);
    // This actual native sender survives in guards after its Widget owner is
    // gone. Its original channel is still the real source App receiver pair.
    let retired = AppEventSender::new(source_app.app_event_tx.clone());
    let original = Arc::new(CopyInputGuard(retired.clone()));
    let separate = Arc::new(CopyInputGuard(retired));
    let mut capture = guards::Capture::for_app(&source_census, &source_app)?;
    let first = capture.strong(&original)?;
    let second = capture.strong(&separate)?;
    let (target, mut target_events, target_census) = guards::channel();
    let target_app = AppEventSender::new(target);
    let mut prepared = guards::Prepared::decode(&capture.encode(BYTES)?, BYTES)?;
    let references = prepared.app_sender_refs()?;
    assert_eq!(references.len(), 1);
    assert!(prepared.bind(references[0], &target_app).is_err());
    assert!(
        prepared
            .bind_retired(references[0], &source_census, &target_app)
            .is_err()
    );
    prepared.bind_retired(references[0], &target_census, &target_app)?;
    prepared.bind_retired(references[0], &target_census, &target_app)?;
    let (a, b) = prepared
        .restore()?
        .install_with(|restored| Ok((restored.strong(first)?, restored.strong(second)?)))?;
    assert!(Arc::ptr_eq(&a.0.voice_only, &b.0.voice_only));
    assert!(!Arc::ptr_eq(&a.0.voice_only, &target_app.voice_only));
    assert!(a.0.app_event_tx.same_channel(&target_app.app_event_tx));
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(closed(&mut target_events), 0);
    assert_eq!(closed(&mut source_events), 0);
    drop(a);
    assert_eq!(closed(&mut target_events), 1);
    drop(b);
    assert_eq!(closed(&mut target_events), 1);
    // Original C1 allocations remain pinned; decoding never releases them.
    assert_eq!(closed(&mut source_events), 0);
    Ok(())
}

#[test]
fn genuinely_closed_sender_never_becomes_active_and_survives_a_later_capture() -> io::Result<()> {
    let (abandoned, abandoned_receiver) = unbounded_channel();
    let closed_sender = AppEventSender::new(abandoned);
    let original = Arc::new(CopyInputGuard(closed_sender));
    drop(abandoned_receiver);
    assert!(original.0.app_event_tx.is_closed());
    let (source, _source_events, source_census) = guards::channel();
    let source_app = AppEventSender::new(source);
    let mut capture = guards::Capture::for_app(&source_census, &source_app)?;
    let id = capture.strong(&original)?;
    let bytes = capture.encode(BYTES)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(raw["senders"][0]["kind"], "closedLocal");
    let (target, mut target_events, target_census) = guards::channel();
    let target_app = AppEventSender::new(target);
    let mut prepared = guards::Prepared::decode(&bytes, BYTES)?;
    for sender in prepared.app_sender_refs()? {
        prepared.bind_retired(sender, &target_census, &target_app)?;
    }
    let closed_guard = prepared
        .restore()?
        .install_with(|restored| restored.strong(id))?;
    // Closed is an installed native state, distinct from an abandoned restore.
    // It can therefore cross a later C2->C3 without arming its local close event.
    let mut next_capture = guards::Capture::for_app(&target_census, &target_app)?;
    let next_id = next_capture.strong(&closed_guard)?;
    let bytes = next_capture.encode(BYTES)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(raw["senders"][0]["kind"], "closedLocal");
    let (next, mut next_events, next_census) = guards::channel();
    let next_app = AppEventSender::new(next);
    let mut prepared = guards::Prepared::decode(&bytes, BYTES)?;
    for sender in prepared.app_sender_refs()? {
        prepared.bind_retired(sender, &next_census, &next_app)?;
    }
    let final_guard = prepared
        .restore()?
        .install_with(|restored| restored.strong(next_id))?;
    drop(closed_guard);
    drop(next_capture);
    drop(final_guard);
    assert_eq!(closed(&mut target_events), 0);
    assert_eq!(closed(&mut next_events), 0);
    Ok(())
}

#[test]
fn live_unmatched_channel_or_real_receiver_close_cannot_mint_a_captured_sender() -> io::Result<()> {
    let (source, mut source_events, source_census) = guards::channel();
    let source_app = AppEventSender::new(source);
    let (other, _other_events, _other_census) = guards::channel();
    let foreign = Arc::new(CopyInputGuard(AppEventSender::new(other)));
    let mut capture = guards::Capture::for_app(&source_census, &source_app)?;
    assert!(capture.strong(&foreign).is_err());
    let valid = Arc::new(CopyInputGuard(AppEventSender::new(
        source_app.app_event_tx.clone(),
    )));
    capture.strong(&valid)?;
    source_events.close();
    assert!(capture.encode(BYTES).is_err());
    assert!(guards::Capture::for_app(&source_census, &source_app).is_err());
    Ok(())
}

#[test]
fn local_sender_kind_is_a_required_exact_string_not_a_selector_or_map() -> io::Result<()> {
    let (source, _source_events, source_census) = guards::channel();
    let source_app = AppEventSender::new(source);
    let mut capture = guards::Capture::for_app(&source_census, &source_app)?;
    let actual = Arc::new(CopyInputGuard(AppEventSender::new(source_app.app_event_tx)));
    capture.strong(&actual)?;
    let bytes = capture.encode(BYTES)?;
    for value in [
        serde_json::json!({"retiredLocal":null}),
        serde_json::json!("other"),
        serde_json::Value::Null,
    ] {
        let mut wrong: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        wrong["senders"][0]["kind"] = value;
        assert!(guards::Prepared::decode(&serde_json::to_vec(&wrong).unwrap(), BYTES).is_err());
    }
    let mut wrong: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    wrong["senders"][0].as_object_mut().unwrap().remove("kind");
    assert!(guards::Prepared::decode(&serde_json::to_vec(&wrong).unwrap(), BYTES).is_err());
    Ok(())
}

#[test]
fn readonly_pane_sender_preserves_actual_widget_but_refuses_retired_and_closed_guard_roles()
-> io::Result<()> {
    let (source, _source_events, source_census) = guards::channel();
    let source_app = AppEventSender::new(source);
    let retired = Arc::new(CopyInputGuard(AppEventSender::new(
        source_app.app_event_tx.clone(),
    )));
    let (closed_tx, closed_receiver) = unbounded_channel();
    let closed_guard = Arc::new(CopyInputGuard(AppEventSender::new(closed_tx)));
    drop(closed_receiver);
    let mut capture = guards::Capture::for_app(&source_census, &source_app)?;
    let widget = capture.sender(&source_app)?;
    capture.strong(&retired)?;
    capture.strong(&closed_guard)?;
    let (target, mut target_events, target_census) = guards::channel();
    let target_app = AppEventSender::new(target);
    let mut prepared = guards::Prepared::decode(&capture.encode(BYTES)?, BYTES)?;
    prepared.bind(widget, &target_app)?;
    let guard_only = prepared.app_sender_refs()?;
    assert_eq!(guard_only.len(), 2);
    for reference in &guard_only {
        prepared.bind_retired(*reference, &target_census, &target_app)?;
    }
    let actual = prepared.restore()?;
    let sender = actual.sender(widget)?;
    assert!(sender.app_event_tx.same_channel(&target_app.app_event_tx));
    assert!(Arc::ptr_eq(&sender.voice_only, &target_app.voice_only));
    for reference in guard_only {
        assert!(actual.sender(reference).is_err());
    }
    drop(actual);
    assert!(target_events.try_recv().is_err());
    Ok(())
}
