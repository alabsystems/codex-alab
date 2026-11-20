// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Accepted original unsubscribe and pure view-lease retirement survive C1
//! EOF. Cache is an actual owned operation, not a completed-ID tombstone.
use super::*;
use crate::outgoing_message::alab_side_view_claim::Claim;
use crate::outgoing_message::alab_side_view_claim::metadata;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
#[derive(Default)]
pub(super) struct Closings {
    jobs: HashMap<String, Arc<Closing>>,
}
struct Closing {
    actual: Arc<CodexThread>,
    target: CloseTarget,
    parameters: serde_json::Value,
    args: ThreadUnsubscribeParams,
    progress: Mutex<Progress>,
}
#[derive(Default)]
struct Progress {
    joined: bool,
    result: Option<Result<ThreadUnsubscribeResponse, JSONRPCErrorError>>,
    observers: Vec<RequestContext>,
    unknown: bool,
    observed: bool,
}
struct Guard {
    actual: Arc<Closing>,
    scope: Arc<Scope>,
    finished: bool,
    _tracked: tokio_util::task::task_tracker::TaskTrackerToken,
}
impl Guard {
    fn finish(&mut self) {
        self.finished = true;
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.finished {
            self.actual
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .unknown = true;
            self.scope.input.fail(std::io::Error::other(
                "accepted Side close remains unobserved",
            ));
        }
    }
}
impl InitialSlot {
    fn side_close_owner(&self) -> Result<Arc<InitialSessionOperation>, JSONRPCErrorError> {
        self.0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid_request("actual Side view owner unavailable"))
    }

    pub(crate) async fn close_side_view(
        &self,
        context: RequestContext,
        args: ThreadUnsubscribeParams,
        session: Arc<crate::message_processor::ConnectionSessionState>,
    ) -> Result<(), JSONRPCErrorError> {
        let owner = self.side_close_owner()?;
        owner.close_side_view(context, args, session).await
    }
}

impl ThreadRequestProcessor {
    pub(crate) async fn alab_query_side_close(
        &self,
        context: &RequestContext,
    ) -> Result<(), JSONRPCErrorError> {
        let owner = self.alab_initial_session.side_close_owner()?;
        owner.check_side_close_origin(context).await?;
        let claim = context
            .alab_side_view
            .as_deref()
            .ok_or_else(|| invalid_request("Side close query missing"))?;
        match &claim.selection {
            metadata::Selection::ReadResult {
                view,
                operation,
                parameters,
            } => {
                let result = owner.read_side_close_result(view, operation, parameters)?;
                let payload = codex_app_server_protocol::alab_response_metadata::acknowledgment(
                    metadata::READ_RESULT_KEY,
                    serde_json::to_value(result)
                        .map_err(|error| internal_error(error.to_string()))?,
                )
                .map_err(|error| internal_error(error.to_string()))?;
                owner
                    .processor
                    .outgoing
                    .send_response_as(context.alab_request_id().clone(), payload)
                    .await;
                Ok(())
            }
            metadata::Selection::AcknowledgeResult {
                view, parameters, ..
            } => {
                let args = ThreadUnsubscribeParams {
                    thread_id: view.thread_id.clone(),
                };
                owner
                    .observe_side_close(context, &claim.selection, parameters, &args)
                    .await
            }
            _ => Err(invalid_request("Side close query phase unavailable")),
        }
    }
}

