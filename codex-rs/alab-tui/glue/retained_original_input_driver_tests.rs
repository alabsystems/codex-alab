// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual private UI queue/event custody only. These restored DATA owners have
//! no actual reply, Capture, FrontendTenure, Source result, or Native grant.
use super::*;

fn item(id: i64, label: &str) -> io::Result<Arc<Item>> {
    let reservation = OriginalInputReservation::restore_checkpoint(serde_json::json!({
        "schema":1,"service":"00000000000000000000000000000001",
        "operation":format!("{id:032x}"),"requestId":id,"descriptor":null,"value":null,"receipt":null
    })).map_err(io::Error::other)?;
    Ok(Arc::new(Item {
        owner: Weak::new(),
        request: InputRequest::UserInput {
            params: ToolRequestUserInputParams {
                thread_id: "00000000-0000-0000-0000-000000000001".into(),
                turn_id: "same-turn".into(),
                item_id: label.into(),
                questions: Vec::new(),
                is_blocking: true,
                auto_resolution_ms: None,
            },
        },
        native_approval: Mutex::new(None),
        native_features: Mutex::new(Ok(None)),
        native_faulted: std::sync::atomic::AtomicBool::new(false),
        reservation: Mutex::new(reservation),
        serial: Semaphore::new(1),
        progress: Mutex::new(Progress::default()),
    }))
}
fn answer(text: &str) -> ToolRequestUserInputResponse {
    serde_json::from_value(serde_json::json!({"answers":{"q":{"answers":[text]}}}))
        .expect("typed response")
}
#[test]
fn delayed_a_event_keeps_a_when_same_turn_queue_has_advanced_to_b() -> io::Result<()> {
    let a = item(17, "a")?;
    let b = item(18, "b")?;
    let mut queue = a.with_scope(QueueBindings::capture);
    b.with_scope(|| queue.push());
    let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
    let sender = crate::app_event_sender::AppEventSender::new(sender);
    let thread = codex_protocol::ThreadId::from_string(a.thread_id_data()).map_err(invalid)?;
    if !queue.submit(&sender, thread, "same-turn".into(), answer("original A")) {
        return Err(invalid("A did not enqueue"));
    }
    queue.advance();
    // This is the actual private AppEvent already queued by A, not a turn-ID
    // lookup into the current B entry or a reconstruction of a reply.
    let crate::app_event::AppEvent::AlabOriginalInput(event) =
        events.try_recv().map_err(invalid)?
    else {
        return Err(invalid("wrong event"));
    };
    assert!(Arc::ptr_eq(&event.item, &a));
    assert_eq!(
        event.publication.reservation().request_id_data(),
        RequestId::Integer(17)
    );
    assert!(Arc::ptr_eq(queue.current.as_ref().unwrap(), &b));
    assert!(
        b.reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending_publication()
            .is_none()
    );
    assert!(a.freeze(answer("different answer")).is_err());
    Ok(())
}
#[test]
fn deferred_scope_retains_each_item_and_restores_outer_owner_after_unwind() -> io::Result<()> {
    let a = item(21, "same-item")?;
    let b = item(22, "same-item")?;
    let _outer = enter(Some(a.clone()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        b.with_scope(|| panic!("actual scope unwinds"))
    }));
    assert!(result.is_err());
    assert!(current().is_some_and(|actual| Arc::ptr_eq(&actual, &a)));
    let _resolved = resolving(&RequestId::Integer(21));
    let mut queue = QueueBindings {
        current: Some(b.clone()),
        queued: VecDeque::from([Some(a.clone()), Some(b.clone())]),
    };
    let mut requests = VecDeque::from([
        a.request.user_input().unwrap().clone(),
        b.request.user_input().unwrap().clone(),
    ]);
    queue.retain(&mut requests, "same-item");
    assert_eq!(requests.len(), 1);
    assert!(Arc::ptr_eq(
        queue.queued.front().unwrap().as_ref().unwrap(),
        &b
    ));
    assert!(!queue.current_matches_resolved());
    Ok(())
}
#[test]
fn frozen_before_dispatch_cannot_seal_without_actual_source_bind() -> io::Result<()> {
    let a = item(31, "a")?;
    let original = a
        .reservation
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .checkpoint()
        .map_err(io::Error::other)?;
    let _event = a.freeze(answer("original"))?;
    assert!(a.checkpoint().is_err());
    let data = a
        .reservation
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .checkpoint()
        .map_err(io::Error::other)?;
    assert_eq!(original["operation"], data["operation"]);
    assert_eq!(data["requestId"], serde_json::json!(31));
    assert!(!data["value"].is_null());
    assert!(data["descriptor"].is_null()); // No Source binding/grant is fabricated.
    let restored = OriginalInputReservation::restore_checkpoint(data).map_err(io::Error::other)?;
    assert!(
        restored
            .freeze(serde_json::to_value(answer("another")).map_err(invalid)?)
            .is_err()
    );
    Ok(())
}
#[test]
fn closed_unused_item_cannot_accept_late_queued_value() -> io::Result<()> {
    let a = item(41, "a")?;
    a.progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .closed_unused = true;
    assert!(a.freeze(answer("late")).is_err());
    assert!(
        a.reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending_publication()
            .is_none()
    );
    Ok(())
}

#[test]
fn exact_closed_registry_removal_keeps_newer_same_turn_callback() -> io::Result<()> {
    let a = item(51, "a")?;
    let b = item(52, "b")?;
    let mut pending = PendingAppServerRequests::default();
    let first = ServerRequest::ToolRequestUserInput {
        request_id: a.id(),
        params: a.request.user_input().unwrap().clone(),
    };
    let second = ServerRequest::ToolRequestUserInput {
        request_id: b.id(),
        params: b.request.user_input().unwrap().clone(),
    };
    assert!(pending.note_server_request(&first).is_none());
    assert!(pending.note_server_request(&second).is_none());
    // These are the actual lower original registry methods. No callback reply
    // resource or Source publication/retirement permission is manufactured.
    let removed = pending
        .resolve_notification(a.thread_id_data(), &a.id())
        .ok_or_else(|| invalid("A missing"))?;
    assert_eq!(removed.thread_id, a.thread_id_data());
    let result = pending
        .take_resolution_record(
            b.thread_id_data(),
            AppCommand::user_input_answer("same-turn".into(), answer("B")),
        )
        .map_err(invalid)?
        .ok_or_else(|| invalid("B missing"))?;
    assert_eq!(result.request.request_id, b.id());
    assert!(
        pending
            .resolve_notification(a.thread_id_data(), &a.id())
            .is_none()
    );
    Ok(())
}
