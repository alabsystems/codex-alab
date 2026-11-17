// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Config/read on the SAME configured runtime and genuinely empty UI-primary
//! slot. The whole accepted operation belongs to the physical-bind owner.
//! Neither configured completion nor its Native commit releases a Goal Hold.
use super::super::alab_initial_session::selected_primary::CurrentPrimary;
use super::super::alab_initial_session::selected_primary::EmptyPrimary;
use super::*;
use crate::outgoing_message::alab_configured_claim::Claim;
use crate::outgoing_message::alab_configured_claim::RESULT_KEY;
use crate::outgoing_message::alab_configured_claim::Receipt;
use crate::outgoing_message::alab_configured_claim::Selection;
use crate::request_processors::config_processor::ConfigRequestProcessor;
use codex_alab_aterm::frontend_control::retained_service::ConfiguredRuntimeAck as RuntimeAck;
use codex_alab_aterm::frontend_control::retained_service::ConfiguredRuntimeActivation as RuntimeActivation;
use codex_alab_aterm::frontend_control::retained_service::ConfiguredRuntimeCommit;
use codex_alab_aterm::frontend_control::retained_service::ConfiguredRuntimeFrontier as RuntimeFrontier;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;

#[path = "retained_configured_hold.rs"]
mod closed_hold;

#[derive(Default)]
pub(super) struct ConfiguredState {
    // Installed once with the SAME configured operation, before dispatch.
    // The physical-bind owner, not a UI projection, owns this closed custody.
    custody: Option<Arc<closed_hold::Keeper>>,
    projections: BTreeMap<String, Arc<ConfiguredProjection>>,
    latest: Option<String>,
    observations: BTreeMap<String, Arc<ConfiguredAction>>,
    commits: BTreeMap<String, Arc<ConfiguredAction>>,
}
type RetiredConfigured = (
    Option<Arc<ConfiguredProjection>>,
    Option<Arc<ConfiguredAction>>,
    Option<Arc<ConfiguredAction>>,
);

impl ConfiguredState {
    /// Only a NEW actual physical frontier supersedes these receipts. The
    /// original Native A check has already rejected stale old deliveries.
    /// Unknown/unfinished work is never discarded for capacity.
    fn retire_completed(&mut self) -> Vec<RetiredConfigured> {
        let keys = self
            .projections
            .iter()
            .filter(|(key, work)| {
                work.completion.result.borrow().is_some()
                    && !work.completion.uncertain.load(Ordering::Acquire)
                    && self.observations.get(*key).is_none_or(|action| {
                        action.completion.result.borrow().is_some()
                            && !action.completion.uncertain.load(Ordering::Acquire)
                    })
                    && self.commits.get(*key).is_none_or(|action| {
                        action.completion.result.borrow().is_some()
                            && !action.completion.uncertain.load(Ordering::Acquire)
                    })
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        // Snapshot eligibility before mutating the same maps. Return all
        // original resources so their final drops occur outside the state lock.
        let mut retired = Vec::with_capacity(keys.len());
        for key in keys {
            retired.push((
                self.projections.remove(&key),
                self.observations.remove(&key),
                self.commits.remove(&key),
            ));
        }
        retired
    }
    pub(super) fn unobserved(&self) -> bool {
        self.projections.values().any(|work| {
            work.completion.result.borrow().is_none()
                || work.completion.uncertain.load(Ordering::Acquire)
        }) || self
            .observations
            .values()
            .chain(self.commits.values())
            .any(|work| {
                work.completion.result.borrow().is_none()
                    || work.completion.uncertain.load(Ordering::Acquire)
            })
            || self
                .custody
                .as_ref()
                .is_some_and(|keeper| keeper.unobserved())
    }
}

struct ConfiguredProjection {
    receipt: Receipt,
    request: ConnectionRequestId,
    params: ConfigReadParams,
    arguments: Arc<serde_json::Value>,
    origin: Arc<AcceptedOrigin>,
    empty: EmptyPrimary,
    frontier: RuntimeFrontier,
    _context: RequestContext,
    response: StdMutex<Option<ConfigReadResponse>>,
    response_entered: AtomicBool,
    response_attempted: AtomicBool,
    completion: Arc<ConfiguredCompletion>,
    ack: StdMutex<Option<RuntimeAck>>,
    stage: StdMutex<Option<RuntimeActivation>>,
    commit: StdMutex<Option<ConfiguredRuntimeCommit>>,
}

#[derive(Clone, Copy)]
enum ConfiguredPhase {
    Observe,
    Commit,
}
struct ConfiguredAction {
    projection: Arc<ConfiguredProjection>,
    phase: ConfiguredPhase,
    _context: RequestContext,
    completion: Arc<ConfiguredCompletion>,
}
struct ConfiguredCompletion {
    uncertain: AtomicBool,
    result: watch::Sender<Option<Result<(), JSONRPCErrorError>>>,
}
impl ConfiguredCompletion {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            uncertain: AtomicBool::new(false),
            result: watch::channel(None).0,
        })
    }
}