impl InitialSessionOperation {
    async fn check_side_close_origin(
        &self,
        context: &RequestContext,
    ) -> Result<(), JSONRPCErrorError> {
        let origin =
            context
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| invalid_request("Side close needs actual Native A"))?;
        self.observe_bound_side_origin(&origin)
            .await
            .map_err(|_| invalid_request("Side close Native origin is not current"))
    }

    fn read_side_close_result(
        &self,
        view: &metadata::SideView,
        operation: &str,
        parameters: &serde_json::Value,
    ) -> Result<metadata::ReadResult, JSONRPCErrorError> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed || self.input.is_stopped() {
            return Err(invalid_request("Side close result owner closed"));
        }
        let scope = state
            .clients
            .get(&view.claim)
            .ok_or_else(|| invalid_request("actual Side close result scope unavailable"))?;
        let closings = scope
            .side_closings
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let job = closings
            .jobs
            .get(operation)
            .ok_or_else(|| invalid_request("actual Side close result unavailable"))?;
        if &job.parameters != parameters || job.args.thread_id != view.thread_id {
            return Err(invalid_request(
                "Side close readback changed full original wire",
            ));
        }
        let progress = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if progress.unknown {
            return Err(invalid_request("Side close outcome remains unknown"));
        }
        let result = progress
            .result
            .as_ref()
            .ok_or_else(|| invalid_request("Side close result pending"))?;
        Ok(metadata::ReadResult::new(view, operation, result))
    }

    async fn close_side_view(
        self: &Arc<Self>,
        context: RequestContext,
        args: ThreadUnsubscribeParams,
        session: Arc<crate::message_processor::ConnectionSessionState>,
    ) -> Result<(), JSONRPCErrorError> {
        let Claim {
            selection,
            parameters,
        } = context
            .alab_side_view
            .as_deref()
            .cloned()
            .ok_or_else(|| invalid_request("Side view selection missing"))?;
        self.check_side_close_origin(&context).await?;
        if matches!(selection, metadata::Selection::Observe { .. }) {
            return self
                .observe_side_close(&context, &selection, &parameters, &args)
                .await;
        }
        if !matches!(selection, metadata::Selection::Close { .. }) {
            return Err(invalid_request("original Side close phase unavailable"));
        }
        let prepared = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed || self.input.is_stopped() {
                return Err(invalid_request("Side close admission closed"));
            }
            let scope = state
                .clients
                .get(&selection.view().claim)
                .cloned()
                .ok_or_else(|| invalid_request("actual Side scope unavailable"))?;
            let existing = scope
                .side_closings
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .jobs
                .contains_key(selection.operation());
            if existing {
                None
            } else {
                Some(self.prepare_side_close(&scope, &selection.view().thread_id)?)
            }
        };
        if let Some((_, CloseTarget::Candidate { held, .. })) = &prepared {
            held.recheck_held()
                .map_err(|_| invalid_request("actual candidate Source inhibitor unavailable"))?;
        }
        let mut retired = Vec::new(); // Declared outside the lock scope: even early errors drop owned jobs after locks.
        let (scope, actual, worker, cached) = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed || self.input.is_stopped() {
                return Err(invalid_request("Side close admission closed"));
            }
            let scope = state
                .clients
                .get(&selection.view().claim)
                .cloned()
                .ok_or_else(|| invalid_request("actual Side scope unavailable"))?;
            if scope.frozen.retains_candidate() {
                return Err(invalid_request("primary cannot retire as Side view"));
            }
            let mut closings = scope
                .side_closings
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let known_failed_close = closings.jobs.values().any(|job| {
                let p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                p.observed && p.joined && !p.unknown && matches!(p.result, Some(Err(_)))
            });
            if matches!(selection, metadata::Selection::Close { .. }) {
                let keys = closings
                    .jobs
                    .iter()
                    .filter_map(|(key, job)| {
                        let p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                        (p.observed && p.joined).then(|| key.clone())
                    })
                    .collect::<Vec<_>>();
                for key in keys {
                    if let Some(job) = closings.jobs.remove(&key) {
                        retired.push(job);
                    }
                }
            }
            if let Some(actual) = closings.jobs.get(selection.operation()).cloned() {
                if actual.parameters != parameters || actual.args != args {
                    return Err(invalid_request(
                        "Side close replay changed full original parameters",
                    ));
                }
                let mut p = actual
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if p.unknown {
                    return Err(invalid_request("Side close outcome remains unobserved"));
                }
                if matches!(selection, metadata::Selection::Observe { .. }) {
                    if p.result.is_none() {
                        return Err(invalid_request("Side close has no observed result"));
                    }
                    p.observed = true;
                }
                let cached = p.result.clone();
                if cached.is_none()
                    && !p
                        .observers
                        .iter()
                        .any(|old| old.alab_request_id() == context.alab_request_id())
                {
                    if p.observers.len() >= MAX_PENDING_REPLAYS {
                        return Err(invalid_request("Side close observer capacity unavailable"));
                    }
                    p.observers.push(context.clone());
                }
                drop(p);
                drop(closings);
                (scope, actual, None, cached)
            } else {
                if !matches!(selection, metadata::Selection::Close { .. }) {
                    return Err(invalid_request("actual Side close operation unavailable"));
                }
                if closings.jobs.len() >= 128 {
                    return Err(invalid_request("Side close operation custody full"));
                }
                let (actual, target) = prepared.ok_or_else(|| {
                    invalid_request("Side close resource changed before acceptance")
                })?;
                match &target {
                    CloseTarget::Installed => {
                        let current =
                            self.installed_side_core(&scope, &selection.view().thread_id)?;
                        if !Arc::ptr_eq(&current, &actual) {
                            return Err(invalid_request("installed Side close identity changed"));
                        }
                    }
                    CloseTarget::Candidate { .. } => {
                        let mut boundary = scope
                            .side_boundary
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        if boundary.projected
                            || !closings.jobs.is_empty()
                            || (boundary.retiring
                                && !known_failed_close
                                && !scope.side_cleanup_retryable())
                        {
                            return Err(invalid_request(
                                "Side candidate projection/cleanup already accepted",
                            ));
                        }
                        self.check_side_close_candidate(&scope, &actual)?;
                        boundary.retiring = true;
                    }
                }
                let actual = Arc::new(Closing {
                    actual,
                    target,
                    parameters,
                    args,
                    progress: Mutex::new(Progress {
                        observers: vec![context.clone()],
                        ..Default::default()
                    }),
                });
                let worker = Guard {
                    actual: actual.clone(),
                    scope: scope.clone(),
                    finished: false,
                    _tracked: self.tracker.token(),
                };
                closings
                    .jobs
                    .insert(selection.operation().into(), actual.clone());
                drop(closings);
                (scope, actual, Some(worker), None)
            }
        };
        drop(retired);
        if let Some(cached) = cached {
            actual
                .publish(&self.processor.outgoing, &context, &cached)
                .await;
        }
        if let Some(mut guard) = worker {
            let owner = self.clone();
            Box::pin(async move {
                let job = guard.actual.clone();
                let result = AssertUnwindSafe(super::super::alab_side_view_unsubscribe::run(
                    job.actual.clone(),
                    owner.processor.thread_unsubscribe_response_inner(
                        job.args.clone(),
                        context.alab_request_id().connection_id,
                    ),
                ))
                .catch_unwind()
                .await;
                let Ok(result) = result else { return };
                if result.is_ok()
                    && let Ok(thread) = ThreadId::from_string(&job.args.thread_id)
                {
                    // SAME original session resource. The ordinary MP path
                    // performs this after its inner unsubscribe and before
                    // publication; it must finish before our cache/release too.
                    session.mcp_event_streams.stop_thread(thread).await;
                }

                if result.is_ok() && matches!(job.target, CloseTarget::Installed) {
                    owner.release_side_view(&scope, &job.actual);
                }
                let observers = {
                    let mut p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                    p.result = Some(result.clone());
                    std::mem::take(&mut p.observers)
                };
                for observer in observers {
                    job.publish(&owner.processor.outgoing, &observer, &result)
                        .await;
                }
                if result.is_ok()
                    && let CloseTarget::Candidate { id, held } = &job.target
                {
                    // Explicit original discard of an uninstalled NEW child:
                    // drain ONLY that exact unused Core, as failed-new cleanup.
                    // Other installed Side/ordinary/task cores retain policy.
                    use crate::request_processors::thread_lifecycle::ThreadShutdownResult;
                    use crate::request_processors::thread_lifecycle::wait_for_thread_shutdown;
                    if !matches!(
                        wait_for_thread_shutdown(&job.actual).await,
                        ThreadShutdownResult::Complete
                    ) {
                        return;
                    }
                    let check = {
                        let owner = owner.clone();
                        let scope = scope.clone();
                        let job = job.clone();
                        let id = *id;
                        move || {
                            if !scope.prior_selection.unchanged(&owner) {
                                return Err(
                                    "prior primary changed during canceled Side cleanup".into()
                                );
                            }
                            let p = scope
                                .progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner);
                            let close = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                            if !p.worker_finished
                                || p.worker_lost
                                || p.selection_unknown
                                || p.selection_retired
                                || !p.entered
                                || p.resolved.is_some()
                                || !p.created.as_ref().is_some_and(|(born_id, born)| {
                                    *born_id == id && Arc::ptr_eq(born, &job.actual)
                                })
                                || job.actual.wait_until_terminated().now_or_never().is_none()
                                || close.unknown
                                || !matches!(close.result, Some(Ok(_)))
                            {
                                return Err(
                                    "original canceled NEW Side completion/termination unobserved"
                                        .into(),
                                );
                            }
                            Ok(())
                        }
                    };
                    if held.retire_after_new_core_terminated(check).await.is_err() {
                        return;
                    }
                    let lease = {
                        let mut p = scope
                            .progress
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        p.selection_retired = true;
                        p.primary.take()
                    };
                    drop(lease);
                }
                job.progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .joined = true;
                guard.finish();
                drop(guard);
            })
            .await;
        }
        Ok(())
    }
}
impl Closing {
    async fn publish(
        &self,
        outgoing: &crate::outgoing_message::OutgoingMessageSender,
        context: &RequestContext,
        result: &Result<ThreadUnsubscribeResponse, JSONRPCErrorError>,
    ) {
        let id = context.alab_request_id().clone();
        match result {
            Ok(reply) => outgoing.send_response_as(id, reply.clone().into()).await,
            Err(error) => outgoing.send_error(id, error.clone()).await,
        }
    }
}
impl Scope {
    pub(super) fn unobserved_side_close(&self) -> bool {
        self.side_closings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .jobs
            .values()
            .any(|job| {
                let p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                p.unknown || !p.joined || p.result.is_none() || !p.observed
            })
    }
}

