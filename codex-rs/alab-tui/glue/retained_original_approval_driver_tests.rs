// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Local native queue and immutable publication DATA only. Restored operations
//! have no genuine reply, Source capture/receipt, actual tenure or Native grant.
use super::*;
use crate::bottom_pane::ExecApprovalRequest;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
pub(crate) fn item(id: i64, label: &str) -> io::Result<Arc<Item>> {
    let request = ServerRequest::CommandExecutionRequestApproval {
        request_id: RequestId::Integer(id),
        params: CommandExecutionRequestApprovalParams {
            kind: Default::default(),
            thread_id: "00000000-0000-0000-0000-000000000001".into(),
            turn_id: "same-turn".into(),
            item_id: label.into(),
            started_at_ms: 0,
            approval_id: None,
            environment_id: None,
            reason: None,
            network_approval_context: None,
            command: Some("echo original".into()),
            cwd: None,
            command_actions: None,
            additional_permissions: None,
            proposed_execpolicy_amendment: None,
            proposed_network_policy_amendments: None,
            available_decisions: None,
        },
    };
    let request =
        InputRequest::from_original(&request).ok_or_else(|| invalid("typed approval missing"))?;
    let reservation=OriginalInputReservation::restore_checkpoint(serde_json::json!({"schema":1,"service":"00000000000000000000000000000001","operation":format!("{id:032x}"),"requestId":id,"descriptor":null,"value":null,"receipt":null})).map_err(io::Error::other)?;
    Ok(Arc::new(Item {
        owner: Weak::new(),
        request,
        native_approval: Mutex::new(None),
        native_features: Mutex::new(Ok(None)),
        native_faulted: std::sync::atomic::AtomicBool::new(false),
        reservation: Mutex::new(reservation),
        serial: Semaphore::new(1),
        progress: Mutex::new(Progress::default()),
    }))
}
fn native(item: &Arc<Item>, label: &str) -> io::Result<ApprovalRequest> {
    Ok(ApprovalRequest::Exec(ExecApprovalRequest {
        kind: Default::default(),
        thread_id: codex_protocol::ThreadId::from_string(item.thread_id_data()).map_err(invalid)?,
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
    }))
}
#[test]
fn original_approval_sender_freezes_value_before_private_event_and_keeps_a_after_advance()
-> io::Result<()> {
    let a = item(71, "a")?;
    let b = item(72, "b")?;
    let ar = native(&a, "a")?;
    let br = native(&b, "b")?;
    a.with_scope(|| bind_features(&codex_features::Features::with_defaults()));
    let mut bindings = a.with_scope(Bindings::capture);
    b.with_scope(|| bindings.push(&br));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sender = crate::app_event_sender::AppEventSender::new(tx);
    let sender = bindings.sender(&sender, &ar);
    let thread = codex_protocol::ThreadId::from_string(a.thread_id_data()).map_err(invalid)?;
    sender.send(crate::app_event::AppEvent::SubmitThreadOp {
        thread_id: thread,
        op: AppCommand::ExecApproval {
            id: "a".into(),
            turn_id: None,
            decision: CommandExecutionApprovalDecision::Cancel,
        },
    });
    bindings.advance();
    let crate::app_event::AppEvent::AlabOriginalInput(event) = rx.try_recv().map_err(invalid)?
    else {
        return Err(invalid("original approval did not enter protected event"));
    };
    assert!(Arc::ptr_eq(&event.item, &a));
    assert!(
        bindings
            .current
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &b))
    );
    let value = a
        .reservation
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .checkpoint()
        .map_err(io::Error::other)?["value"]
        .clone();
    assert_eq!(
        value,
        serde_json::to_value(
            codex_app_server_protocol::CommandExecutionRequestApprovalResponse {
                decision: CommandExecutionApprovalDecision::Cancel
            }
        )
        .map_err(invalid)?
    );
    assert!(
        a.freeze_value(serde_json::json!({"decision":"accept"}))
            .is_err()
    );
    assert!(
        b.reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending_publication()
            .is_none()
    );
    Ok(())
}
#[test]
fn wrong_native_response_is_held_without_raw_send_or_value_binding() -> io::Result<()> {
    let a = item(73, "a")?;
    a.with_scope(|| bind_features(&codex_features::Features::with_defaults()));
    let native = native(&a, "a")?;
    let bindings = a.with_scope(Bindings::capture);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sender = bindings.sender(&crate::app_event_sender::AppEventSender::new(tx), &native);
    let thread = codex_protocol::ThreadId::from_string(a.thread_id_data()).map_err(invalid)?;
    sender.send(crate::app_event::AppEvent::SubmitThreadOp {
        thread_id: thread,
        op: AppCommand::ExecApproval {
            id: "b".into(),
            turn_id: None,
            decision: CommandExecutionApprovalDecision::Accept,
        },
    });
    assert!(rx.try_recv().is_err());
    assert!(!a.has_original_value());
    Ok(())
}
#[test]
fn actual_queue_permutation_moves_same_items_and_refuses_loss_before_mutation() -> io::Result<()> {
    let a = item(81, "a")?;
    let b = item(82, "b")?;
    let c = item(83, "c")?;
    let mut bindings = Bindings {
        current: Some(b.clone()),
        queued: vec![Some(c.clone()), Some(a.clone())],
    };
    assert!(bindings.reorder_actual(&[2, 1, 1]).is_err());
    assert!(
        bindings
            .current
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &b))
    );
    bindings.reorder_actual(&[2, 1, 0])?;
    let actual = bindings.actual_items();
    assert!(Arc::ptr_eq(actual[0].as_ref().unwrap(), &a));
    assert!(Arc::ptr_eq(actual[1].as_ref().unwrap(), &c));
    assert!(Arc::ptr_eq(actual[2].as_ref().unwrap(), &b));
    Ok(())
}
#[test]
fn real_feature_policy_is_retained_and_changed_policy_fault_is_sticky() -> io::Result<()> {
    let a = item(91, "a")?;
    let original = codex_features::Features::with_defaults();
    a.with_scope(|| bind_features(&original));
    assert_eq!(a.actual_features()?, original);
    *a.native_features
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Err(());
    a.with_scope(|| bind_features(&original));
    assert!(a.actual_features().is_err());
    Ok(())
}