impl Binding {
    pub(crate) async fn handle_configured(
        self: &Arc<Self>,
        request: ConnectionRequestId,
        context: RequestContext,
        params: ConfigReadParams,
        claim: Claim,
        config: ConfigRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        if context.alab_request_id() != &request {
            return Err(invalid_request(
                "configured original request context disagrees",
            ));
        }
        let origin =
            context
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| {
                    invalid_request("configured frontier requires actual physical A origin")
                })?;
        origin.check_current().await.map_err(native_error)?;
        match claim.selection {
            Selection::Project { frontier } => {
                // This is the actual configured operation's selected slot. It
                // may be empty while startup/picker or captured Core work exists.
                let empty = configured_empty(&self.processor, &self.input)?;
                let actual = self
                    .backend
                    .configured_frontier(&frontier, &origin)
                    .await
                    .map_err(native_error)?;
                empty.recheck_owned()?;
                let duplicate = {
                    let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                    state
                        .configured
                        .projections
                        .values()
                        .find(|work| work.receipt.frontier == frontier)
                        .cloned()
                };
                if let Some(work) = duplicate {
                    if work.params != params
                        || work.arguments != claim.arguments
                        || !work.frontier.same_receipt(&origin)
                        || !work.empty.same_version(&empty)
                    {
                        return Err(invalid_request("configured accepted projection disagrees"));
                    }
                    work.wait_ready(&self.input).await?;
                    self.current_configured(&work)?;
                    return self.publish_configured_projection(&work, request).await;
                }
                let work = Arc::new(ConfiguredProjection {
                    receipt: Receipt {
                        frontier,
                        snapshot: token(),
                        runtime: empty.runtime_id_data().to_owned(),
                        version: empty.version_data(),
                    },
                    request: request.clone(),
                    params: params.clone(),
                    arguments: claim.arguments,
                    origin,
                    empty,
                    frontier: actual,
                    _context: context,
                    response: StdMutex::new(None),
                    response_entered: AtomicBool::new(false),
                    response_attempted: AtomicBool::new(false),
                    completion: ConfiguredCompletion::new(),
                    ack: StdMutex::new(None),
                    stage: StdMutex::new(None),
                    commit: StdMutex::new(None),
                });
                let guard = {
                    let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                    if state.closed {
                        return Err(invalid_request("configured frontier admission unavailable"));
                    }
                    // Recheck under the actual acceptance lock; a concurrent
                    // original request cannot create a second accepted worker.
                    if state
                        .configured
                        .projections
                        .values()
                        .any(|prior| prior.receipt.frontier == work.receipt.frontier)
                    {
                        return Err(invalid_request("configured projection acceptance raced"));
                    }
                    let retired = state.configured.retire_completed();
                    if state.configured.projections.len() >= MAX_RECEIPTS {
                        drop(state);
                        drop(retired);
                        return Err(invalid_request("configured frontier capacity unavailable"));
                    }
                    state.configured.latest = Some(work.receipt.snapshot.clone());
                    state.callbacks.frontier_changed(); // ALAB(retained-ui): configured transition invalidates old Core observer receipts.
                    state
                        .configured
                        .projections
                        .insert(work.receipt.snapshot.clone(), work.clone());
                    (
                        ConfiguredGuard::new(self, work.clone(), work.completion.clone()),
                        retired,
                    )
                };
                let (guard, retired) = guard;
                drop(retired); // Original context/config/runtime resources leave locks first.
                let binding = self.clone();
                tokio::spawn(async move {
                    let result =
                        observe_configured(binding.project_configured(&work, config)).await;
                    let result = match result {
                        Ok(result) => result,
                        Err(()) => {
                            binding.input.fail(std::io::Error::other(
                                "configured projection outcome remains unobserved",
                            ));
                            work.completion.uncertain.store(true, Ordering::Release);
                            Err(internal_error(
                                "configured projection outcome remains unobserved",
                            ))
                        }
                    };
                    if let Err(error) = &result {
                        if work.response_attempted.load(Ordering::Acquire) {
                            work.tail(false, &binding.processor.outgoing).await;
                        } else if work.response_entered.load(Ordering::Acquire) {
                            // Sender entry is not enqueue/delivery proof. The
                            // original request must not remain pending or get a
                            // second guessed response on an uncertain send.
                            binding.input.fail(std::io::Error::other(
                                "configured response outcome remains unobserved",
                            ));
                            work.completion.uncertain.store(true, Ordering::Release);
                        } else {
                            binding
                                .processor
                                .outgoing
                                .send_error(request, error.clone())
                                .await;
                        }
                    }
                    work.completion.result.send_replace(Some(result));
                    guard.finish();
                });
                Ok(())
            }
            Selection::Observe(receipt) => {
                self.accept_configured_action(
                    request,
                    context,
                    origin,
                    receipt,
                    ConfiguredPhase::Observe,
                )
                .await
            }
            Selection::Commit(receipt) => {
                self.accept_configured_action(
                    request,
                    context,
                    origin,
                    receipt,
                    ConfiguredPhase::Commit,
                )
                .await
            }
        }
    }

    fn current_configured(&self, work: &ConfiguredProjection) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        work.empty.recheck_owned()?;
        if !work.empty.belongs_to(&self.input)
            || !work.frontier.same_receipt(&work.origin)
            || self
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .configured
                .latest
                .as_ref()
                != Some(&work.receipt.snapshot)
        {
            return Err(invalid_request("configured projection ownership changed"));
        }
        Ok(())
    }

    async fn project_configured(
        &self,
        work: &ConfiguredProjection,
        config: ConfigRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        self.current_configured(work)?;
        // The caller passed its SAME real processor clone. Preserve every
        // original effective-config/layer/model-provider read and typed result.
        let response = read_original_config(&config, work.params.clone()).await?;
        self.current_configured(work)?;
        *work.response.lock().unwrap_or_else(PoisonError::into_inner) = Some(response);
        work.completion.uncertain.store(true, Ordering::Release);
        let ack = match work.frontier.acknowledge_configured(&work.origin).await {
            Ok(ack) => ack,
            Err(RuntimeObservationError::Refused(error)) => {
                work.completion.uncertain.store(false, Ordering::Release);
                return Err(native_error(error));
            }
            Err(RuntimeObservationError::Uncertain(error)) => return Err(native_error(error)),
        };
        *work.ack.lock().unwrap_or_else(PoisonError::into_inner) = Some(ack);
        work.completion.uncertain.store(false, Ordering::Release);
        self.current_configured(work)?;
        self.publish_configured_projection(work, work.request.clone())
            .await?;
        work.response_attempted.store(true, Ordering::Release);
        work.tail(true, &self.processor.outgoing).await;
        Ok(())
    }

    async fn publish_configured_projection(
        &self,
        work: &ConfiguredProjection,
        request: ConnectionRequestId,
    ) -> Result<(), JSONRPCErrorError> {
        let response = work
            .response
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| internal_error("actual configured response unavailable"))?;
        let payload = codex_app_server_protocol::alab_response_metadata::decorate(
            response.into(),
            RESULT_KEY,
            serde_json::to_value(&work.receipt)
                .map_err(|_| internal_error("configured receipt encode failed"))?,
        )
        .map_err(|_| internal_error("configured response metadata failed"))?;
        work.response_entered.store(true, Ordering::Release);
        self.processor
            .outgoing
            .send_response(request, payload)
            .await;
        Ok(())
    }

    async fn accept_configured_action(
        self: &Arc<Self>,
        request: ConnectionRequestId,
        context: RequestContext,
        origin: Arc<AcceptedOrigin>,
        receipt: Receipt,
        phase: ConfiguredPhase,
    ) -> Result<(), JSONRPCErrorError> {
        let work = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .configured
            .projections
            .get(&receipt.snapshot)
            .cloned()
            .ok_or_else(|| invalid_request("configured projection unavailable"))?;
        if receipt != work.receipt || !work.frontier.same_receipt(&origin) {
            return Err(invalid_request(
                "configured completion belongs to another physical delivery",
            ));
        }
        work.wait_ready(&self.input).await?;
        self.current_configured(&work)?;
        let (accepted, duplicate) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed || state.configured.latest.as_ref() != Some(&receipt.snapshot) {
                return Err(invalid_request("configured completion was superseded"));
            }
            let operations = match phase {
                ConfiguredPhase::Observe => &mut state.configured.observations,
                ConfiguredPhase::Commit => &mut state.configured.commits,
            };
            if let Some(actual) = operations.get(&receipt.snapshot) {
                if !Arc::ptr_eq(&actual.projection, &work) {
                    return Err(invalid_request("configured accepted completion disagrees"));
                }
                (None, Some(actual.clone()))
            } else {
                if operations.len() >= MAX_RECEIPTS {
                    return Err(invalid_request(
                        "configured completion capacity unavailable",
                    ));
                }
                let action = Arc::new(ConfiguredAction {
                    projection: work,
                    phase,
                    _context: context,
                    completion: ConfiguredCompletion::new(),
                });
                operations.insert(receipt.snapshot.clone(), action.clone());
                let guard = ConfiguredGuard::new(self, action.clone(), action.completion.clone());
                (Some((action, guard)), None)
            }
        };
        if let Some(action) = duplicate {
            let result = wait_configured(&self.input, action.completion.result.subscribe()).await;
            Self::publish_configured_completion(
                &self.processor.outgoing,
                request,
                &action.projection.receipt,
                result,
            )
            .await;
            return Ok(());
        }
        let (action, guard) = accepted
            .ok_or_else(|| internal_error("configured completion acceptance unavailable"))?;
        let binding = self.clone();
        tokio::spawn(async move {
            let result = match observe_configured(binding.run_configured_action(&action)).await {
                Ok(result) => result,
                Err(()) => {
                    binding.input.fail(std::io::Error::other(
                        "configured completion outcome remains unobserved",
                    ));
                    action.completion.uncertain.store(true, Ordering::Release);
                    Err(internal_error(
                        "configured completion outcome remains unobserved",
                    ))
                }
            };
            action.completion.result.send_replace(Some(result.clone()));
            Self::publish_configured_completion(
                &binding.processor.outgoing,
                request,
                &action.projection.receipt,
                result,
            )
            .await;
            guard.finish();
        });
        Ok(())
    }

    async fn run_configured_action(
        self: &Arc<Self>,
        action: &ConfiguredAction,
    ) -> Result<(), JSONRPCErrorError> {
        let work = &action.projection;
        self.current_configured(work)?;
        let ack = work
            .ack
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| {
                invalid_request("actual configured runtime acknowledgment unavailable")
            })?;
        action.completion.uncertain.store(true, Ordering::Release);
        match action.phase {
            ConfiguredPhase::Observe => {
                let staged = match ack.observe_activation().await {
                    Ok(value) => value,
                    Err(RuntimeObservationError::Refused(error)) => {
                        action.completion.uncertain.store(false, Ordering::Release);
                        return Err(native_error(error));
                    }
                    Err(RuntimeObservationError::Uncertain(error)) => {
                        return Err(native_error(error));
                    }
                };
                *work.stage.lock().unwrap_or_else(PoisonError::into_inner) = Some(staged);
                self.current_configured(work)?;
            }
            ConfiguredPhase::Commit => {
                let staged = work
                    .stage
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                    .ok_or_else(|| invalid_request("actual configured Native stage unavailable"))?;
                let committed = match ack.observe_commit(&staged).await {
                    Ok(value) => value,
                    Err(RuntimeObservationError::Refused(error)) => {
                        action.completion.uncertain.store(false, Ordering::Release);
                        return Err(native_error(error));
                    }
                    Err(RuntimeObservationError::Uncertain(error)) => {
                        return Err(native_error(error));
                    }
                };
                *work.commit.lock().unwrap_or_else(PoisonError::into_inner) = Some(committed);
                self.current_configured(work)?;
                // No release call exists in this route. Even a pending Core
                // cannot acquire a fresh automatic window from configured UI.
            }
        }
        action.completion.uncertain.store(false, Ordering::Release);
        Ok(())
    }

    async fn publish_configured_completion(
        outgoing: &Arc<OutgoingMessageSender>,
        request: ConnectionRequestId,
        receipt: &Receipt,
        result: Result<(), JSONRPCErrorError>,
    ) {
        let payload = result.and_then(|()| {
            codex_app_server_protocol::alab_response_metadata::acknowledgment(
                RESULT_KEY,
                serde_json::to_value(receipt)
                    .map_err(|_| internal_error("configured receipt encode failed"))?,
            )
            .map_err(|_| internal_error("configured completion encode failed"))
        });
        match payload {
            Ok(payload) => outgoing.send_response(request, payload).await,
            Err(error) => outgoing.send_error(request, error).await,
        }
    }
}

