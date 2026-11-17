// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual Outgoing callback/queue controls, not SDK or frontend admission proof.

use super::super::ConnectionId;
use super::super::OutgoingMessage;
use super::super::ThreadScopedOutgoingMessageSender;
use super::*;
use crate::error_code::internal_error;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestParams;
use codex_app_server_protocol::ServerRequestPayload;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Waker;
use std::time::Duration;
use tokio::sync::oneshot;

struct Provider;
impl IncomingStreamProvider for Provider {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::pending())
    }
    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(std::future::pending())
    }
}

struct Harness {
    incoming: Incoming,
    outgoing: Arc<OutgoingMessageSender>,
    messages: mpsc::Receiver<OutgoingEnvelope>,
    thread: ThreadId,
}
fn retained() -> Harness {
    let incoming = Incoming::new(Provider);
    let (send, messages) = mpsc::channel(8);
    let outgoing = Arc::new(OutgoingMessageSender::alab_for_retained_input(
        send,
        AnalyticsEventsClient::disabled(),
        incoming.clone(),
    ));
    for id in [ConnectionId(1), ConnectionId(2)] {
        outgoing.alab_callback_connection_open(
            id,
            Arc::new(crate::connection_rpc_gate::ConnectionRpcGate::new()),
        );
    }
    Harness {
        incoming,
        outgoing,
        messages,
        thread: ThreadId::new(),
    }
}
fn scoped(h: &Harness) -> ThreadScopedOutgoingMessageSender {
    ThreadScopedOutgoingMessageSender::new(h.outgoing.clone(), Vec::new(), h.thread)
}
fn human(thread: ThreadId) -> ServerRequestPayload {
    ServerRequestPayload::CommandExecutionRequestApproval(CommandExecutionRequestApprovalParams {
        kind: CommandExecutionApprovalKind::HumanCommand,
        thread_id: thread.to_string(),
        turn_id: "turn-1".into(),
        item_id: "call-1".into(),
        started_at_ms: 0,
        approval_id: None,
        environment_id: None,
        reason: None,
        command: Some("echo control".into()),
        cwd: None,
        command_actions: None,
        additional_permissions: None,
        proposed_execpolicy_amendment: None,
        proposed_network_policy_amendments: None,
        network_approval_context: None,
        available_decisions: None,
    })
}
fn dynamic(thread: ThreadId) -> ServerRequestPayload {
    ServerRequestPayload::DynamicToolCall(DynamicToolCallParams {
        thread_id: thread.to_string(),
        turn_id: "turn-1".into(),
        call_id: "call-2".into(),
        namespace: None,
        tool: "control".into(),
        arguments: json!({}),
    })
}
fn verification(thread: ThreadId) -> ServerRequestPayload {
    ServerRequestPayload::McpServerElicitationRequest(McpServerElicitationRequestParams {
        thread_id: thread.to_string(),
        turn_id: Some("turn-1".into()),
        server_name: "control".into(),
        request: McpServerElicitationRequest::UserVerification {
            meta: None,
            title: "Control".into(),
            description: String::new(),
            challenge: "AQID".into(),
        },
    })
}
fn pending<F: Future>(mut future: Pin<&mut F>) {
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
}
async fn bounded<F: Future>(future: F) -> F::Output {
    match tokio::time::timeout(Duration::from_secs(5), future).await {
        Ok(value) => value,
        Err(_) => panic!("retained approval wait hang detector"),
    }
}

