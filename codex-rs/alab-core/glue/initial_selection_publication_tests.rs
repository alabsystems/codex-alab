// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower publication controls using real MessageProcessor request registration
//! and OutgoingEnvelope delivery. No Native origin/accept/Core call is faked.
//! The original body is proven unpolled at the tap and is aborted/joined before
//! the private publication Scope is constructed. These are not SDK admissions.
use super::*;
use crate::message_processor::ConnectionSessionState;
use crate::message_processor::alab_task_mcp::tests::Harness;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingEnvelope;
use crate::outgoing_message::OutgoingMessage;
use crate::outgoing_message::alab_initial_claim::alab_publication_codec::take_offer;
use crate::outgoing_message::alab_selection_request_capture::Capture;
use crate::outgoing_message::alab_selection_request_capture::remains_registered;
use crate::transport::AppServerTransport;
use crate::transport::ConnectionOrigin;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_app_server_protocol::ConfigWarningNotification;
use codex_app_server_protocol::InitializeResponse;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::ServerNotification;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;
use wiremock::MockServer;

const CLAIM: &str = "0123456789abcdef0123456789abcdef";
const MARKER: &str = "initial-selection-publication-fifo-end";

async fn dispatched_context(
    h: &Harness,
    connection: ConnectionId,
    session: &Arc<ConnectionSessionState>,
    request_id: i64,
) -> Result<(
    RequestContext,
    Arc<OutgoingMessageSender>,
    Arc<ThreadRequestProcessor>,
)> {
    dispatched_context_for(h, connection, session, request_id, h.parent_id).await
}

pub(in crate::request_processors::thread_processor::alab_initial_session) async fn dispatched_context_for(
    h: &Harness,
    connection: ConnectionId,
    session: &Arc<ConnectionSessionState>,
    request_id: i64,
    actual_target: ThreadId,
) -> Result<(
    RequestContext,
    Arc<OutgoingMessageSender>,
    Arc<ThreadRequestProcessor>,
)> {
    let (processor, outgoing) = h.processor.alab_selection_publication_parts_for_tests();
    let mut capture = Capture::arm(&outgoing)?;
    let request: JSONRPCRequest = serde_json::from_value(json!({
        "id":request_id,"method":"thread/resume","params":{
            "threadId":actual_target.to_string(),"excludeTurns":true,
            "_meta":{"foreign.original":{"kept":[1,2]},"alab.initialSession":{
                "claim":CLAIM,"prospectiveCreate":ThreadStartParams::default()
            }}
        }
    }))?;
    let actual = h.processor.clone();
    let session = session.clone();
    let task = tokio::spawn(async move {
        actual
            .process_request(
                connection,
                request,
                &AppServerTransport::WebSocket {
                    bind_address: "127.0.0.1:0"
                        .parse()
                        .unwrap_or_else(|_| unreachable!("fixed test address")),
                },
                session,
            )
            .await;
    });
    let captured = tokio::time::timeout(Duration::from_secs(10), capture.next()).await;
    // The tap is inside registration and before request_fut's first poll, so
    // this cancellation cannot destroy a partially executing Core operation.
    task.abort();
    let stopped = task.await;
    let context = captured.context("actual dispatch did not register")??;
    ensure!(
        stopped.is_err_and(|error| error.is_cancelled()),
        "original body escaped its actual registration tap"
    );
    ensure!(
        context.alab_initial_claim.is_some(),
        "actual parser did not retain original selection"
    );
    ensure!(
        context.alab_connection_resource.is_none(),
        "fixture must not invent Native origin"
    );
    ensure!(remains_registered(&outgoing, context.alab_request_id()).await);
    Ok((context, outgoing, Arc::new(processor)))
}

