// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original Source callback-map/oneshot/replay admission only. No
//! AcceptedOrigin, BoundBackend, Core, Native ACK or UI application is faked.
use super::*;
use codex_app_server_protocol::ServerRequestPayload;
use codex_app_server_protocol::ToolRequestUserInputParams;
fn endpoint(
    records: usize,
    associations: usize,
) -> (Arc<OutgoingMessageSender>, mpsc::Receiver<OutgoingEnvelope>) {
    let (sender, wire) = mpsc::channel(16);
    let mut outgoing =
        OutgoingMessageSender::new(sender, codex_analytics::AnalyticsEventsClient::disabled());
    outgoing.alab_callback_outcomes = Some(Arc::new(Ledger {
        limits: Limits {
            records,
            associations,
        },
        ..Ledger::default()
    }));
    for id in [ConnectionId(1), ConnectionId(2)] {
        outgoing.alab_callback_connection_open(
            id,
            Arc::new(crate::connection_rpc_gate::ConnectionRpcGate::new()),
        );
    }
    (Arc::new(outgoing), wire)
}
async fn request(
    outgoing: &Arc<OutgoingMessageSender>,
    thread: ThreadId,
) -> (RequestId, oneshot::Receiver<ClientRequestResult>) {
    ThreadScopedOutgoingMessageSender::new(outgoing.clone(), vec![ConnectionId(1)], thread)
        .send_request(ServerRequestPayload::ToolRequestUserInput(
            ToolRequestUserInputParams {
                thread_id: thread.to_string(),
                turn_id: "turn".into(),
                item_id: "item".into(),
                questions: Vec::new(),
                is_blocking: true,
                auto_resolution_ms: None,
            },
        ))
        .await
}
fn ledger(outgoing: &OutgoingMessageSender) -> Result<&Arc<Ledger>, &'static str> {
    outgoing
        .alab_callback_outcomes
        .as_ref()
        .ok_or("actual Source ledger missing")
}
#[tokio::test]
async fn capacity_refuses_original_waiter_before_request_or_callback_publication()
-> Result<(), &'static str> {
    let (outgoing, mut wire) = endpoint(1, 2);
    let thread = ThreadId::new();
    let (first, first_reply) = request(&outgoing, thread).await;
    let _first = wire.recv().await.ok_or("first request missing")?;
    let original = ledger(&outgoing)?
        .capture(ConnectionId(1), &[first.clone()])
        .map_err(|_| "original cell missing")?;
    let (refused, reply) = request(&outgoing, thread).await;
    if !matches!(reply.await, Ok(Err(_))) {
        return Err("capacity did not return original oneshot error");
    }
    if !matches!(wire.try_recv(), Err(mpsc::error::TryRecvError::Empty)) {
        return Err("refused request was published");
    }
    if outgoing
        .request_id_to_callback
        .lock()
        .await
        .contains_key(&refused)
    {
        return Err("refused callback was registered");
    }
    let same = ledger(&outgoing)?
        .capture(ConnectionId(1), &[first.clone()])
        .map_err(|_| "original record was evicted")?;
    if !Arc::ptr_eq(&original[0], &same[0]) {
        return Err("capacity replaced original cell");
    }
    outgoing.cancel_request(&first).await;
    let _first_reply = first_reply.await;
    Ok(())
}
#[tokio::test]
async fn whole_replay_capacity_failure_precedes_any_replayed_request() -> Result<(), &'static str> {
    let (outgoing, mut wire) = endpoint(2, 1);
    let thread = ThreadId::new();
    let (id, reply) = request(&outgoing, thread).await;
    let _first = wire.recv().await.ok_or("first request missing")?;
    if outgoing
        .replay_requests_to_connection_for_thread(ConnectionId(2), thread)
        .await
        .is_ok()
    {
        return Err("unreserved replay succeeded");
    }
    if !matches!(wire.try_recv(), Err(mpsc::error::TryRecvError::Empty)) {
        return Err("partial replay was published");
    }
    if ledger(&outgoing)?
        .capture(ConnectionId(2), &[id.clone()])
        .is_ok()
    {
        return Err("failed replay fabricated recipient association");
    }
    if ledger(&outgoing)?
        .capture(ConnectionId(1), &[id.clone()])
        .is_err()
    {
        return Err("failed replay evicted original");
    }
    outgoing.cancel_request(&id).await;
    let _reply = reply.await;
    Ok(())
}
#[tokio::test]
async fn actual_map_snapshot_pins_cells_across_eof_and_endpoint_removal() -> Result<(), &'static str>
{
    let (outgoing, mut wire) = endpoint(2, 2);
    let thread = ThreadId::new();
    let (id, reply) = request(&outgoing, thread).await;
    let _first = wire.recv().await.ok_or("first request missing")?;
    let requests = outgoing.pending_requests_for_thread(thread).await;
    outgoing.connection_closed(ConnectionId(1)).await;
    outgoing
        .notify_client_response(
            ConnectionId(1),
            id.clone(),
            serde_json::json!({"answers":{}}),
        )
        .await;
    let _accepted = reply
        .await
        .map_err(|_| "actual original callback closed")?
        .map_err(|_| "actual response refused")?;
    let reserved = requests
        .reserve(&outgoing, ConnectionId(2), true)
        .map_err(|_| "original pinned snapshot lost")?
        .ok_or("Source reservation missing")?;
    let actual = requests.first().ok_or("actual request snapshot missing")?;
    reserved
        .install(actual)
        .map_err(|_| "original pinned cell could not install")?;
    outgoing
        .sender
        .send(OutgoingEnvelope::ToConnection {
            connection_id: ConnectionId(2),
            message: OutgoingMessage::Request(actual.clone()),
            write_complete_tx: None,
        })
        .await
        .map_err(|_| "actual replay publication failed")?;
    let _published = wire
        .recv()
        .await
        .ok_or("actual replay publication missing")?;
    let cells = ledger(&outgoing)?
        .capture(ConnectionId(2), &[id])
        .map_err(|_| "reserved actual cell absent")?;
    if !matches!(cells[0].fact().outcome, Outcome::Response {}) {
        return Err("snapshot recreated Pending after real response");
    }
    Ok(())
}
#[tokio::test]
async fn unspent_replay_budget_drops_without_retiring_endpoint_facts() -> Result<(), &'static str> {
    let (outgoing, mut wire) = endpoint(2, 2);
    let thread = ThreadId::new();
    let (id, reply) = request(&outgoing, thread).await;
    let _first = wire.recv().await.ok_or("first request missing")?;
    let requests = outgoing.pending_requests_for_thread(thread).await;
    let reserved = requests
        .reserve(&outgoing, ConnectionId(2), true)
        .map_err(|_| "reserve failed")?
        .ok_or("Source reservation missing")?;
    if ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .reserved
        != 1
    {
        return Err("actual budget not held");
    }
    drop(reserved);
    if ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .reserved
        != 0
    {
        return Err("unused budget leaked");
    }
    if ledger(&outgoing)?
        .capture(ConnectionId(2), &[id.clone()])
        .is_ok()
    {
        return Err("unused budget fabricated recipient");
    }
    outgoing.cancel_request(&id).await;
    let _reply = reply.await;
    Ok(())
}
#[tokio::test]
async fn repeated_normal_closed_endpoint_ack_reclaims_budget_without_evicting_facts()
-> Result<(), &'static str> {
    let (outgoing, mut wire) = endpoint(1, 1);
    let thread = ThreadId::new();
    for _ in 0..512 {
        let (id, reply) = request(&outgoing, thread).await;
        let _request = wire.recv().await.ok_or("normal request missing")?;
        outgoing
            .notify_client_response(
                ConnectionId(1),
                id.clone(),
                serde_json::json!({"answers":{}}),
            )
            .await;
        let _accepted = reply
            .await
            .map_err(|_| "original normal callback lost")?
            .map_err(|_| "original response rejected")?;
        ledger(&outgoing)?
            .acknowledge(ConnectionId(1), &[id])
            .map_err(|_| "actual closed endpoint ACK failed")?;
    }
    if !ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recipients
        .is_empty()
    {
        return Err("empty recipient bucket leaked");
    }
    Ok(())
}
#[tokio::test]
async fn actual_registered_connection_close_wakes_only_readonly_observer()
-> Result<(), &'static str> {
    let (outgoing, mut wire) = endpoint(2, 2);
    let thread = ThreadId::new();
    let (id, reply) = request(&outgoing, thread).await;
    let _request = wire.recv().await.ok_or("actual request absent")?;
    let request = ConnectionRequestId {
        connection_id: ConnectionId(1),
        request_id: RequestId::String("actual-observe".into()),
    };
    let context = RequestContext::new(request, "config/read", tracing::Span::none(), None);
    outgoing.register_request_context(context.clone()).await;
    let observer = outgoing
        .alab_callback_observer(&context)
        .await
        .map_err(|_| "observer registration failed")?;
    outgoing.connection_closed(ConnectionId(1)).await;
    tokio::time::timeout(std::time::Duration::from_secs(60), observer.cancelled())
        .await
        .map_err(|_| "physical close observer not woken")?;
    if !outgoing
        .request_id_to_callback
        .lock()
        .await
        .contains_key(&id)
    {
        return Err("readonly observer close cancelled actual callback");
    }
    outgoing.cancel_request(&id).await;
    let _reply = reply.await;
    Ok(())
}
#[tokio::test]
async fn connection_close_before_observer_registration_cannot_recreate_liveness()
-> Result<(), &'static str> {
    let (outgoing, _wire) = endpoint(1, 1);
    let context = RequestContext::new(
        ConnectionRequestId {
            connection_id: ConnectionId(1),
            request_id: RequestId::String("closed-before-worker".into()),
        },
        "config/read",
        tracing::Span::none(),
        None,
    );
    outgoing.register_request_context(context.clone()).await;
    outgoing.connection_closed(ConnectionId(1)).await;
    if outgoing.alab_callback_observer(&context).await.is_ok() {
        return Err("closed actual context was recreated");
    }
    Ok(())
}

