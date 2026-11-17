// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual Source callback lifecycle only. Private ledger setup below supplies
//! no AcceptedOrigin, CurrentOrigin, capture admission, Go or Native permission.
use super::*;
use codex_app_server_protocol::ServerRequestPayload;
use codex_app_server_protocol::ToolRequestUserInputParams;

fn source_endpoint() -> (Arc<OutgoingMessageSender>, mpsc::Receiver<OutgoingEnvelope>) {
    let (tx, rx) = mpsc::channel(16);
    let mut outgoing =
        OutgoingMessageSender::new(tx, codex_analytics::AnalyticsEventsClient::disabled());
    outgoing.alab_callback_outcomes = Some(Arc::new(Ledger::default()));
    outgoing.alab_callback_connection_open(
        ConnectionId(1),
        Arc::new(crate::connection_rpc_gate::ConnectionRpcGate::new()),
    );
    (Arc::new(outgoing), rx)
}
async fn request(
    outgoing: &Arc<OutgoingMessageSender>,
    thread: ThreadId,
) -> (RequestId, oneshot::Receiver<ClientRequestResult>) {
    ThreadScopedOutgoingMessageSender::new(outgoing.clone(), vec![ConnectionId(1)], thread)
        .send_request(ServerRequestPayload::ToolRequestUserInput(
            ToolRequestUserInputParams {
                thread_id: thread.to_string(),
                turn_id: "original-turn".into(),
                item_id: "original-item".into(),
                questions: Vec::new(),
                is_blocking: true,
                auto_resolution_ms: None,
            },
        ))
        .await
}

