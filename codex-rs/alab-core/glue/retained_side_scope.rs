// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One original Side injection on SAME accepted construction Scope/Core.
//! Claim/occurrence bytes select DATA only. Actual A and the closed Source
//! transition admit work; accepted worker custody survives observer EOF.
use super::*;
use crate::outgoing_message::alab_side_boundary_claim::Claim;
use crate::outgoing_message::alab_side_boundary_claim::metadata;
use crate::request_processors::turn_processor::TurnRequestProcessor;
use crate::request_processors::turn_processor::alab_retained_side::OriginalInjection;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::ThreadInjectItemsParams;
use codex_app_server_protocol::ThreadInjectItemsResponse;
use codex_core::PreparedSideBoundary;
use codex_core::RecordedSideBoundary;
use tokio::sync::watch;

#[derive(Default)]
struct Progress {
    complete: Option<Result<metadata::Completion, JSONRPCErrorError>>,
    recorded: Option<RecordedSideBoundary>,
    observers: Vec<RequestContext>,
    unknown: bool,
}
#[derive(Default)]
pub(super) struct SideSlot {
    actual: Option<Arc<SideInjection>>,
    pub(super) projected: bool,
    pub(super) retiring: bool,
}
pub(super) struct SideInjection {
    scope: Weak<Scope>,
    input: Incoming,
    thread: Arc<CodexThread>,
    prepared: PreparedSideBoundary,
    parameters: serde_json::Value,
    arguments: ThreadInjectItemsParams,
    claim: String,
    progress: Mutex<Progress>,
    changed: watch::Sender<()>,
}
struct Worker {
    actual: Arc<SideInjection>,
    finished: bool,
    _tracked: tokio_util::task::task_tracker::TaskTrackerToken,
}
impl Drop for Worker {
    fn drop(&mut self) {
        if !self.finished {
            let mut p = self
                .actual
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            p.unknown = true;
            drop(p);
            if let Some(scope) = self.actual.scope.upgrade() {
                scope
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .selection_unknown = true;
            }
            self.actual.input.fail(std::io::Error::other(
                "accepted Side injection outcome unobserved",
            ));
            self.actual.changed.send_replace(());
        }
    }
}
impl InitialSlot {
    pub(crate) async fn inject_side(
        &self,
        context: RequestContext,
        arguments: ThreadInjectItemsParams,
        original: TurnRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        let operation = self
            .0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid_request("actual Side construction owner unavailable"))?;
        operation.inject_side(context, arguments, original).await
    }
}
impl InitialSessionOperation {
    async fn inject_side(
        self: &Arc<Self>,
        context: RequestContext,
        arguments: ThreadInjectItemsParams,
        original: TurnRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        let Claim {
            selection,
            parameters,
        } = context
            .alab_side_injection
            .as_deref()
            .cloned()
            .ok_or_else(|| invalid_request("original Side selection missing"))?;
        let origin =
            context
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| invalid_request("Side injection needs actual received Native A"))?;
        self.observe_bound_side_origin(&origin)
            .await
            .map_err(|_| invalid_request("Side injection Native origin is not current"))?;
        let candidate = self.source_candidate(&selection.claim).await?;
        if candidate.thread_id().to_string() != arguments.thread_id
            || candidate.is_primary_presentation()
        {
            return Err(invalid_request(
                "Side injection changed actual candidate or presentation",
            ));
        }
        let held = candidate.source_selection()?;
        held.recheck_held()
            .map_err(|_| invalid_request("actual Side Source inhibitor unavailable"))?;
        candidate.recheck().await?;
        // This fresh observer is current before admission/replay. Once admitted,
        // the owned worker never re-requires C1 or retries effects on C2.
        self.observe_bound_side_origin(&origin)
            .await
            .map_err(|_| invalid_request("Side injection observer withdrew before acceptance"))?;
        let scope = candidate.side_scope();
        let (actual, worker, cached) = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed || self.input.is_stopped() {
                return Err(invalid_request("Side injection admission closed"));
            }
            let registered = state
                .accepted
                .iter()
                .chain(state.clients.values())
                .any(|x| Arc::ptr_eq(x, &scope));
            if !registered {
                return Err(invalid_request("actual Side construction retired"));
            }
            let mut slot = scope
                .side_boundary
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(actual) = slot.actual.as_ref() {
                if actual.claim != selection.claim
                    || actual.arguments != arguments
                    || actual.parameters != parameters
                    || !Arc::ptr_eq(&actual.thread, candidate.thread())
                {
                    return Err(invalid_request(
                        "Side injection replay changed original full arguments",
                    ));
                }
                let mut p = actual
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if p.unknown {
                    return Err(invalid_request("Side injection outcome remains unobserved"));
                }
                let cached = p.complete.clone();
                if cached.is_none() {
                    let id = context.alab_request_id();
                    if !p.observers.iter().any(|old| old.alab_request_id() == id) {
                        if p.observers.len() >= MAX_PENDING_REPLAYS {
                            return Err(invalid_request(
                                "Side replay observer capacity unavailable",
                            ));
                        }
                        p.observers.push(context.clone());
                    }
                }
                (actual.clone(), None, cached)
            } else {
                if slot.projected {
                    return Err(invalid_request(
                        "Side projection already froze this boundary",
                    ));
                }
                let prepared = candidate
                    .thread()
                    .prepare_retained_side_boundary()
                    .map_err(|error| invalid_request(error.to_string()))?;
                let actual = Arc::new(SideInjection {
                    scope: Arc::downgrade(&scope),
                    input: self.input.clone(),
                    thread: candidate.thread().clone(),
                    prepared,
                    parameters,
                    arguments,
                    claim: selection.claim,
                    progress: Mutex::new(Progress {
                        observers: vec![context.clone()],
                        ..Default::default()
                    }),
                    changed: watch::channel(()).0,
                });
                let worker = Worker {
                    actual: actual.clone(),
                    finished: false,
                    _tracked: self.tracker.token(),
                };
                slot.actual = Some(actual.clone());
                (actual, Some(worker), None)
            }
        };
        if let Some(cached) = cached {
            actual
                .publish(&self.processor.outgoing, &context, &cached)
                .await;
        }
        if let Some(mut worker) = worker {
            let operation = self.clone();
            let candidate = candidate.clone();
            Box::pin(async move {
                let actual=worker.actual.clone();
                let result=AssertUnwindSafe(async {
                    held.recheck_held().map_err(|_|invalid_request("Side inhibitor changed before original injection"))?;
                    candidate.recheck().await?;
                    let invocation=OriginalInjection::new(actual.thread.clone(),actual.prepared.clone());
                    let output=invocation.run(original.thread_inject_items(context.alab_request_id(),actual.arguments.clone())).await?;
                    if output.is_none(){return Err(internal_error("original Side injection response missing"))}
                    let recorded=tokio::select! {
                        result=actual.prepared.recorded(&actual.thread)=>result.map_err(|e|internal_error(e.to_string()))?,
                        ()=actual.input.withdrawn()=>return Err(internal_error("Side recording unobserved during owner shutdown")),
                        ()=actual.thread.wait_until_terminated()=>return Err(internal_error("Side Core terminated before original recording observation")),
                    };
                    let completion=metadata::Completion {schema:1,claim:actual.claim.clone(),thread_id:actual.arguments.thread_id.clone(),
                        occurrence:recorded.marker_data().token_data().to_owned()};
                    if !completion.valid(){return Err(internal_error("actual Side recording completion invalid"))}
                    let mut p=actual.progress.lock().unwrap_or_else(PoisonError::into_inner);
                    p.recorded=Some(recorded);
                    Ok(completion)
                }).catch_unwind().await;
                let Ok(result)=result else {return};
                // Original validation/error is cached exactly. An injection
                // that started but lost its recording is conservatively Unknown.
                if actual.input.is_stopped() && result.is_err(){return}
                let observers={
                    let mut p=actual.progress.lock().unwrap_or_else(PoisonError::into_inner);
                    p.complete=Some(result.clone());
                    std::mem::take(&mut p.observers)
                };
                actual.changed.send_replace(());
                worker.finished=true;
                for observer in observers {actual.publish(&operation.processor.outgoing,&observer,&result).await;}
            }).await;
        }
        Ok(())
    }
}
impl SideInjection {
    async fn publish(
        &self,
        outgoing: &crate::outgoing_message::OutgoingMessageSender,
        context: &RequestContext,
        result: &Result<metadata::Completion, JSONRPCErrorError>,
    ) {
        let id = context.alab_request_id().clone();
        match result {
            Ok(completion) => {
                let payload: ClientResponsePayload = ThreadInjectItemsResponse {}.into();
                let value = serde_json::json!({"schema":completion.schema,"claim":completion.claim,
                    "threadId":completion.thread_id,"occurrence":completion.occurrence});
                match codex_app_server_protocol::alab_response_metadata::decorate(
                    payload,
                    metadata::RESULT_KEY,
                    value,
                ) {
                    Ok(payload) => outgoing.send_response_as(id, payload).await,
                    Err(error) => {
                        outgoing
                            .send_error(id, internal_error(error.to_string()))
                            .await
                    }
                }
            }
            Err(error) => outgoing.send_error(id, error.clone()).await,
        }
    }
    pub(super) fn recorded(
        &self,
        actual: &Arc<CodexThread>,
    ) -> Result<RecordedSideBoundary, JSONRPCErrorError> {
        let p = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if p.unknown || !matches!(p.complete, Some(Ok(_))) || !Arc::ptr_eq(actual, &self.thread) {
            return Err(invalid_request("actual Side boundary has not completed"));
        }
        let recorded = p
            .recorded
            .clone()
            .ok_or_else(|| invalid_request("actual canonical Side occurrence missing"))?;
        recorded
            .check_owner(actual)
            .map_err(|e| invalid_request(e.to_string()))?;
        Ok(recorded)
    }
}