impl ConfiguredProjection {
    async fn wait_ready(&self, input: &Incoming) -> Result<(), JSONRPCErrorError> {
        wait_configured(input, self.completion.result.subscribe()).await?;
        if self.completion.uncertain.load(Ordering::Acquire) {
            return Err(invalid_request(
                "configured projection outcome remains unobserved",
            ));
        }
        Ok(())
    }
    async fn tail(&self, ready: bool, outgoing: &Arc<OutgoingMessageSender>) {
        use codex_app_server_protocol::RetainedConfiguredNotification;
        use codex_app_server_protocol::RetainedFrontierFailure;
        use codex_app_server_protocol::RetainedFrontierOutcome;
        outgoing
            .send_server_notification_to_connections(
                &[self.request.connection_id],
                ServerNotification::AlabRetainedConfigured(RetainedConfiguredNotification {
                    frontier: self.receipt.frontier.clone(),
                    snapshot: self.receipt.snapshot.clone(),
                    runtime: self.receipt.runtime.clone(),
                    version: self.receipt.version,
                    outcome: if ready {
                        RetainedFrontierOutcome::Ready {}
                    } else {
                        RetainedFrontierOutcome::Failed {
                            reason: RetainedFrontierFailure::Unavailable,
                        }
                    },
                }),
            )
            .await;
    }
}

