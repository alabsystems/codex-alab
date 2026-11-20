// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native editing/resource controls only. The private DATA-only Items have no
//! real reply, Source capture/receipt, current Native origin or grant. Production
//! install_bound still requires actual same-client callback owners and cuts.
use super::*;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
const LIMIT: usize = 1_048_576;
fn item(id: i64, label: &str) -> io::Result<Arc<Item>> {
    crate::app::retained_original_input::data_only_approval_item(id, label)
}
fn request(item: &Arc<Item>, label: &str) -> ApprovalRequest {
    ApprovalRequest::Exec(ExecApprovalRequest {
        kind: Default::default(),
        thread_id: ThreadId::from_string(item.thread_id_data()).expect("canonical DATA thread"),
        thread_label: None,
        id: label.into(),
        environment_id: None,
        command: vec!["echo".into(), "original".into()],
        reason: None,
        available_decisions: vec![
            CommandExecutionApprovalDecision::Accept,
            CommandExecutionApprovalDecision::Cancel,
        ],
        network_approval_context: None,
        additional_permissions: None,
    })
}
fn native(
    item: &Arc<Item>,
    label: &str,
) -> (
    ApprovalOverlay,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let keys = crate::keymap::RuntimeKeymap::defaults();
    let features = Features::with_defaults();
    item.with_scope(|| crate::app::retained_original_input::approvals::bind_features(&features));
    (
        item.with_scope(|| {
            ApprovalOverlay::new(
                request(item, label),
                AppEventSender::new(tx),
                features,
                keys.approval,
                keys.list,
            )
        }),
        rx,
    )
}
fn key(view: &mut ApprovalOverlay, key: KeyCode) {
    view.handle_key_event(KeyEvent::new(key, KeyModifiers::NONE));
}
type NormalizedFixture = (
    ApprovalOverlay,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    Arc<Item>,
    Arc<Item>,
);
fn normalized() -> io::Result<NormalizedFixture> {
    let a = item(101, "a")?;
    let b = item(102, "b")?;
    let (mut view, rx) = native(&b, "b");
    key(&mut view, KeyCode::Down); // Genuine C2 B editing before original A restoration.
    a.with_scope(|| {
        crate::app::retained_original_input::approvals::bind_features(&Features::with_defaults())
    });
    a.with_scope(|| view.enqueue_request(request(&a, "a")));
    let previous = RequestData::capture(view.current_request.as_ref().expect("actual B"));
    let next = view.queue.pop().expect("same actual A row");
    let old = view.current_request.take().expect("same actual B row");
    view.queue.push(old);
    view.alab_validate_activation_switch(&next, &a)?;
    view.alab_inputs.reorder_actual(&[1, 0])?;
    view.alab_switch_preserving_activation(
        next,
        a.clone(),
        b.clone(),
        previous,
        Features::with_defaults(),
    )?;
    Ok((view, rx, a, b))
}
#[test]
fn c2_b_real_list_and_sender_survive_a_activation_and_original_pop() -> io::Result<()> {
    let (mut view, mut events, a, b) = normalized()?;
    assert!(
        RequestData::capture(&request(&a, "a"))
            .matches_native(view.current_request.as_ref().unwrap())?
    );
    assert_eq!(view.alab_editing.original()?, 0);
    assert_eq!(view.alab_activations.actual.len(), 1);
    key(&mut view, KeyCode::Enter); // A uses its original default choice.
    assert!(
        RequestData::capture(&request(&b, "b"))
            .matches_native(view.current_request.as_ref().unwrap())?
    );
    assert!(view.alab_editing.original()? > 0);
    key(&mut view, KeyCode::Enter); // B retains its original C2 Cancel selection.
    let mut values = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let AppEvent::AlabOriginalInput(event) = event {
            values.push(event.local_ui_original_value_data()?);
        }
    }
    assert_eq!(
        values,
        vec![
            (
                serde_json::json!(101),
                serde_json::json!({"decision":"accept"})
            ),
            (
                serde_json::json!(102),
                serde_json::json!({"decision":"cancel"})
            )
        ]
    );
    assert!(view.alab_activations.actual.is_empty());
    Ok(())
}
#[test]
fn queued_native_editor_survives_e1_data_and_closed_current_pruning() -> io::Result<()> {
    let (view, _events, _a, _b) = normalized()?;
    let raw = view.alab_capture_approval_editing(LIMIT)?;
    let data = Snapshot::decode(&raw, LIMIT)?;
    assert_eq!(data.queued_activations.len(), 1);
    assert_eq!(data.queued_activations[0].position, 1);
    let next = editing_retaining(&raw, &[false, true], LIMIT)?.expect("actual queued B remains");
    assert_eq!(retained_positions(&raw, &[false, true], LIMIT)?, vec![1]);
    let b = item(102, "b")?;
    let (mut actual, mut events) = native(&b, "b");
    let prepared = PreparedApproval::decode(&next, LIMIT)?;
    prepared.validate_destination(&actual)?;
    // Native DATA only; the real installer additionally requires actual
    // replayed callback owners/Source cuts. No callback is built by decode.
    prepared.install_native_editing(&mut actual)?;
    key(&mut actual, KeyCode::Enter);
    let mut found = false;
    while let Ok(event) = events.try_recv() {
        if let AppEvent::AlabOriginalInput(event) = event {
            let (id, value) = event.local_ui_original_value_data()?;
            assert_eq!(id, serde_json::json!(102));
            assert_eq!(value, serde_json::json!({"decision":"cancel"}));
            found = true;
        }
    }
    assert!(found);
    Ok(())
}
#[test]
fn queue_editor_required_shape_and_position_cannot_rebind_another_item() -> io::Result<()> {
    let (view, _events, _a, _b) = normalized()?;
    let raw = view.alab_capture_approval_editing(LIMIT)?;
    let value: serde_json::Value = serde_json::from_str(raw.get()).map_err(invalid)?;
    let mut wrong = value.clone();
    wrong["queued_activations"][0]["position"] = serde_json::json!(0);
    assert!(
        PreparedApproval::decode(
            &serde_json::value::to_raw_value(&wrong).map_err(invalid)?,
            LIMIT
        )
        .is_err()
    );
    let mut missing = value;
    missing.as_object_mut().unwrap().remove("editing");
    assert!(
        PreparedApproval::decode(
            &serde_json::value::to_raw_value(&missing).map_err(invalid)?,
            LIMIT
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn native_queued_editor_cannot_install_without_actual_callback_owner() -> io::Result<()> {
    let (view, _events, a, b) = normalized()?;
    let raw = view.alab_capture_approval_editing(LIMIT)?;
    let (mut actual, mut events) = native(&a, "a");
    b.with_scope(|| actual.enqueue_request(request(&b, "b")));
    assert!(
        PreparedApproval::decode(&raw, LIMIT)?
            .install_bound(&mut actual, &[])
            .is_err()
    );
    assert!(actual.alab_activations.pending.is_empty());
    assert!(events.try_recv().is_err());
    Ok(())
}