impl Scope {
    pub(super) fn side_close_unknown(&self) -> bool {
        self.side_closings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .jobs
            .values()
            .any(|job| {
                let p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                p.unknown || !p.joined || p.result.is_none()
            })
    }
}

#[derive(Clone)]
enum CloseTarget {
    Installed,
    Candidate {
        id: ThreadId,
        held: Arc<codex_alab_aterm::frontend_control::retained_service::HeldSourceTransition>,
    },
}
impl InitialSessionOperation {
    fn prepare_side_close(
        &self,
        scope: &Arc<Scope>,
        thread: &str,
    ) -> Result<(Arc<CodexThread>, CloseTarget), JSONRPCErrorError> {
        if let Ok(actual) = self.installed_side_core(scope, thread) {
            return Ok((actual, CloseTarget::Installed));
        }
        let p = scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (id, actual) = p
            .created
            .as_ref()
            .filter(|(id, _)| id.to_string() == thread)
            .ok_or_else(|| invalid_request("actual NEW Side close birth missing"))?;
        let held = p
            .held_selection
            .clone()
            .ok_or_else(|| invalid_request("actual NEW Side close inhibitor missing"))?;
        let actual = actual.clone();
        let id = *id;
        drop(p);
        self.check_side_close_candidate(scope, &actual)?;
        Ok((actual, CloseTarget::Candidate { id, held }))
    }
    fn check_side_close_candidate(
        &self,
        scope: &Arc<Scope>,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        let p = scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if scope.frozen.retains_candidate()
            || !matches!(scope.arguments, Arguments::Fork(_))
            || !p.worker_finished
            || p.worker_lost
            || p.selection_unknown
            || p.selection_retired
            || !p.entered
            || p.resolved.is_some()
            || !matches!(p.complete, Some(Reply::Fork(..)))
            || !p
                .created
                .as_ref()
                .is_some_and(|(_, born)| Arc::ptr_eq(born, actual))
            || !p.primary.as_ref().is_some_and(|lease| lease.is_for(actual))
        {
            return Err(invalid_request(
                "actual uninstalled NEW Side close owner unavailable",
            ));
        }
        Ok(())
    }
}
impl Scope {
    fn side_cleanup_retryable(&self) -> bool {
        let rollback = self
            .side_rollback
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        rollback.as_ref().is_some_and(|job| job.failed_and_joined())
    }
}