pub(in crate::request_processors::thread_processor::alab_initial_session) fn publication_fixture(
    h: &Harness,
    processor: Arc<ThreadRequestProcessor>,
    context: RequestContext,
    session: &ConnectionSessionState,
) -> Result<(Arc<InitialSessionOperation>, Arc<Scope>)> {
    let claim = context
        .alab_initial_claim
        .clone()
        .context("actual selected context missing")?;
    ensure!(claim.method.as_ref() == "thread/resume");
    let params = serde_json::from_value(claim.arguments.as_ref().clone())?;
    let operation = Arc::new(InitialSessionOperation {
        claim: claim.selection.claim.clone(),
        kind: Kind::Selection,
        binding: None,
        input: h.input.clone(),
        bound: None,
        processor: processor.clone(),
        tracker: TaskTracker::new(),
        selected_primary: Mutex::new(selected_primary::SelectionState::new()),
        side_views: Mutex::new(side_views::Views::default()),
        selection_changed: tokio::sync::Notify::new(),
        state: Mutex::new(State {
            closed: false,
            accepted: None,
            next_claim: None,
            clients: HashMap::new(),
            retired_saved_resumes: saved_resume_receipts::Receipts::default(),
            preparations: HashMap::new(),
        }),
    });
    let scope = Arc::new(Scope {
        candidate: InitialSessionPlan::fresh_claim(),
        registered_selection: None,
        prepared_offer: None,
        preparation_error: None,
        side_boundary: Mutex::new(side_boundary::SideSlot::default()),
        side_closings: Mutex::new(side_view_close::Closings::default()),
        side_rollback: Mutex::new(None),
        prior_selection: operation
            .snapshot_selection()
            .map_err(|error| anyhow::anyhow!("{}", error.code))?,
        completed: tokio::sync::watch::channel(()).0,
        operation: Arc::downgrade(&operation),
        input: h.input.clone(),
        processor,
        binding: None,
        context,
        frozen: frozen::FrozenRequest::new(claim),
        arguments: Arguments::Resume(params),
        metadata: ClientMetadata {
            name: session.app_server_client_name().map(str::to_owned),
            version: session.client_version().map(str::to_owned),
            extensions: session.client_mcp_extensions(),
        },
        progress: Arc::new(Mutex::new(Progress {
            entered: false,
            created: None,
            resolved: None,
            loaded_task_tools: false,
            next_selection: None,
            staged: None,
            complete: None,
            worker_lost: false,
            worker_finished: false,
            held_selection: None,
            selection_retired: false,
            selection_unknown: false,
            history_fallback: false,
            dynamic_tools_fallback: false,
            task_binding: None,
            primary: None,
            primary_weak: None,
            replays: Vec::new(),
        })),
    });
    operation
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .accepted = Some(scope.clone());
    // This inserts ONLY private publication state. InitialSessionOperation::accept
    // is deliberately never called or claimed; no actual Native role is present.
    Ok((operation, scope))
}
fn original_error(data: Option<Value>) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: -32600,
        message: "original selected policy error".into(),
        data,
    }
}
async fn terminal_errors(
    messages: &mut mpsc::UnboundedReceiver<OutgoingEnvelope>,
    outgoing: &OutgoingMessageSender,
    request: &ConnectionRequestId,
) -> Result<Vec<JSONRPCErrorError>> {
    // Same original sender FIFO, not an empty-queue/time-slice inference.
    outgoing
        .send_server_notification_to_connections(
            &[request.connection_id],
            ServerNotification::ConfigWarning(ConfigWarningNotification {
                summary: MARKER.into(),
                details: None,
                path: None,
                range: None,
            }),
        )
        .await;
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut errors = Vec::new();
        loop {
            let envelope = messages.recv().await.context("actual outbox closed")?;
            let OutgoingEnvelope::ToConnection { connection_id, message, .. } = envelope else { continue; };
            if connection_id != request.connection_id { continue; }
            match message {
                OutgoingMessage::Error(error) if error.id == request.request_id => errors.push(error.error),
                OutgoingMessage::Response(response) if response.id == request.request_id => anyhow::bail!("unexpected success response"),
                OutgoingMessage::AppServerNotification(envelope)
                    if matches!(&envelope.notification, ServerNotification::ConfigWarning(warning) if warning.summary == MARKER) => return Ok(errors),
                _ => {},
            }
        }
    }).await.context("actual publication FIFO marker absent")?
}
async fn initialized_c2(h: &mut Harness) -> Result<Arc<ConnectionSessionState>> {
    let c2 = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
    let _: InitializeResponse = h
        .request(
            ConnectionId(42),
            &c2,
            json!({"id":8100,"method":"initialize","params":{
                "clientInfo":{"name":"selection-publication-c2","version":"1"},
                "capabilities":{"experimentalApi":true,"requiredHumanApproval":true}
            }}),
        )
        .await?;
    Ok(c2)
}

