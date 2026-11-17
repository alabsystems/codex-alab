// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native widget/data controls. No Source callback owner, Native
//! controller, capture, App grant or completed-view permission is fabricated.
use super::*;
use crate::app_command::AppCommand;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::unbounded_channel;
const LIMIT: usize = 1_048_576;
fn exec(thread: ThreadId, id: &str) -> ApprovalRequest {
    ApprovalRequest::Exec(ExecApprovalRequest {
        kind: CommandExecutionApprovalKind::Command,
        thread_id: thread,
        thread_label: None,
        id: id.into(),
        environment_id: None,
        command: vec!["echo".into(), "original".into()],
        reason: Some("original reason".into()),
        available_decisions: vec![
            CommandExecutionApprovalDecision::Accept,
            CommandExecutionApprovalDecision::Cancel,
        ],
        network_approval_context: None,
        additional_permissions: None,
    })
}
fn overlay(request: ApprovalRequest) -> (ApprovalOverlay, UnboundedReceiver<AppEvent>) {
    let (sender, receiver) = unbounded_channel();
    let keymap = crate::keymap::RuntimeKeymap::defaults();
    (
        ApprovalOverlay::new(
            request,
            AppEventSender::new(sender),
            Features::with_defaults(),
            keymap.approval,
            keymap.list,
        ),
        receiver,
    )
}
fn key(view: &mut ApprovalOverlay, code: KeyCode) {
    view.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}