impl InitialSessionOperation {
    async fn observe_side_close(
        &self,
        context: &RequestContext,
        selection: &metadata::Selection,
        parameters: &serde_json::Value,
        args: &ThreadUnsubscribeParams,
    ) -> Result<(), JSONRPCErrorError> {
        let observed = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed || self.input.is_stopped() {
                return Err(invalid_request("Side close observation owner closed"));
            }
            match state.clients.get(&selection.view().claim) {
                None => metadata::observation::State::Unowned,
                Some(scope) => {
                    let closings = scope
                        .side_closings
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    match closings.jobs.get(selection.operation()) {
                        None => metadata::observation::State::Unowned,
                        Some(job) => {
                            if &job.parameters != parameters || &job.args != args {
                                return Err(invalid_request(
                                    "Side close observation changed full original wire",
                                ));
                            }
                            let mut p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                            if p.unknown || p.result.is_none() {
                                return Err(invalid_request("actual Side close remains unknown"));
                            }
                            p.observed = true;
                            metadata::observation::State::Observed
                        }
                    }
                }
            }
        };
        let data = metadata::observation::Observation::new(
            selection.operation().into(),
            selection.view().claim.clone(),
            args.thread_id.clone(),
            observed,
        );
        if !data.matches(
            selection.operation(),
            selection.view().claim.as_str(),
            args.thread_id.as_str(),
        ) {
            return Err(internal_error("Source observation contract changed"));
        }
        let payload = codex_app_server_protocol::alab_response_metadata::acknowledgment(
            metadata::OBSERVATION_KEY,
            serde_json::to_value(data).map_err(|e| internal_error(e.to_string()))?,
        )
        .map_err(|e| internal_error(e.to_string()))?;
        self.processor
            .outgoing
            .send_response_as(context.alab_request_id().clone(), payload)
            .await;
        Ok(())
    }
}

#[cfg(all(test, unix))]
#[path = "retained_side_view_close_guard_tests.rs"]
mod guard_tests;