#[tokio::test]
async fn selected_resume_stages_one_lossless_error_then_publishes_exactly_once() -> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let result: Result<()> = async {
        for (index, data) in [
            None,
            Some(Value::Null),
            Some(json!({"_meta":false,"alab.initialSessionNext":{"foreign":"kept"}})),
        ]
        .into_iter()
        .enumerate()
        {
            let (context, outgoing, processor) = dispatched_context(
                &h,
                ConnectionId(41),
                &h.c1,
                8200 + i64::try_from(index).context("bounded test request index")?,
            )
            .await?;
            let request = context.alab_request_id().clone();
            let (operation, scope) = publication_fixture(&h, processor, context, &h.c1)?;
            let original = original_error(data);
            let publication =
                super::super::alab_initial_resume::ResumeClientTarget::retained(scope.clone())
                    .publication();
            super::super::alab_initial_resume::send_error(
                publication,
                &outgoing,
                request.clone(),
                original.clone(),
            )
            .await;
            ensure!(
                remains_registered(&outgoing, &request).await,
                "accepted stage sent a premature terminal error"
            );
            let staged = scope
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .staged
                .clone()
                .context("error not staged")?;
            scope.finish(staged).await;
            let errors = terminal_errors(&mut h.messages, &outgoing, &request).await?;
            ensure!(
                errors.len() == 1,
                "one accepted publication must produce exactly one terminal Error"
            );
            ensure!(!remains_registered(&outgoing, &request).await);
            let mut restored = errors
                .into_iter()
                .next()
                .context("terminal Error missing")?;
            let offer =
                take_offer(&mut restored, CLAIM)?.context("next selection not delivered")?;
            ensure!(offer.previous_claim == CLAIM);
            ensure!(serde_json::to_value(restored)? == serde_json::to_value(original)?);
            ensure!(
                operation
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .next_claim
                    .as_deref()
                    == Some(offer.claim.as_str())
            );
            drop((operation, scope));
        }
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn real_c1_close_preserves_the_same_cached_offer_for_another_registered_context() -> Result<()>
{
    let model = MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let mut c2 = None;
    let result: Result<()> = async {
        let (context, outgoing, processor) =
            dispatched_context(&h, ConnectionId(41), &h.c1, 8301).await?;
        let original_request = context.alab_request_id().clone();
        let (operation, scope) = publication_fixture(&h, processor, context, &h.c1)?;
        h.processor.connection_closed(ConnectionId(41), &h.c1).await;
        let original = original_error(Some(
            json!(["original",null,{"_meta":{"foreign.owner":true}}]),
        ));
        scope.finish(Reply::Error(original.clone())).await;
        let first = terminal_errors(&mut h.messages, &outgoing, &original_request).await?;
        ensure!(first.len() == 1);
        let cached = scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .complete
            .clone()
            .context("terminal cache missing")?;
        let next = initialized_c2(&mut h).await?;
        c2 = Some(next.clone());
        let (context2, _, _) = dispatched_context(&h, ConnectionId(42), &next, 8302).await?;
        ensure!(
            scope.frozen.matches(
                context2
                    .alab_initial_claim
                    .as_ref()
                    .context("C2 original claim missing")?
            )
        );
        let replay_request = context2.alab_request_id().clone();
        // Invoke ONLY the same lower terminal-publication helper. Native
        // replay admission is deliberately neither bypassed nor claimed.
        publish(&scope, &context2, &cached).await;
        let second = terminal_errors(&mut h.messages, &outgoing, &replay_request).await?;
        ensure!(second.len() == 1);
        ensure!(
            serde_json::to_value(&first[0])? == serde_json::to_value(&second[0])?,
            "same accepted error/offer must not be regenerated or nested"
        );
        let mut restored = second[0].clone();
        let offer =
            take_offer(&mut restored, CLAIM)?.context("cached offer missing after C1 close")?;
        ensure!(serde_json::to_value(restored)? == serde_json::to_value(original)?);
        ensure!(
            operation
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .next_claim
                .as_deref()
                == Some(offer.claim.as_str())
        );
        drop((operation, scope));
        Ok(())
    }
    .await;
    if let Some(c2) = c2 {
        h.processor.connection_closed(ConnectionId(42), &c2).await;
    }
    let cleanup = h.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn unknown_core_entry_or_real_resolved_capture_never_issues_a_selection_offer() -> Result<()>
{
    let model = MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let result: Result<()> = async {
        for case in 0..3 {
            let (context, outgoing, processor) =
                dispatched_context(&h, ConnectionId(41), &h.c1, 8400 + case).await?;
            let request = context.alab_request_id().clone();
            let (operation, scope) = publication_fixture(&h, processor, context, &h.c1)?;
            let reply = match case {
                0 => Reply::Unknown,
                1 => {
                    scope.before_core().map_err(|error| {
                        anyhow::anyhow!("actual pre-Core hook rejected with code {}", error.code)
                    })?;
                    Reply::Error(original_error(None))
                }
                _ => {
                    scope.capture_resume_loaded(h.parent_id, &h.parent);
                    Reply::Error(original_error(None))
                }
            };
            scope.finish(reply).await;
            let errors = terminal_errors(&mut h.messages, &outgoing, &request).await?;
            ensure!(errors.len() == 1);
            let mut observed = errors[0].clone();
            ensure!(take_offer(&mut observed, CLAIM)?.is_none());
            ensure!(
                operation
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .next_claim
                    .is_none()
            );
            drop((operation, scope));
        }
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}