#[test]
fn original_approval_selection_round_trips_and_uses_only_actual_destination_sender()
-> io::Result<()> {
    let request = exec(ThreadId::new(), "original");
    let (mut old, mut old_events) = overlay(request.clone());
    key(&mut old, KeyCode::Down);
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    let saved = PreparedApproval::decode(&raw, LIMIT)?;
    let (mut actual, mut events) = overlay(request);
    saved.validate_destination(&actual)?;
    // Pure list DATA only. The actual bound whole-view installer additionally
    // requires typed original Source owners; this test never fabricates them.
    saved.install_native_editing(&mut actual)?;
    key(&mut actual, KeyCode::Enter);
    let mut choices = 0;
    while let Ok(event) = events.try_recv() {
        if let AppEvent::SubmitThreadOp {
            op: AppCommand::ExecApproval { decision, .. },
            ..
        } = event
        {
            assert!(matches!(decision, CommandExecutionApprovalDecision::Cancel));
            choices += 1;
        }
    }
    assert_eq!(choices, 1);
    assert!(old_events.try_recv().is_err());
    Ok(())
}
#[test]
fn real_destination_input_is_not_overwritten_even_when_its_selection_returns_to_start()
-> io::Result<()> {
    let request = exec(ThreadId::new(), "original");
    let (old, _) = overlay(request.clone());
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    let saved = PreparedApproval::decode(&raw, LIMIT)?;
    let (mut actual, _) = overlay(request);
    key(&mut actual, KeyCode::Down);
    key(&mut actual, KeyCode::Up);
    assert_eq!(
        actual.list.selected_actual_idx(),
        old.list.selected_actual_idx()
    );
    saved.validate_destination(&actual)?;
    let actual_generation = actual.alab_editing.original()?;
    saved.install_native_editing(&mut actual)?;
    assert_eq!(actual.alab_editing.original()?, actual_generation);
    Ok(())
}
#[test]
fn changed_actual_request_and_changed_option_meanings_refuse_before_mutation() -> io::Result<()> {
    let request = exec(ThreadId::new(), "original");
    let (old, _) = overlay(request.clone());
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    let saved = PreparedApproval::decode(&raw, LIMIT)?;
    let mut other = request.clone();
    if let ApprovalRequest::Exec(r) = &mut other {
        r.command.push("changed".into());
    }
    let (actual, _) = overlay(other);
    assert!(saved.validate_destination(&actual).is_err());
    let (mut actual, _) = overlay(request);
    actual.options.reverse();
    assert!(saved.validate_destination(&actual).is_err());
    assert!(!actual.done);
    assert!(!actual.current_complete);
    Ok(())
}
#[test]
fn physical_lifo_queue_pruning_preserves_current_and_new_actual_appends() -> io::Result<()> {
    let thread = ThreadId::new();
    let first = exec(thread, "first");
    let second = exec(thread, "second");
    let third = exec(thread, "third");
    let (mut old, _) = overlay(first.clone());
    old.enqueue_request(second.clone());
    old.enqueue_request(third.clone());
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    let kept =
        editing_retaining(&raw, &[true, false, true], LIMIT)?.expect("original current remains");
    let (mut actual, _) = overlay(first);
    actual.enqueue_request(third);
    actual.enqueue_request(exec(thread, "new"));
    PreparedApproval::decode(&kept, LIMIT)?.validate_destination(&actual)?;
    assert_eq!(actual.queue.len(), 2);
    assert!(!RequestData::capture(&second).matches_native(&actual.queue[0])?);
    let next = editing_retaining(&raw, &[false, true, true], LIMIT)?
        .expect("actual next original LIFO row remains");
    let promoted = requests_data(&next, LIMIT)?;
    assert!(promoted[0].matches_native(&actual.queue[0])?);
    Ok(())
}
#[test]
fn patch_payload_keeps_non_utf8_paths_and_whole_original_diff() -> io::Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let cwd = PathBuf::from(std::ffi::OsString::from_vec(b"/original-\xff".to_vec()));
    let changed = PathBuf::from(std::ffi::OsString::from_vec(b"file-\xfe".to_vec()));
    let moved = PathBuf::from(std::ffi::OsString::from_vec(b"target-\xfd".to_vec()));
    let request = ApprovalRequest::ApplyPatch(ApplyPatchApprovalRequest {
        thread_id: ThreadId::new(),
        thread_label: None,
        id: "patch".into(),
        reason: Some("whole".into()),
        cwd: AbsolutePathBuf::try_from(cwd).map_err(invalid)?,
        changes: HashMap::from([(
            changed,
            FileChange::Update {
                unified_diff: "--- old\n+++ new\n-original\n+actual\n".into(),
                move_path: Some(moved),
            },
        )]),
    });
    let (old, _) = overlay(request.clone());
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    let (actual, _) = overlay(request);
    PreparedApproval::decode(&raw, LIMIT)?.validate_destination(&actual)?;
    Ok(())
}
#[test]
fn nested_foreign_fields_missing_fields_and_unbounded_choices_do_not_decode() -> io::Result<()> {
    let (old, _) = overlay(exec(ThreadId::new(), "original"));
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    let mut wire: serde_json::Value = serde_json::from_str(raw.get()).map_err(invalid)?;
    wire["request"]["foreign"] = serde_json::json!(true);
    let raw = serde_json::value::to_raw_value(&wire).map_err(invalid)?;
    assert!(PreparedApproval::decode(&raw, LIMIT).is_err());
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    assert!(PreparedApproval::decode(&raw, 1).is_err());
    let mut wire: serde_json::Value = serde_json::from_str(raw.get()).map_err(invalid)?;
    wire["request"]
        .as_object_mut()
        .expect("request")
        .remove("reason");
    let raw = serde_json::value::to_raw_value(&wire).map_err(invalid)?;
    assert!(PreparedApproval::decode(&raw, LIMIT).is_err());
    Ok(())
}
#[test]
fn completed_native_decision_cannot_install_without_actual_original_callback_owner()
-> io::Result<()> {
    let request = exec(ThreadId::new(), "original");
    let (mut old, mut original_events) = overlay(request.clone());
    key(&mut old, KeyCode::Enter);
    assert!(old.done);
    assert!(original_events.try_recv().is_ok());
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    let (mut actual, mut events) = overlay(request);
    assert!(
        PreparedApproval::decode(&raw, LIMIT)?
            .install_bound(&mut actual, &[])
            .is_err()
    );
    assert!(!actual.done);
    assert!(!actual.current_complete);
    assert!(events.try_recv().is_err());
    Ok(())
}
#[test]
fn newer_thread_label_is_preserved_while_original_request_semantics_stay_bound() -> io::Result<()> {
    let request = exec(ThreadId::new(), "original");
    let (old, _) = overlay(request.clone());
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    let mut newer = request;
    if let ApprovalRequest::Exec(r) = &mut newer {
        r.thread_label = Some("new actual label".into());
    }
    let (actual, _) = overlay(newer);
    PreparedApproval::decode(&raw, LIMIT)?.validate_destination(&actual)?;
    assert_eq!(
        actual
            .current_request
            .as_ref()
            .and_then(ApprovalRequest::thread_label),
        Some("new actual label")
    );
    Ok(())
}

#[test]
fn completed_original_view_advances_new_actual_requests_without_answering_or_discarding_them()
-> io::Result<()> {
    let thread = ThreadId::new();
    let original = exec(thread, "original");
    let (mut old, _) = overlay(original.clone());
    key(&mut old, KeyCode::Enter);
    let raw = old.alab_capture_approval_editing(LIMIT)?;
    let later = exec(thread, "later");
    let (mut actual, mut events) = overlay(original);
    actual.enqueue_request(later.clone());
    let prepared = PreparedApproval::decode(&raw, LIMIT)?;
    prepared.validate_destination(&actual)?;
    // Native-data mutation only; no original Source owner or completion proof
    // is made by this lower widget control. install_bound is the real route.
    prepared.install_native_editing(&mut actual)?;
    assert!(!actual.done);
    assert!(!actual.current_complete);
    assert!(
        RequestData::capture(&later).matches_native(
            actual
                .current_request
                .as_ref()
                .expect("new actual request remains")
        )?
    );
    assert!(events.try_recv().is_err());
    key(&mut actual, KeyCode::Enter);
    let mut found = false;
    while let Ok(event) = events.try_recv() {
        if let AppEvent::SubmitThreadOp {
            op: AppCommand::ExecApproval { id, .. },
            ..
        } = event
        {
            assert_eq!(id, "later");
            found = true;
        }
    }
    assert!(found);
    Ok(())
}