async fn wait_configured(
    input: &Incoming,
    mut changed: watch::Receiver<Option<Result<(), JSONRPCErrorError>>>,
) -> Result<(), JSONRPCErrorError> {
    loop {
        if let Some(result) = changed.borrow_and_update().clone() {
            return result;
        }
        tokio::select! {
            biased;
            () = input.withdrawn() => return Err(invalid_request("configured owner withdrew")),
            result = changed.changed() => result.map_err(|_| internal_error("configured observation unavailable"))?,
        }
    }
}
async fn observe_configured<T>(future: impl std::future::Future<Output = T>) -> Result<T, ()> {
    let mut future = Box::pin(AssertUnwindSafe(future).catch_unwind());
    let result = future.as_mut().await;
    let destroyed = catch_unwind(AssertUnwindSafe(|| drop(future)));
    match (result, destroyed) {
        (Ok(result), Ok(())) => Ok(result),
        _ => Err(()),
    }
}

/// The resource is kept OUTSIDE the polled future as well as in the accepted
/// registry. Generic R is custody only; it never supplies Native authority.
struct ConfiguredGuard<R> {
    input: Incoming,
    completion: Arc<ConfiguredCompletion>,
    _resource: R,
    _token: tokio_util::task::task_tracker::TaskTrackerToken,
    finished: bool,
}
impl<R> ConfiguredGuard<R> {
    fn new(binding: &Binding, resource: R, completion: Arc<ConfiguredCompletion>) -> Self {
        Self {
            input: binding.input.clone(),
            completion,
            _resource: resource,
            _token: binding.tracker.token(),
            finished: false,
        }
    }
    fn finish(mut self) {
        self.finished = true;
    }
}
impl<R> Drop for ConfiguredGuard<R> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.input.fail(std::io::Error::other(
            "configured accepted outcome remains unobserved",
        ));
        self.completion.uncertain.store(true, Ordering::Release);
        self.completion.result.send_replace(Some(Err(internal_error(
            "configured accepted outcome remains unobserved",
        ))));
    }
}