#[tokio::test]
async fn captured_source_cells_survive_disconnect_and_actual_late_completion() {
    let (outgoing, mut wire) = source_endpoint();
    let thread = ThreadId::new();
    let (id, receiver) = request(&outgoing, thread).await;
    assert!(matches!(
        wire.recv().await,
        Some(OutgoingEnvelope::ToConnection {
            connection_id: ConnectionId(1),
            message: OutgoingMessage::Request(_),
            ..
        })
    ));
    let ledger = outgoing.alab_callback_outcomes.as_ref().unwrap();
    // This exercises the lower Source cells, not the public capture admission
    // which separately requires a genuine physical A/current Native origin.
    let cells = ledger.capture(ConnectionId(1), &[id.clone()]).unwrap();
    let mut changes = cells[0].changed.subscribe();
    outgoing.connection_closed(ConnectionId(1)).await;
    assert!(matches!(cells[0].fact().outcome, Outcome::Pending {}));
    let answer = serde_json::json!({"answers":{}});
    outgoing
        .notify_client_response(ConnectionId(1), id, answer.clone())
        .await;
    assert_eq!(receiver.await.unwrap().unwrap(), answer);
    changes.changed().await.unwrap();
    assert!(matches!(cells[0].fact().outcome, Outcome::Response {}));
    assert!(
        outgoing
            .pending_requests_for_thread(thread)
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn original_thread_cancel_closes_endpoint_but_never_claims_core_consumed_it() {
    let (outgoing, _wire) = source_endpoint();
    let thread = ThreadId::new();
    let (id, receiver) = request(&outgoing, thread).await;
    let cells = outgoing
        .alab_callback_outcomes
        .as_ref()
        .unwrap()
        .capture(ConnectionId(1), &[id])
        .unwrap();
    let error = JSONRPCErrorError {
        code: -32000,
        message: "actual cancellation".into(),
        data: None,
    };
    outgoing
        .cancel_requests_for_thread(thread, Some(error.clone()))
        .await;
    assert_eq!(receiver.await.unwrap().unwrap_err(), error);
    assert!(matches!(cells[0].fact().outcome, Outcome::Cancelled {}));
}

#[tokio::test]
async fn unmarked_real_callback_destruction_stays_unknown_and_cannot_be_acknowledged() {
    let (outgoing, _wire) = source_endpoint();
    let thread = ThreadId::new();
    let (id, receiver) = request(&outgoing, thread).await;
    let ledger = outgoing.alab_callback_outcomes.as_ref().unwrap();
    let cells = ledger.capture(ConnectionId(1), &[id.clone()]).unwrap();
    let entry = outgoing
        .request_id_to_callback
        .lock()
        .await
        .remove(&id)
        .unwrap();
    drop(entry); // Actual sender/Endpoint guard destruction, without a terminal hook.
    assert!(receiver.await.is_err());
    assert!(matches!(cells[0].fact().outcome, Outcome::Unknown {}));
    assert!(ledger.acknowledge(ConnectionId(1), &[id]).is_err());
}

#[tokio::test]
async fn semantic_ack_requires_actual_closed_cells_and_preserves_existing_capture_owner() {
    let (outgoing, _wire) = source_endpoint();
    let thread = ThreadId::new();
    let (id, receiver) = request(&outgoing, thread).await;
    let ledger = outgoing.alab_callback_outcomes.as_ref().unwrap();
    let cells = ledger.capture(ConnectionId(1), &[id.clone()]).unwrap();
    assert!(ledger.acknowledge(ConnectionId(1), &[id.clone()]).is_err());
    assert!(outgoing.cancel_request(&id).await);
    assert!(receiver.await.is_err());
    ledger.acknowledge(ConnectionId(1), &[id.clone()]).unwrap();
    assert!(ledger.capture(ConnectionId(1), &[id]).is_err());
    assert!(matches!(cells[0].fact().outcome, Outcome::Cancelled {}));
}

#[tokio::test]
async fn source_cut_waits_for_actual_removed_endpoint_owner_to_finish_or_drop() {
    let (outgoing, _wire) = source_endpoint();
    let thread = ThreadId::new();
    let (id, receiver) = request(&outgoing, thread).await;
    let cells = outgoing
        .alab_callback_outcomes
        .as_ref()
        .unwrap()
        .capture(ConnectionId(1), &[id.clone()])
        .unwrap();
    let entry = outgoing
        .request_id_to_callback
        .lock()
        .await
        .remove(&id)
        .unwrap();
    let cut = facts_cut(&outgoing, &cells);
    tokio::pin!(cut);
    assert!(
        futures::poll!(&mut cut).is_pending(),
        "removed-but-still-owned is neither Pending nor resolved"
    );
    entry.alab_outcome.as_ref().unwrap().close(Closed::Response);
    let result = serde_json::json!({"answers":{}});
    entry.callback.send(Ok(result.clone())).unwrap();
    assert_eq!(receiver.await.unwrap().unwrap(), result);
    let facts = cut.await.unwrap();
    assert!(matches!(facts[0].outcome, Outcome::Response {}));
}

#[tokio::test]
async fn original_recipient_cells_are_pinned_atomically_before_eof_or_refused_after_it() {
    let (outgoing, mut wire) = source_endpoint();
    let thread = ThreadId::new();
    let (first, first_receiver) = request(&outgoing, thread).await;
    let (second, second_receiver) = request(&outgoing, thread).await;
    assert!(wire.recv().await.is_some());
    assert!(wire.recv().await.is_some());
    let ledger = outgoing.alab_callback_outcomes.as_ref().unwrap();
    let ids = [first.clone(), second.clone()];
    // SAME synchronous cell cut used by actual prepare; no Native context or
    // admission is fabricated in this lower Source lifecycle control.
    let pinned = ledger.capture(ConnectionId(1), &ids).unwrap();
    outgoing.connection_closed(ConnectionId(1)).await;
    assert!(
        ledger.capture(ConnectionId(1), &ids).is_err(),
        "EOF before preparation must refuse, not recreate a bucket"
    );
    assert!(
        ledger.capture(ConnectionId(2), &ids).is_err(),
        "foreign endpoint must not inherit original recipients"
    );
    assert_eq!(pinned.len(), 2);
    assert!(
        pinned
            .iter()
            .all(|record| matches!(record.fact().outcome, Outcome::Pending {}))
    );
    let response = serde_json::json!({"answers":{}});
    outgoing
        .notify_client_response(ConnectionId(1), first, response.clone())
        .await;
    assert_eq!(first_receiver.await.unwrap().unwrap(), response);
    assert!(outgoing.cancel_request(&second).await);
    assert!(second_receiver.await.is_err());
    let facts = facts_cut(&outgoing, &pinned).await.unwrap();
    assert!(matches!(facts[0].outcome, Outcome::Response {}));
    assert!(matches!(facts[1].outcome, Outcome::Cancelled {}));
}

#[tokio::test]
async fn captured_closed_cells_can_ack_after_gap_without_inventing_a_c2_bucket() {
    let (outgoing, _wire) = source_endpoint();
    let thread = ThreadId::new();
    let (id, receiver) = request(&outgoing, thread).await;
    let ledger = outgoing.alab_callback_outcomes.as_ref().unwrap();
    let cells = ledger.capture(ConnectionId(1), &[id.clone()]).unwrap();
    outgoing.connection_closed(ConnectionId(1)).await;
    assert!(
        ledger
            .acknowledge_captured(ConnectionId(2), &cells, &[id.clone()])
            .is_err()
    );
    assert!(outgoing.cancel_request(&id).await);
    assert!(receiver.await.is_err());
    // Lower Source cells only; public ACK separately requires the NEW genuine
    // RequestContext/A/current Native observation, which is not fabricated here.
    ledger
        .acknowledge_captured(ConnectionId(2), &cells, &[id.clone()])
        .unwrap();
    assert!(
        !ledger
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recipients
            .contains_key(&ConnectionId(2))
    );
    assert!(
        ledger
            .acknowledge_captured(
                ConnectionId(2),
                &cells,
                &[RequestId::String("foreign".into())]
            )
            .is_err()
    );
    assert!(
        ledger
            .acknowledge_captured(ConnectionId(2), &cells, &[id.clone(), id])
            .is_err()
    );
    assert!(matches!(cells[0].fact().outcome, Outcome::Cancelled {}));
}

#[tokio::test]
async fn captured_ack_does_not_remove_a_different_actual_cell_from_recipient_state() {
    let (outgoing, _wire) = source_endpoint();
    let thread = ThreadId::new();
    let (first, first_receiver) = request(&outgoing, thread).await;
    let ledger = outgoing.alab_callback_outcomes.as_ref().unwrap();
    let old = ledger.capture(ConnectionId(1), &[first.clone()]).unwrap();
    assert!(outgoing.cancel_request(&first).await);
    assert!(first_receiver.await.is_err());
    let (second, second_receiver) = request(&outgoing, thread).await;
    let new = ledger.capture(ConnectionId(1), &[second.clone()]).unwrap();
    // Negative identity seam: associate a DIFFERENT genuinely registered cell
    // under the old selector to exercise Arc equality independently of IDs.
    // No reply/capture Native admission or authoritative endpoint is fabricated.
    ledger
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recipients
        .entry(ConnectionId(2))
        .or_default()
        .insert(first.clone(), new[0].clone());
    ledger
        .acknowledge_captured(ConnectionId(2), &old, &[first.clone()])
        .unwrap();
    let retained = ledger
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recipients
        .get(&ConnectionId(2))
        .unwrap()
        .get(&first)
        .unwrap()
        .clone();
    assert!(Arc::ptr_eq(&retained, &new[0]));
    assert!(outgoing.cancel_request(&second).await);
    assert!(second_receiver.await.is_err());
}
