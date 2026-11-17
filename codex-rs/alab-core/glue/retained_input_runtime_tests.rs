// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual Source bookkeeping/auth only. These sign no Native/App/Core grant.
use super::*;
fn descriptor(auth: &auth::Auth, ordinal: u64) -> Result<InputDescriptor, JSONRPCErrorError> {
    let mut d = InputDescriptor {
        epoch: auth.epoch.clone(),
        ordinal,
        operation: format!("{ordinal:032x}"),
        request_id: RequestId::Integer(17),
        proof: String::new(),
    };
    d.proof = auth
        .sign("original-input-descriptor", &descriptor_body(&d))
        .map_err(native_error)?;
    Ok(d)
}
#[test]
fn actual_source_receipt_binds_exact_original_value_and_operation() -> Result<(), JSONRPCErrorError>
{
    let auth = auth::Auth::new();
    let d = descriptor(&auth, 1)?;
    let value = serde_json::json!({"answers":{"question":{"answers":["original human Value"]}}});
    let mut receipt = InputReceipt {
        descriptor: d,
        outcome: PublicationOutcome::Published {},
        proof: String::new(),
    };
    receipt.proof = auth
        .sign(
            "original-input-receipt",
            &(&receipt.descriptor, &value, &receipt.outcome),
        )
        .map_err(native_error)?;
    verify_receipt(&auth, &receipt, &value)?;
    if verify_receipt(&auth, &receipt, &serde_json::json!({"answers":{}})).is_ok() {
        return Err(invalid_request("receipt accepted another original Value"));
    }
    let other = auth::Auth::new();
    if verify_receipt(&other, &receipt, &value).is_ok() {
        return Err(invalid_request(
            "another Source reconstructed old publication receipt",
        ));
    }
    receipt.descriptor.request_id = RequestId::Integer(18);
    if verify_receipt(&auth, &receipt, &value).is_ok() {
        return Err(invalid_request("logical callback replacement accepted"));
    }
    Ok(())
}
#[test]
fn terminal_retirement_intervals_reclaim_actual_book_budget_without_forgetting_unknown_hole()
-> Result<(), &'static str> {
    let mut book = book::Book::default();
    let hole = book.admit(2)?;
    book.insert(hole, "unknown-original-input".into(), ());
    for i in 0..4096 {
        let ordinal = book.admit(2)?;
        let operation = format!("normal-{i}");
        book.insert(ordinal, operation.clone(), ());
        if book.retire(ordinal, &operation).is_none() {
            return Err("actual Source bookkeeping retirement missing");
        }
        if !book.retired.contains(ordinal) {
            return Err("lost-reply retry interval forgotten");
        }
    }
    if book.retired.contains(hole) || book.active.len() != 1 {
        return Err("Unknown owner was evicted or normal owners leaked");
    }
    Ok(())
}

#[tokio::test]
async fn queued_original_value_wins_before_new_origin_conditional_binding()
-> Result<(), Box<dyn std::error::Error>> {
    use futures::FutureExt;
    let (outgoing, gate) =
        crate::outgoing_message::OutgoingMessageSender::alab_input_ingress_for_control()?;
    let frame = outgoing
        .alab_original_input_frame(&gate)
        .ok_or("actual original frame lease absent")?;
    let cut = frame.connection_for_control();
    let progress = StdMutex::new(Progress::default());
    let original = serde_json::json!({"answers":{"q":{"answers":["queued original A"]}}});
    let competing = serde_json::json!({"answers":{"q":{"answers":["competing replacement B"]}}});
    gate.close().await;
    outgoing.alab_callback_connection_closing(crate::outgoing_message::ConnectionId(1), &gate);
    outgoing.alab_callback_ingress_drained(crate::outgoing_message::ConnectionId(1));
    let mut replacement = Box::pin(bind_after_original_ingress(
        &progress,
        cut.drain(),
        &competing,
    ));
    let early = replacement.as_mut().now_or_never();
    // Actual old frame's synchronous registry admission freezes its Value
    // BEFORE ending the real read-to-dispatch lease.
    let original_result = progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .bind_value(&original);
    frame.accepted();
    let actual = replacement.await;
    original_result.map_err(|_| "original registry rejected first immutable value")?;
    if early.is_some()
        || actual.is_ok()
        || progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .value
            .as_ref()
            != Some(&original)
    {
        return Err("replacement won before original accepted frame drained".into());
    }
    Ok(())
}

#[tokio::test]
async fn genuinely_drained_missing_value_can_bind_same_conditional_operation()
-> Result<(), Box<dyn std::error::Error>> {
    use futures::FutureExt;
    let (outgoing, gate) =
        crate::outgoing_message::OutgoingMessageSender::alab_input_ingress_for_control()?;
    let frame = outgoing
        .alab_original_input_frame(&gate)
        .ok_or("actual original frame lease absent")?;
    let cut = frame.connection_for_control();
    let progress = StdMutex::new(Progress::default());
    let original = serde_json::json!({"answers":{"q":{"answers":["same frozen A"]}}});
    gate.close().await;
    outgoing.alab_callback_connection_closing(crate::outgoing_message::ConnectionId(1), &gate);
    outgoing.alab_callback_ingress_drained(crate::outgoing_message::ConnectionId(1));
    let mut replacement = Box::pin(bind_after_original_ingress(
        &progress,
        cut.drain(),
        &original,
    ));
    let early = replacement.as_mut().now_or_never();
    drop(frame); // Genuine original queued/preaccept path destruction; no publication owner accepted it.
    let actual = replacement.await;
    actual
        .map_err(|_| "same conditional operation remained permanently refused after real drain")?;
    if early.is_some()
        || progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .value
            .as_ref()
            != Some(&original)
    {
        return Err(
            "missing-value conditional bind skipped actual drain or changed original value".into(),
        );
    }
    Ok(())
}

#[tokio::test]
async fn actual_source_stop_ends_only_prepublication_ingress_wait_without_binding_or_receipt()
-> Result<(), &'static str> {
    use futures::FutureExt;
    let progress = StdMutex::new(Progress::default());
    let value = serde_json::json!({"answers":{"q":{"answers":["still-owned original A"]}}});
    let stop = tokio_util::sync::CancellationToken::new();
    let mut wait = Box::pin(observe_until_stop(
        stop.cancelled(),
        std::future::pending(),
        bind_after_original_ingress(&progress, std::future::pending(), &value),
    ));
    let early = wait.as_mut().now_or_never();
    stop.cancel();
    let result = wait.await;
    let state = progress.lock().unwrap_or_else(PoisonError::into_inner);
    if early.is_some()
        || result.is_ok()
        || state.value.is_some()
        || state.delivery.is_some()
        || state.receipt.is_some()
    {
        return Err("Source stop fabricated input binding/publication/cleanup");
    }
    Ok(())
}