impl ThreadRequestProcessor {
    pub(crate) async fn alab_retained_configured(
        &self,
        request: ConnectionRequestId,
        context: RequestContext,
        params: ConfigReadParams,
        claim: Claim,
        config: ConfigRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        self.alab_retained_frontier
            .actual()?
            .handle_configured(request, context, params, claim, config)
            .await
    }
}

/// Source resource check only. Empty means no observed UI-selected primary,
/// even while the actual manager still owns a pending/live Core.
fn configured_empty(
    processor: &ThreadRequestProcessor,
    input: &Incoming,
) -> Result<EmptyPrimary, JSONRPCErrorError> {
    let empty = match processor.alab_initial_session.current_primary()? {
        CurrentPrimary::Empty(empty) => empty,
        CurrentPrimary::Selected(_) => {
            return Err(invalid_request(
                "configured frontier requires the actual empty primary slot",
            ));
        }
    };
    if !empty.belongs_to(input) {
        return Err(invalid_request(
            "configured runtime belongs to another input",
        ));
    }
    empty.recheck_owned()?;
    Ok(empty)
}
async fn read_original_config(
    config: &ConfigRequestProcessor,
    params: ConfigReadParams,
) -> Result<ConfigReadResponse, JSONRPCErrorError> {
    config.read(params).await
}

#[cfg(test)]
#[path = "retained_configured_runtime_tests.rs"]
mod tests;

#[path = "retained_configured_callback_proof.rs"]
mod callback_proof; // ALAB(retained-ui): actual configured Empty projection/commit proof for OWN closed endpoint cleanup, no Hold release.
