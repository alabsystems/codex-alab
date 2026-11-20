// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original attached delete and outcome custody on SAME accepted Side Scope.
//! No observer loss retries deletion or proves Native/effects settlement.
use super::*;
use crate::outgoing_message::alab_side_rollback_claim::Claim;
use crate::outgoing_message::alab_side_rollback_claim::metadata;
use codex_app_server_protocol::ThreadDeleteParams;
use codex_app_server_protocol::ThreadDeleteResponse;
pub(super) struct Rollback {
    actual: Arc<CodexThread>,
    id: ThreadId,
    operation: String,
    parameters: serde_json::Value,
    args: ThreadDeleteParams,
    progress: Mutex<Progress>,
}
#[derive(Default)]
struct Progress {
    reply: Option<Result<ThreadDeleteResponse, JSONRPCErrorError>>,
    observers: Vec<RequestContext>,
    observed: bool,
    joined: bool,
    unknown: bool,
}
struct Guard {
    job: Arc<Rollback>,
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
            self.job
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .unknown = true;
            self.scope
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .selection_unknown = true;
            self.scope.input.fail(std::io::Error::other(
                "accepted original Side rollback remains unobserved",
            ));
        }
    }
}
impl InitialSlot {
    pub(crate) async fn rollback_side(
        &self,
        context: RequestContext,
        args: ThreadDeleteParams,
    ) -> Result<(), JSONRPCErrorError> {
        let owner = self
            .0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid_request("actual Side rollback owner unavailable"))?;
        owner.rollback_side(context, args).await
    }
}
impl InitialSessionOperation {
    async fn rollback_side(
        self: &Arc<Self>,
        context: RequestContext,
        args: ThreadDeleteParams,
    ) -> Result<(), JSONRPCErrorError> {
        let Claim {
            selection,
            parameters,
        } = context
            .alab_side_rollback
            .as_deref()
            .cloned()
            .ok_or_else(|| invalid_request("Side rollback selection missing"))?;
        let origin =
            context
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| invalid_request("original Side rollback needs actual Native A"))?;
        self.observe_bound_side_origin(&origin)
            .await
            .map_err(|_| invalid_request("Side rollback origin is not current"))?;
        if matches!(selection, metadata::Selection::Observe { .. }) {
            return self
                .observe_side_rollback(context, &selection, &parameters, &args)
                .await;
        }
        let scope = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed || self.input.is_stopped() {
                return Err(invalid_request("Side rollback admission closed"));
            }
            state
                .clients
                .get(selection.claim())
                .cloned()
                .ok_or_else(|| invalid_request("actual Side construction unavailable"))?
        };
        let held = {
            let p = scope
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            p.held_selection.clone()
        };
        // SDK -> Source lock order. Replays of the already-owned cleanup may
        // outlive exact ticket retirement; fresh cleanup still needs it held.
        let fresh = scope
            .side_rollback
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none();
        if fresh {
            held.as_ref()
                .ok_or_else(|| invalid_request("actual Side inhibitor unavailable"))?
                .recheck_held()
                .map_err(|_| invalid_request("actual Side inhibitor is not held"))?;
        }
        let (job, worker, cached) = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed
                || self.input.is_stopped()
                || !state
                    .clients
                    .get(selection.claim())
                    .is_some_and(|current| Arc::ptr_eq(current, &scope))
            {
                return Err(invalid_request("actual Side rollback owner withdrew"));
            }
            let mut boundary = scope
                .side_boundary
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut slot = scope
                .side_rollback
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(job) = slot.as_ref() {
                if job.operation != selection.operation()
                    || job.parameters != parameters
                    || job.args != args
                {
                    return Err(invalid_request(
                        "Side rollback changed full original operation",
                    ));
                }
                let mut p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                if p.unknown {
                    return Err(invalid_request("Side rollback outcome is unknown"));
                }
                if matches!(selection, metadata::Selection::Observe { .. }) {
                    if p.reply.is_none() {
                        return Err(invalid_request("original rollback result unavailable"));
                    }
                    p.observed = true;
                }
                let cached = p.reply.clone();
                if cached.is_none()
                    && !p
                        .observers
                        .iter()
                        .any(|old| old.alab_request_id() == context.alab_request_id())
                {
                    if p.observers.len() >= MAX_PENDING_REPLAYS {
                        return Err(invalid_request("Side rollback observers full"));
                    }
                    p.observers.push(context.clone());
                }
                (job.clone(), None, cached)
            } else {
                if !matches!(selection, metadata::Selection::Rollback { .. })
                    || boundary.projected
                    || boundary.retiring
                    || scope.frozen.retains_candidate()
                    || !matches!(scope.arguments, Arguments::Fork(_))
                {
                    return Err(invalid_request(
                        "Side candidate cannot enter original rollback",
                    ));
                }
                let p = scope
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if !p.worker_finished
                    || p.worker_lost
                    || p.selection_unknown
                    || p.selection_retired
                    || !p.entered
                    || p.resolved.is_some()
                    || !matches!(p.complete, Some(Reply::Fork(..)))
                {
                    return Err(invalid_request(
                        "original NEW Side construction is not observed",
                    ));
                }
                let (id, actual) = p
                    .created
                    .as_ref()
                    .filter(|(id, actual)| {
                        id.to_string() == args.thread_id
                            && p.primary.as_ref().is_some_and(|lease| lease.is_for(actual))
                    })
                    .ok_or_else(|| invalid_request("original Side birth/lease resource missing"))?;
                let job = Arc::new(Rollback {
                    actual: actual.clone(),
                    id: *id,
                    operation: selection.operation().into(),
                    parameters,
                    args,
                    progress: Mutex::new(Progress {
                        observers: vec![context.clone()],
                        ..Default::default()
                    }),
                });
                drop(p);
                let worker = Guard {
                    job: job.clone(),
                    scope: scope.clone(),
                    finished: false,
                    _tracked: self.tracker.token(),
                };
                boundary.retiring = true; // SAME exclusion lock as first S2 capture.
                *slot = Some(job.clone());
                (job, Some(worker), None)
            }
        };
        if let Some(cached) = cached {
            job.publish(&self.processor.outgoing, &context, &cached)
                .await;
        }
        if let Some(mut guard) = worker {
            let owner = self.clone();
            Box::pin(async move {
                let job = guard.job.clone();
                let result = AssertUnwindSafe(super::super::alab_side_rollback_target::run(
                    job.id,
                    job.actual.clone(),
                    owner.processor.alab_original_side_delete(job.args.clone()),
                ))
                .catch_unwind()
                .await;
                let Ok(result) = result else { return };
                let (reply, deleted) = match result {
                    Ok((response, deleted)) => (Ok(response), deleted),
                    Err(error) => (Err(error), Vec::new()),
                };
                let observers = {
                    let mut p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                    p.reply = Some(reply.clone());
                    std::mem::take(&mut p.observers)
                };
                // Original reply is retained before publication/notifications or
                // Native cleanup. An original error is never rewritten.
                for observer in observers {
                    job.publish(&owner.processor.outgoing, &observer, &reply)
                        .await;
                }
                owner.processor.alab_original_side_deleted(deleted).await;
                let terminated = job.actual.wait_until_terminated().now_or_never().is_some();
                if terminated {
                    let Some(held) = held else { return };
                    let check = {
                        let scope = scope.clone();
                        let job = job.clone();
                        let owner = owner.clone();
                        move || {
                            if !scope.prior_selection.unchanged(&owner) {
                                return Err("prior primary changed during Side rollback".into());
                            }
                            let p = scope
                                .progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner);
                            let cleanup =
                                job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                            if !p.worker_finished
                                || p.worker_lost
                                || p.selection_unknown
                                || p.selection_retired
                                || !p.entered
                                || p.resolved.is_some()
                                || !p.created.as_ref().is_some_and(|(id, actual)| {
                                    *id == job.id && Arc::ptr_eq(actual, &job.actual)
                                })
                                || job.actual.wait_until_terminated().now_or_never().is_none()
                                || cleanup.unknown
                                || cleanup.reply.is_none()
                            {
                                return Err(
                                    "original Side termination/rollback completion is not observed"
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
                // A normal original error with live Core preserves its ticket,
                // lease and original whole SidebarFork for explicit UI recovery.
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
impl Rollback {
    async fn publish(
        &self,
        outgoing: &crate::outgoing_message::OutgoingMessageSender,
        context: &RequestContext,
        reply: &Result<ThreadDeleteResponse, JSONRPCErrorError>,
    ) {
        match reply {
            Ok(response) => {
                outgoing
                    .send_response_as(context.alab_request_id().clone(), response.clone().into())
                    .await
            }
            Err(error) => {
                outgoing
                    .send_error(context.alab_request_id().clone(), error.clone())
                    .await
            }
        }
    }
}
impl Scope {
    pub(super) fn unobserved_side_rollback(&self) -> bool {
        self.side_rollback
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|job| {
                let p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                p.unknown || !p.joined || !p.observed
            })
    }
    pub(super) fn unknown_side_rollback(&self) -> bool {
        self.side_rollback
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|job| {
                let p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                p.unknown || !p.joined
            })
    }
}

impl Rollback {
    pub(super) fn failed_and_joined(&self) -> bool {
        let p = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        p.joined && !p.unknown && matches!(p.reply, Some(Err(_)))
    }
}

impl InitialSessionOperation {
    async fn observe_side_rollback(
        &self,
        context: RequestContext,
        selection: &metadata::Selection,
        parameters: &serde_json::Value,
        args: &ThreadDeleteParams,
    ) -> Result<(), JSONRPCErrorError> {
        let observed = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed || self.input.is_stopped() {
                return Err(invalid_request("Side rollback observation owner closed"));
            }
            match state.clients.get(selection.claim()) {
                None => metadata::observation::State::Unowned,
                Some(scope) => {
                    let slot = scope
                        .side_rollback
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    match &*slot {
                        None => metadata::observation::State::Unowned,
                        Some(job) => {
                            if job.operation != selection.operation()
                                || &job.parameters != parameters
                                || &job.args != args
                            {
                                return Err(invalid_request(
                                    "Side rollback observation changed full original wire",
                                ));
                            }
                            let mut p = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                            if p.unknown || p.reply.is_none() {
                                return Err(invalid_request(
                                    "actual Side rollback remains unknown",
                                ));
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
            selection.claim().into(),
            args.thread_id.clone(),
            observed,
        );
        if !data.matches(
            selection.operation(),
            selection.claim(),
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
#[path = "retained_side_rollback_guard_tests.rs"]
mod guard_tests;