impl selected_primary::PrimaryCandidate {
    // Resource comes from the SAME accepted record; never a new slot/manager.
    fn side_scope(&self) -> Arc<Scope> {
        self.scope_for_side()
    }
}
impl Scope {
    pub(super) fn recorded_side_boundary(
        &self,
        actual: &Arc<CodexThread>,
    ) -> Result<Option<RecordedSideBoundary>, JSONRPCErrorError> {
        if self.frozen.retains_candidate() || !matches!(self.arguments, Arguments::Fork(_)) {
            return Ok(None);
        }
        let mut slot = self
            .side_boundary
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.retiring {
            return Err(invalid_request(
                "actual Side candidate is in original cleanup",
            ));
        }
        let recorded = slot
            .actual
            .as_ref()
            .ok_or_else(|| invalid_request("original Side injection has not been accepted"))?
            .recorded(actual)?;
        slot.projected = true;
        Ok(Some(recorded))
    }
}

impl InitialSessionOperation {
    /// Warm Side reconciliation may reuse only the retained original local
    /// occurrence on this EXACT advancing Core. New/cold incarnations never
    /// recover a private receipt from marker bytes, a path or a logical ID.
    pub(super) fn retained_side_boundary(
        &self,
        actual: &Arc<CodexThread>,
    ) -> Result<Option<RecordedSideBoundary>, JSONRPCErrorError> {
        let scopes = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .accepted
                .iter()
                .chain(state.clients.values())
                .cloned()
                .collect::<Vec<_>>()
        };
        for scope in scopes {
            let same = {
                let p = scope
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                p.worker_finished
                    && !p.worker_lost
                    && !p.selection_unknown
                    && p.selection_retired
                    && p.created
                        .as_ref()
                        .is_some_and(|(_, born)| Arc::ptr_eq(born, actual))
            };
            if same && let Some(recorded) = scope.recorded_side_boundary(actual)? {
                return Ok(Some(recorded));
            }
        }
        Ok(None)
    }
}