#[tokio::test]
async fn real_gate_close_before_async_cleanup_prevents_reserved_replay_install()
-> Result<(), &'static str> {
    let (outgoing, mut wire) = endpoint(2, 2);
    let thread = ThreadId::new();
    let (id, reply) = request(&outgoing, thread).await;
    let _original = wire.recv().await.ok_or("original request absent")?;
    let actual = ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .connections
        .get(&ConnectionId(2))
        .cloned()
        .ok_or("actual connection gate absent")?;
    let requests = outgoing.pending_requests_for_thread(thread).await;
    let reserved = requests
        .reserve(&outgoing, ConnectionId(2), true)
        .map_err(|_| "reserve failed")?
        .ok_or("reservation absent")?;
    actual.close().await; // Original physical EOF closes this SAME gate before async ledger cleanup.
    if reserved
        .install(requests.first().ok_or("actual snapshot absent")?)
        .is_ok()
    {
        return Err("closed actual gate admitted late replay association");
    }
    if ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recipients
        .contains_key(&ConnectionId(2))
    {
        return Err("closed gate acquired new recipient association");
    }
    drop(reserved);
    if ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .reserved
        != 0
    {
        return Err("rejected replay leaked budget");
    }
    outgoing.cancel_request(&id).await;
    let _reply = reply.await;
    Ok(())
}
#[tokio::test]
async fn async_close_cleanup_cannot_be_undone_by_old_replay_reservation() -> Result<(), &'static str>
{
    let (outgoing, mut wire) = endpoint(2, 2);
    let thread = ThreadId::new();
    let (id, reply) = request(&outgoing, thread).await;
    let _original = wire.recv().await.ok_or("original request absent")?;
    let requests = outgoing.pending_requests_for_thread(thread).await;
    let reserved = requests
        .reserve(&outgoing, ConnectionId(2), true)
        .map_err(|_| "reserve failed")?
        .ok_or("reservation absent")?;
    outgoing.connection_closed(ConnectionId(2)).await;
    if reserved
        .install(requests.first().ok_or("actual snapshot absent")?)
        .is_ok()
    {
        return Err("cleanup was undone by late reserved install");
    }
    if ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recipients
        .contains_key(&ConnectionId(2))
    {
        return Err("closed recipient bucket recreated");
    }
    outgoing.alab_callback_connection_open(
        ConnectionId(2),
        Arc::new(crate::connection_rpc_gate::ConnectionRpcGate::new()),
    );
    if reserved
        .install(requests.first().ok_or("actual snapshot absent")?)
        .is_ok()
    {
        return Err("old reservation inherited new physical gate");
    }
    drop(reserved);
    outgoing.cancel_request(&id).await;
    let _reply = reply.await;
    Ok(())
}
#[tokio::test]
async fn late_original_register_preserves_endpoint_without_closed_recipient_association()
-> Result<(), &'static str> {
    let (outgoing, mut wire) = endpoint(2, 2);
    let thread = ThreadId::new();
    let actual = ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .connections
        .get(&ConnectionId(1))
        .cloned()
        .ok_or("actual connection gate absent")?;
    actual.close().await;
    let (id, reply) = request(&outgoing, thread).await;
    let _original = wire
        .recv()
        .await
        .ok_or("accepted original request missing")?;
    if !outgoing
        .request_id_to_callback
        .lock()
        .await
        .contains_key(&id)
    {
        return Err("closed recipient cancelled accepted endpoint");
    }
    if ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recipients
        .contains_key(&ConnectionId(1))
    {
        return Err("late original register pinned closed recipient");
    }
    let requests = outgoing.pending_requests_for_thread(thread).await;
    let reserved = requests
        .reserve(&outgoing, ConnectionId(2), true)
        .map_err(|_| "accepted endpoint could not replay to actual live connection")?
        .ok_or("reservation absent")?;
    reserved
        .install(requests.first().ok_or("actual pending snapshot absent")?)
        .map_err(|_| "accepted endpoint lost before live replay")?;
    let cells = ledger(&outgoing)?
        .capture(ConnectionId(2), &[id.clone()])
        .map_err(|_| "actual live association absent")?;
    if !matches!(cells[0].fact().outcome, Outcome::Pending {}) {
        return Err("late register inferred endpoint closure");
    }
    drop(reserved);
    outgoing.cancel_request(&id).await;
    let _reply = reply.await;
    Ok(())
}
#[tokio::test]
async fn actual_gate_close_refuses_observer_before_async_context_cleanup()
-> Result<(), &'static str> {
    let (outgoing, _wire) = endpoint(1, 1);
    let context = RequestContext::new(
        ConnectionRequestId {
            connection_id: ConnectionId(1),
            request_id: RequestId::String("closed-before-cleanup".into()),
        },
        "config/read",
        tracing::Span::none(),
        None,
    );
    outgoing.register_request_context(context.clone()).await;
    let actual = ledger(&outgoing)?
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .connections
        .get(&ConnectionId(1))
        .cloned()
        .ok_or("actual connection gate absent")?;
    actual.close().await;
    if outgoing.alab_callback_observer(&context).await.is_ok() {
        return Err("closed actual gate admitted observer before cleanup");
    }
    Ok(())
}