#[tokio::test]
async fn gap_retains_same_request_until_real_eligible_replay_and_response() {
    let mut h = retained();
    h.outgoing
        .enable_human_approval_connection(ConnectionId(1))
        .await;
    h.outgoing.connection_closed(ConnectionId(1)).await;
    let (id, mut response) = scoped(&h).send_request(human(h.thread)).await;
    assert!(h.messages.try_recv().is_err());
    assert!(matches!(
        response.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(
        h.outgoing.pending_requests_for_thread(h.thread).await.len(),
        1
    );

    // Neither an ineligible replay nor a reply from a never-delivered recipient
    // may answer this wait. The existing replay operation supplies eligibility.
    assert!(
        h.outgoing
            .replay_requests_to_connection_for_thread(ConnectionId(2), h.thread)
            .await
            .is_ok()
    );
    assert!(h.messages.try_recv().is_err());
    h.outgoing
        .enable_human_approval_connection(ConnectionId(2))
        .await;
    h.outgoing
        .notify_client_response(ConnectionId(2), id.clone(), json!({"decision":"accept"}))
        .await;
    h.outgoing
        .notify_client_error(
            ConnectionId(2),
            id.clone(),
            internal_error("not delivered to this connection"),
        )
        .await;
    assert!(matches!(
        response.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(
        h.outgoing.pending_requests_for_thread(h.thread).await,
        vec![human(h.thread).request_with_id(id.clone())]
    );
    assert!(
        h.outgoing
            .replay_requests_to_connection_for_thread(ConnectionId(2), h.thread)
            .await
            .is_ok()
    );
    let Some(OutgoingEnvelope::ToConnection {
        connection_id: ConnectionId(2),
        message: OutgoingMessage::Request(request),
        ..
    }) = bounded(h.messages.recv()).await
    else {
        panic!("missing eligible replay")
    };
    assert_eq!(request, human(h.thread).request_with_id(id.clone()));
    let result = json!({"decision":"accept"});
    h.outgoing
        .notify_client_error(
            ConnectionId(1),
            id.clone(),
            internal_error("retired recipient"),
        )
        .await;
    assert!(matches!(
        response.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(
        h.outgoing.pending_requests_for_thread(h.thread).await,
        vec![human(h.thread).request_with_id(id.clone())]
    );
    h.outgoing
        .notify_client_response(ConnectionId(1), id.clone(), result.clone())
        .await;
    assert!(matches!(
        response.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    h.outgoing
        .notify_client_response(ConnectionId(2), id.clone(), result.clone())
        .await;
    assert_eq!(bounded(response).await.unwrap(), Ok(result.clone()));
    h.outgoing
        .notify_client_response(ConnectionId(2), id, result)
        .await;
    assert!(
        h.outgoing
            .pending_requests_for_thread(h.thread)
            .await
            .is_empty()
    );
    assert!(
        h.outgoing
            .replay_requests_to_connection_for_thread(ConnectionId(2), h.thread)
            .await
            .is_ok()
    );
    while let Ok(queued) = h.messages.try_recv() {
        assert!(!matches!(
            queued,
            OutgoingEnvelope::ToConnection {
                message: OutgoingMessage::Request(_),
                ..
            } | OutgoingEnvelope::Broadcast {
                message: OutgoingMessage::Request(_)
            }
        ));
    }
}

#[tokio::test]
async fn ordinary_and_nonthread_human_gaps_keep_existing_failure_behavior() {
    let (send, mut messages) = mpsc::channel(8);
    let ordinary = Arc::new(OutgoingMessageSender::new(
        send,
        AnalyticsEventsClient::disabled(),
    ));
    let thread = ThreadId::new();
    let (_, response) =
        ThreadScopedOutgoingMessageSender::new(ordinary.clone(), Vec::new(), thread)
            .send_request(human(thread))
            .await;
    assert!(bounded(response).await.is_err());
    assert!(
        ordinary
            .pending_requests_for_thread(thread)
            .await
            .is_empty()
    );
    assert!(messages.try_recv().is_err());

    let mut h = retained();
    let (_, response) = h.outgoing.send_request(human(h.thread)).await;
    assert!(bounded(response).await.is_err());
    assert!(
        h.outgoing
            .pending_requests_for_thread(h.thread)
            .await
            .is_empty()
    );
    assert!(h.messages.try_recv().is_err());
}

#[tokio::test]
async fn retained_input_never_retains_or_replays_ownerless_verification() {
    let mut h = retained();
    let (_, response) = scoped(&h).send_request(verification(h.thread)).await;
    assert!(bounded(response).await.is_err());
    assert!(
        h.outgoing
            .pending_requests_for_thread(h.thread)
            .await
            .is_empty()
    );
    assert!(h.messages.try_recv().is_err());

    h.outgoing
        .enable_user_verification_connection(ConnectionId(1))
        .await;
    let sender =
        ThreadScopedOutgoingMessageSender::new(h.outgoing.clone(), vec![ConnectionId(1)], h.thread);
    let (_, response) = sender.send_request(verification(h.thread)).await;
    bounded(h.messages.recv()).await.unwrap();
    h.outgoing.connection_closed(ConnectionId(1)).await;
    assert!(bounded(response).await.is_err());
    h.outgoing
        .enable_user_verification_connection(ConnectionId(2))
        .await;
    assert!(
        h.outgoing
            .replay_requests_to_connection_for_thread(ConnectionId(2), h.thread)
            .await
            .is_ok()
    );
    assert!(h.messages.try_recv().is_err());
    assert!(h.outgoing.request_id_to_callback.lock().await.is_empty());
    assert!(!h.incoming.is_stopped());
}

#[tokio::test]
async fn actual_owner_stop_while_registration_waits_leaves_no_callback() {
    let h = retained();
    let sender = scoped(&h);
    let callbacks = h.outgoing.request_id_to_callback.lock().await;
    let mut registration = Box::pin(sender.send_request(human(h.thread)));
    pending(registration.as_mut());
    h.incoming.stop();
    drop(callbacks);
    let (_, response) = bounded(registration).await;
    assert!(bounded(response).await.is_err());
    assert!(h.outgoing.request_id_to_callback.lock().await.is_empty());
}

#[tokio::test]
async fn cancellation_before_queued_registration_is_terminal_even_before_input_stop() {
    let h = retained();
    let sender = scoped(&h);
    let callbacks = h.outgoing.request_id_to_callback.lock().await;
    let mut cancellation = Box::pin(h.outgoing.cancel_all_requests(None));
    pending(cancellation.as_mut());
    let mut registration = Box::pin(sender.send_request(human(h.thread)));
    pending(registration.as_mut());
    // Tokio's callback mutex queues cancellation first. The actual input stays
    // open to prove the sticky callback close, rather than an ended-state check.
    drop(callbacks);
    let ((), (_, response)) = bounded(async { tokio::join!(cancellation, registration) }).await;
    assert!(!h.incoming.is_stopped());
    assert!(bounded(response).await.is_err());
    let (_, late) = sender.send_request(dynamic(h.thread)).await;
    assert!(bounded(late).await.is_err());
    assert!(h.outgoing.request_id_to_callback.lock().await.is_empty());
}

#[tokio::test]
async fn registration_before_host_stop_is_released_by_the_same_drain() {
    let h = retained();
    let (_, response) = scoped(&h).send_request(human(h.thread)).await;
    assert_eq!(
        h.outgoing.pending_requests_for_thread(h.thread).await.len(),
        1
    );
    let error = internal_error("retained runtime is shutting down");
    // This is the existing stop_admission order, using the actual input object.
    h.incoming.stop();
    h.outgoing.cancel_all_requests(Some(error.clone())).await;
    assert_eq!(bounded(response).await.unwrap(), Err(error));
    let (_, late) = scoped(&h).send_request(human(h.thread)).await;
    assert!(bounded(late).await.is_err());
    assert!(h.outgoing.request_id_to_callback.lock().await.is_empty());
}

#[tokio::test]
async fn ordinary_cancel_all_still_allows_later_requests() {
    let (send, _messages) = mpsc::channel(8);
    let outgoing = Arc::new(OutgoingMessageSender::new(
        send,
        AnalyticsEventsClient::disabled(),
    ));
    outgoing.cancel_all_requests(None).await;
    let thread = ThreadId::new();
    let sender = ThreadScopedOutgoingMessageSender::new(outgoing.clone(), Vec::new(), thread);
    let (id, response) = sender.send_request(dynamic(thread)).await;
    let result = json!({"success":true,"contentItems":[]});
    outgoing
        .notify_client_response(ConnectionId(7), id, result.clone())
        .await;
    assert_eq!(bounded(response).await.unwrap(), Ok(result));
}

#[tokio::test]
async fn distinct_incoming_owners_do_not_share_withdrawal() {
    let a = retained();
    let b = retained();
    assert_ne!(a.incoming, b.incoming);
    a.incoming.stop();
    let (_, rejected) = scoped(&a).send_request(human(a.thread)).await;
    assert!(bounded(rejected).await.is_err());
    let (_, mut waiting) = scoped(&b).send_request(human(b.thread)).await;
    assert!(matches!(
        waiting.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    b.incoming.stop();
    b.outgoing.cancel_all_requests(None).await;
    assert!(bounded(waiting).await.is_err());
}