impl InitialSessionOperation {
    /// Current-A query through THIS actual BoundBackend's controller/origin
    /// ledger. The existing SDK observer returns correlation DATA, never a
    /// credential-reader/model/Go grant. Accepted workers do not call it again.
    pub(super) async fn observe_bound_side_origin(
        &self,
        origin: &AcceptedOrigin,
    ) -> Result<(), JSONRPCErrorError> {
        let backend = self.bound.as_ref().ok_or_else(|| {
            invalid_request("Side operation requires its actual bound backend owner")
        })?;
        let current = backend.observe_reader_origin(origin).await.map_err(|_| {
            invalid_request("Side origin does not belong to the current bound backend")
        })?;
        if !current.same_receipt(origin) {
            return Err(invalid_request("Side origin receipt changed"));
        }
        Ok(())
    }
}

// These wrappers live under the actual processor module and use its existing
// private weak association. No upstream field visibility or owner is widened.
impl ThreadRequestProcessor {
    pub(crate) async fn alab_retained_side_inject(
        &self,
        context: RequestContext,
        args: codex_app_server_protocol::ThreadInjectItemsParams,
        original: TurnRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        self.alab_initial_session
            .inject_side(context, args, original)
            .await
    }
    pub(crate) async fn alab_retained_side_close(
        &self,
        context: RequestContext,
        args: codex_app_server_protocol::ThreadUnsubscribeParams,
        session: Arc<crate::message_processor::ConnectionSessionState>,
    ) -> Result<(), JSONRPCErrorError> {
        self.alab_initial_session
            .close_side_view(context, args, session)
            .await
    }
    pub(crate) async fn alab_retained_side_rollback(
        &self,
        context: RequestContext,
        args: codex_app_server_protocol::ThreadDeleteParams,
    ) -> Result<(), JSONRPCErrorError> {
        self.alab_initial_session.rollback_side(context, args).await
    }
}
