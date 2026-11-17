// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual Source callback/send controls only. No Native/AcceptedOrigin/Core
//! positive is manufactured; these invoke the production private-cell send seam.
use super::*;
use codex_app_server_protocol::ServerRequestPayload;
use codex_app_server_protocol::ToolRequestUserInputParams;
use futures::FutureExt;
fn endpoint() -> (Arc<OutgoingMessageSender>, mpsc::Receiver<OutgoingEnvelope>) {
    let (sender, wire) = mpsc::channel(8);
    let mut outgoing =
        OutgoingMessageSender::new(sender, codex_analytics::AnalyticsEventsClient::disabled());
    outgoing.alab_callback_outcomes = Some(Arc::new(Ledger::default()));
    outgoing.alab_callback_connection_open(
        ConnectionId(1),
        Arc::new(crate::connection_rpc_gate::ConnectionRpcGate::new()),
    );
    (Arc::new(outgoing), wire)
}
async fn original(
    outgoing: &Arc<OutgoingMessageSender>,
    wire: &mut mpsc::Receiver<OutgoingEnvelope>,
) -> io::Result<(
    RequestId,
    oneshot::Receiver<ClientRequestResult>,
    Arc<Ledger>,
    Arc<Record>,
)> {
    let thread = ThreadId::new();
    let (id, reply) =
        ThreadScopedOutgoingMessageSender::new(outgoing.clone(), vec![ConnectionId(1)], thread)
            .send_request(ServerRequestPayload::ToolRequestUserInput(
                ToolRequestUserInputParams {
                    thread_id: thread.to_string(),
                    turn_id: "same-turn".into(),
                    item_id: "original-item".into(),
                    questions: Vec::new(),
                    is_blocking: true,
                    auto_resolution_ms: None,
                },
            ))
            .await;
    let _message = wire
        .recv()
        .await
        .ok_or_else(|| invalid("actual original request absent"))?;
    let ledger = outgoing
        .alab_callback_outcomes
        .as_ref()
        .ok_or_else(|| invalid("actual ledger absent"))?
        .clone();
    let record = ledger
        .capture(ConnectionId(1), std::slice::from_ref(&id))?
        .into_iter()
        .next()
        .ok_or_else(|| invalid("actual original cell absent"))?;
    Ok((id, reply, ledger, record))
}
#[tokio::test]
async fn actual_private_cell_publishes_exact_original_value_once() -> io::Result<()> {
    let (outgoing, mut wire) = endpoint();
    let (id, reply, ledger, record) = original(&outgoing, &mut wire).await?;
    let value = serde_json::json!({"answers":{"question":{"answers":["original human answer"]}}});
    if !matches!(
        publish_exact(&outgoing, &ledger, &record, ConnectionId(1), value.clone()).await?,
        DispatchOutcome::Published
    ) {
        return Err(invalid("actual callback did not accept original value"));
    }
    let actual = reply
        .await
        .map_err(io::Error::other)?
        .map_err(|_| invalid("actual original callback refused"))?;
    if actual != value {
        return Err(invalid("published value changed"));
    }
    if !matches!(
        publish_exact(&outgoing, &ledger, &record, ConnectionId(1), value).await?,
        DispatchOutcome::SealedClosed
    ) {
        return Err(invalid("closed endpoint was not sealed"));
    }
    if outgoing
        .request_id_to_callback
        .lock()
        .await
        .contains_key(&id)
    {
        return Err(invalid("second publication recreated original callback"));
    }
    Ok(())
}
#[tokio::test]
async fn actual_receiver_drop_is_distinct_from_endpoint_response_fact() -> io::Result<()> {
    let (outgoing, mut wire) = endpoint();
    let (_id, reply, ledger, record) = original(&outgoing, &mut wire).await?;
    drop(reply);
    if !matches!(
        publish_exact(
            &outgoing,
            &ledger,
            &record,
            ConnectionId(1),
            serde_json::json!({"answers":{}})
        )
        .await?,
        DispatchOutcome::ReceiverClosed
    ) {
        return Err(invalid("actual send rejection was reported accepted"));
    }
    if !matches!(record.fact().outcome, Outcome::Response {}) {
        return Err(invalid("original endpoint response lifecycle changed"));
    }
    Ok(()) // Response alone did not establish acceptance of the exact Value.
}
#[tokio::test]
async fn another_original_response_is_not_attributed_to_this_publication() -> io::Result<()> {
    let (outgoing, mut wire) = endpoint();
    let (id, reply, ledger, record) = original(&outgoing, &mut wire).await?;
    let other = serde_json::json!({"answers":{"question":{"answers":["other recipient answer"]}}});
    outgoing
        .notify_client_response(ConnectionId(1), id, other.clone())
        .await;
    if reply
        .await
        .map_err(io::Error::other)?
        .map_err(|_| invalid("original other response refused"))?
        != other
    {
        return Err(invalid("other response changed"));
    }
    if !matches!(
        publish_exact(
            &outgoing,
            &ledger,
            &record,
            ConnectionId(1),
            serde_json::json!({"answers":{"question":{"answers":["our old answer"]}}})
        )
        .await?,
        DispatchOutcome::SealedClosed
    ) {
        return Err(invalid("other acceptance attributed to original operation"));
    }
    Ok(())
}
#[tokio::test]
async fn original_record_from_another_sender_cannot_publish_by_logical_id() -> io::Result<()> {
    let (outgoing, mut wire) = endpoint();
    let (id, reply, ledger, record) = original(&outgoing, &mut wire).await?;
    let (other, _other_wire) = endpoint();
    if publish_exact(
        &other,
        &ledger,
        &record,
        ConnectionId(1),
        serde_json::json!({"answers":{}}),
    )
    .await
    .is_ok()
    {
        return Err(invalid("another Source sender accepted private cell"));
    }
    if !outgoing
        .request_id_to_callback
        .lock()
        .await
        .contains_key(&id)
    {
        return Err(invalid("refusal consumed original callback"));
    }
    outgoing.cancel_request(&id).await;
    let _reply = reply.await;
    Ok(())
}
#[tokio::test]
async fn removed_but_owned_endpoint_never_becomes_absence_negative() -> io::Result<()> {
    let (outgoing, mut wire) = endpoint();
    let (id, reply, ledger, record) = original(&outgoing, &mut wire).await?;
    let original = outgoing
        .request_id_to_callback
        .lock()
        .await
        .remove(&id)
        .ok_or_else(|| invalid("original endpoint absent"))?;
    let mut publishing = Box::pin(publish_exact(
        &outgoing,
        &ledger,
        &record,
        ConnectionId(1),
        serde_json::json!({"answers":{}}),
    ));
    let early = publishing.as_mut().now_or_never();
    drop(original); // Actual unmarked original owner destruction is Unknown.
    let outcome = publishing.await;
    let _reply = reply.await;
    if early.is_some() || !matches!(outcome, Ok(DispatchOutcome::Unknown)) {
        return Err(invalid(
            "removed/unknown endpoint yielded false publication settlement",
        ));
    }
    Ok(())
}

impl OutgoingMessageSender {
    /// Lower real Source gate/cut fixture only; no Incoming/Native admission.
    pub(crate) fn alab_input_ingress_for_control() -> io::Result<(
        Arc<Self>,
        Arc<crate::connection_rpc_gate::ConnectionRpcGate>,
    )> {
        let (outgoing, _wire) = endpoint();
        let ledger = outgoing
            .alab_callback_outcomes
            .as_ref()
            .ok_or_else(|| invalid("lower Source ledger absent"))?;
        let gate = ledger
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .connections
            .get(&ConnectionId(1))
            .cloned()
            .ok_or_else(|| invalid("actual lower connection gate absent"))?;
        Ok((outgoing, gate))
    }
}
