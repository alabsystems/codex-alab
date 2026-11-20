// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Full original request DATA and actual pre-accept Native reservation custody.
//! A client nonce is replay correlation only. Neither it nor an offer admits a
//! caller: every preparation/accept/replay checks its real received A resource.
use super::*;
#[path = "client_retirement_drain_policy.rs"]
mod drain_policy;
use crate::outgoing_message::alab_initial_claim::PreparationCompletion as Completion;
use crate::outgoing_message::alab_initial_claim::PreparationOffer as Offer;
use crate::outgoing_message::alab_initial_claim::PreparationRequest as Request;
use crate::outgoing_message::alab_initial_claim::PreparedArguments as Prepare;
use codex_alab_aterm::frontend_control::retained_service::RegisteredSourceTransition;
use codex_alab_aterm::frontend_control::retained_service::RuntimeObservationError;
use codex_alab_aterm::frontend_control::retained_service::SourceTransition;

#[path = "saved_resume_lookup.rs"]
mod saved_resume_lookup;

#[derive(Clone)]
enum FrozenPreparation {
    Client(Box<Prepare>),
    Initial(Claim),
}
impl FrozenPreparation {
    fn preparation(&self) -> &str {
        match self {
            Self::Client(value) => &value.preparation,
            Self::Initial(value) => &value.selection.claim,
        }
    }
    fn key(&self) -> String {
        match self {
            Self::Client(_) => format!("client:{}", self.preparation()),
            Self::Initial(_) => format!("initial:{}", self.preparation()),
        }
    }
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Client(a), Self::Client(b)) => a == b,
            (Self::Initial(a), Self::Initial(b)) => {
                a.selection == b.selection && a.method == b.method && a.arguments == b.arguments
            }
            _ => false,
        }
    }
}
pub(super) enum InitialPreparation {
    Replay,
    Registered(Arc<PreparedClient>, Arc<RegisteredSourceTransition>, Offer),
    Refused(Arc<PreparedClient>, JSONRPCErrorError),
}
pub(super) struct PreparedClient {
    arguments: FrozenPreparation,
    claim: String,
    transition: Arc<SourceTransition>,
    progress: Mutex<PreparationProgress>,
    changed: tokio::sync::watch::Sender<()>,
}
#[derive(Default)]
struct PreparationProgress {
    registered: Option<Arc<RegisteredSourceTransition>>,
    offer: Option<Offer>,
    accepted: Option<Weak<Scope>>,
    error: Option<JSONRPCErrorError>,
    retiring: bool,
    retired: bool,
    unknown: bool,
    finished: bool,
}
impl PreparedClient {
    pub(super) fn saved_opening(&self) -> bool {
        matches!(&self.arguments, FrozenPreparation::Client(original) if original.saved_opening.is_some())
    }

    pub(super) fn matches(
        &self,
        claim: &crate::outgoing_message::alab_initial_claim::ClientClaim,
    ) -> bool {
        let FrozenPreparation::Client(arguments) = &self.arguments else {
            return false;
        };
        let mut selection = claim.selection.as_ref().clone();
        let offered = selection.prepared.take();
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        offered
            .as_ref()
            .is_some_and(|offer| progress.offer.as_ref() == Some(offer))
            && arguments.method.name() == claim.method.as_ref()
            && arguments.parameters == *claim.arguments
            && arguments.selection == selection
    }
    pub(super) fn registered(&self) -> Result<Arc<RegisteredSourceTransition>, JSONRPCErrorError> {
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if !progress.finished || progress.unknown || progress.retiring || progress.retired {
            return Err(invalid_request(
                "client preparation has no observed Native reservation",
            ));
        }
        progress
            .registered
            .clone()
            .ok_or_else(|| invalid_request("client preparation reservation unavailable"))
    }
    pub(super) fn offer(&self) -> Result<Offer, JSONRPCErrorError> {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .offer
            .clone()
            .ok_or_else(|| invalid_request("client preparation offer unavailable"))
    }
    pub(super) fn accept(&self, scope: &Arc<Scope>) {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .accepted = Some(Arc::downgrade(scope));
    }
    fn reclaimable_unaccepted(&self) -> bool {
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        progress.accepted.is_none()
            && !progress.unknown
            && progress.finished
            && (progress.retired || progress.error.is_some())
    }
    async fn wait(&self, input: &Incoming) -> Result<Completion, JSONRPCErrorError> {
        let mut changed = self.changed.subscribe();
        loop {
            let outcome = {
                let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
                if progress.unknown {
                    Some(Err(internal_error(
                        "client preparation outcome remains unobserved",
                    )))
                } else if progress.accepted.is_some() {
                    Some(Ok(Completion::Accepted {
                        preparation: self.arguments.preparation().to_owned(),
                        claim: self.claim.clone(),
                    }))
                } else if progress.retired {
                    Some(Ok(Completion::Retired {
                        preparation: self.arguments.preparation().to_owned(),
                        claim: self.claim.clone(),
                    }))
                } else if let Some(error) = &progress.error
                    && progress.finished
                {
                    Some(Err(error.clone()))
                } else if progress.finished && !progress.retiring {
                    progress.offer.clone().map(Completion::Prepared).map(Ok)
                } else {
                    None
                }
            };
            if let Some(outcome) = outcome {
                return outcome;
            }
            tokio::select! {
                biased;
                () = input.withdrawn() => return Err(internal_error("client preparation backend withdrew")),
                result = changed.changed() => result.map_err(|_| internal_error("client preparation observation unavailable"))?,
            }
        }
    }
}
struct PreparationGuard {
    actual: Arc<PreparedClient>,
    input: Incoming,
    finished: bool,
    _tracked: tokio_util::task::task_tracker::TaskTrackerToken,
}
impl PreparationGuard {
    fn finish(mut self) {
        self.actual
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .finished = true;
        self.finished = true;
        self.actual.changed.send_replace(());
    }
}
impl Drop for PreparationGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.actual
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .unknown = true;
            self.actual.changed.send_replace(());
            self.input.fail(std::io::Error::other(
                "client preparation worker outcome unobserved",
            ));
        }
    }
}
fn actual_origin(context: &RequestContext) -> Result<Arc<AcceptedOrigin>, JSONRPCErrorError> {
    context
        .alab_connection_resource
        .as_ref()
        .and_then(
            codex_alab_core::connection_resource::ConnectionResource::downcast::<AcceptedOrigin>,
        )
        .ok_or_else(|| {
            invalid_request("client preparation requires actual retained connection origin")
        })
}
impl InitialSlot {
    fn actual(&self) -> Result<Arc<InitialSessionOperation>, JSONRPCErrorError> {
        self.0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid_request("actual client preparation owner unavailable"))
    }
}
impl InitialSessionOperation {
    async fn prepare_client(
        self: &Arc<Self>,
        context: &RequestContext,
        request: &Request,
    ) -> Result<Completion, JSONRPCErrorError> {
        self.open()?;
        let origin = actual_origin(context)?;
        origin
            .check_current()
            .await
            .map_err(|_| invalid_request("client preparation Native origin is not current"))?;
        self.open()?;
        let actual = match request {
            Request::ReadSavedResume { .. } => {
                return Err(invalid_request("saved resume lookup requires its read-only handler"));
            }
            Request::Prepare(arguments) => {
                self.prepare_record(FrozenPreparation::Client(arguments.clone()), &origin)?
            }
            Request::Retire { preparation, claim } => {
                let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                let actual = state
                    .preparations
                    .get(&format!("client:{preparation}"))
                    .cloned()
                    .filter(|actual| actual.claim == *claim)
                    .ok_or_else(|| {
                        invalid_request("actual unaccepted client preparation unavailable")
                    })?;
                drop(state);
                self.retire_unaccepted(actual.clone(), false)?;
                actual
            }
        };
        let completed = actual.wait(&self.input).await?;
        self.open()?;
        // This checks this preparation observer only. It never cancels the
        // SDK registration or an accepted original operation after C1 EOF.
        origin
            .check_current()
            .await
            .map_err(|_| invalid_request("client preparation Native observer withdrew"))?;
        self.open()?;
        Ok(completed)
    }
    pub(super) async fn prepare_initial(
        self: &Arc<Self>,
        claim: &Claim,
        arguments: &Arguments,
        origin: &Arc<AcceptedOrigin>,
    ) -> Result<InitialPreparation, JSONRPCErrorError> {
        {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed {
                return Err(invalid_request("initial preparation admission closed"));
            }
            let renewed = state.accepted.as_ref().is_some_and(|previous| {
                state.next_claim.as_deref() == Some(claim.selection.claim.as_str())
                    && previous.can_offer_selection()
            });
            if let Some(scope) = &state.accepted
                && !renewed
            {
                return if scope.frozen.matches(claim) {
                    Ok(InitialPreparation::Replay)
                } else {
                    Err(invalid_request(
                        "initial replay changed original full arguments",
                    ))
                };
            }
            if !self.matches_original_kind(arguments)
                || (!renewed && claim.selection.claim != self.claim)
            {
                return Err(invalid_request(
                    "initial operation does not match original plan",
                ));
            }
        }
        let record = self.prepare_record(FrozenPreparation::Initial(claim.clone()), origin)?;
        // Normal no-reservation refusal has a typed SDK witness. It may only
        // produce a cached no-Core error/next-choice offer, never execute policy.
        let observed = record.wait(&self.input).await;
        self.open()?;
        if origin.check_current().await.is_err() {
            self.retire_unaccepted(record.clone(), false)?;
            let _ = record.wait(&self.input).await;
            return Err(invalid_request(
                "initial preparation Native caller withdrew",
            ));
        }
        match observed {
            Ok(Completion::Prepared(offer)) => Ok(InitialPreparation::Registered(
                record.clone(),
                record.registered()?,
                offer,
            )),
            Err(error)
                if {
                    let p = record
                        .progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    p.finished && !p.unknown && p.error.is_some() && p.registered.is_none()
                } =>
            {
                Ok(InitialPreparation::Refused(record, error))
            }
            Ok(Completion::Accepted { .. }) => Ok(InitialPreparation::Replay),
            Ok(Completion::Retired { .. }) => Ok(InitialPreparation::Refused(
                record,
                invalid_request("initial Native reservation was retired before acceptance"),
            )),
            _ => Err(internal_error(
                "initial preparation outcome is not available for acceptance",
            )),
        }
    }
    fn prepare_record(
        self: &Arc<Self>,
        arguments: FrozenPreparation,
        origin: &Arc<AcceptedOrigin>,
    ) -> Result<Arc<PreparedClient>, JSONRPCErrorError> {
        {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed {
                return Err(invalid_request("client preparation admission closed"));
            }
            if let Some(actual) = state.preparations.get(&arguments.key()) {
                return if actual.arguments.same(&arguments) {
                    Ok(actual.clone())
                } else {
                    Err(invalid_request(
                        "client preparation changed original full arguments",
                    ))
                };
            }
        }
        let bound = self.bound.as_ref().ok_or_else(|| {
            invalid_request("actual bound backend unavailable for client preparation")
        })?;
        let transition = bound
            .prepare_source_transition(origin)
            .map_err(|_| invalid_request("Native client preparation capacity unavailable"))?;
        let actual = Arc::new(PreparedClient {
            arguments: arguments.clone(),
            claim: InitialSessionPlan::fresh_claim(),
            transition: Arc::new(transition),
            progress: Mutex::new(PreparationProgress::default()),
            changed: tokio::sync::watch::channel(()).0,
        });
        let (chosen, guard, retired) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed {
                return Err(invalid_request("client preparation admission closed"));
            }
            if let Some(previous) = state.preparations.get(&arguments.key()) {
                if !previous.arguments.same(&arguments) {
                    return Err(invalid_request(
                        "client preparation changed original full arguments",
                    ));
                }
                (previous.clone(), None, Vec::new())
            } else {
                // Only demonstrably unaccepted terminal reservations may make
                // room. Never evict accepted, registering or Unknown custody.
                let keys = state
                    .preparations
                    .iter()
                    .filter(|(_, value)| value.reclaimable_unaccepted())
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                if state.preparations.len().saturating_sub(keys.len()) >= 4096 {
                    return Err(invalid_request("client preparation capacity reached"));
                }
                let retired = keys
                    .into_iter()
                    .filter_map(|key| state.preparations.remove(&key))
                    .collect::<Vec<_>>();
                state.preparations.insert(arguments.key(), actual.clone());
                let guard = PreparationGuard {
                    actual: actual.clone(),
                    input: self.input.clone(),
                    finished: false,
                    _tracked: self.tracker.token(),
                };
                (actual.clone(), Some(guard), retired)
            }
        };
        drop(retired);
        drop(actual); // Prepared SDK loser, if any, leaves every Source lock first.
        if let Some(guard) = guard {
            let worker = chosen.clone();
            tokio::spawn(async move {
                let mut work =
                    Box::pin(AssertUnwindSafe(worker.transition.register()).catch_unwind());
                let observed = work.as_mut().await;
                let destroyed = catch_unwind(AssertUnwindSafe(|| drop(work)));
                match (observed, destroyed) {
                    (Ok(Ok(registered)), Ok(())) => {
                        let selection = match registered.selection_data() {
                            Ok(value) => value,
                            Err(_) => return,
                        };
                        let offer = Offer {
                            preparation: worker.arguments.preparation().to_owned(),
                            claim: worker.claim.clone(),
                            source_operation: registered.source_operation_data().into(),
                            selection,
                        };
                        if !offer.valid() {
                            return;
                        }
                        let mut progress = worker
                            .progress
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        progress.offer = Some(offer);
                        progress.registered = Some(Arc::new(registered));
                        drop(progress);
                        guard.finish();
                    }
                    (Ok(Err(RuntimeObservationError::Refused(_))), Ok(())) => {
                        // SDK Refused is emitted only after actual known no
                        // reservation/explicit unaccepted cancellation receipt.
                        worker
                            .progress
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .error = Some(invalid_request("Native client preparation refused"));
                        guard.finish();
                    }
                    (Ok(Err(_)), Ok(())) => {
                        // A shutdown race may refuse observation before the
                        // SDK ever sends registration. Only its exact private
                        // Prepared/confirmed-Rejected phase can prove that;
                        // Registering/Reserved/Unknown must retain custody.
                        if worker.transition.discard_unregistered().is_ok() {
                            worker
                                .progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .error = Some(invalid_request(
                                "Native client preparation did not register",
                            ));
                            guard.finish();
                        } else {
                            drop(guard);
                        }
                    }
                    _ => drop(guard),
                }
            });
        }
        Ok(chosen)
    }
    fn retire_unaccepted(
        self: &Arc<Self>,
        actual: Arc<PreparedClient>,
        owner_shutdown: bool,
    ) -> Result<(), JSONRPCErrorError> {
        let guard = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed && !owner_shutdown {
                return Err(invalid_request("client preparation admission closed"));
            }
            let mut progress = actual
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if progress.accepted.is_some() || progress.retired || progress.retiring {
                return Ok(());
            }
            if progress.unknown || !progress.finished {
                return Err(invalid_request(
                    "client preparation is not known unaccepted",
                ));
            }
            if progress.error.is_some() {
                progress.retired = true;
                drop(progress);
                actual.changed.send_replace(());
                return Ok(());
            }
            let registered = progress
                .registered
                .clone()
                .ok_or_else(|| internal_error("client preparation reservation missing"))?;
            progress.retiring = true;
            drop(progress);
            (
                registered,
                PreparationGuard {
                    actual: actual.clone(),
                    input: self.input.clone(),
                    finished: false,
                    _tracked: self.tracker.token(),
                },
            )
        };
        tokio::spawn(async move {
            let (registered, guard) = guard;
            let mut work =
                Box::pin(AssertUnwindSafe(registered.retire_unaccepted()).catch_unwind());
            let result = work.as_mut().await;
            let destroyed = catch_unwind(AssertUnwindSafe(|| drop(work)));
            if matches!((result, destroyed), (Ok(Ok(())), Ok(()))) {
                let mut progress = actual
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                progress.retired = true;
                progress.retiring = false;
                drop(progress);
                guard.finish();
            }
        });
        Ok(())
    }
}
impl InitialSessionOperation {
    pub(super) fn retire_initial_preparation(&self, previous: &Arc<Scope>) {
        let Some(claim) = previous.frozen.claim() else {
            return;
        };
        let known_no_core = {
            let progress = previous
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            progress.worker_finished
                && !progress.worker_lost
                && !progress.selection_unknown
                && !progress.entered
                && progress.created.is_none()
                && progress.resolved.is_none()
                && matches!(progress.complete, Some(Reply::Error(_)))
                && (previous.registered_selection.is_none() || progress.selection_retired)
        };
        if !known_no_core {
            return;
        }
        let retired = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let key = format!("initial:{claim}");
            let same = state.preparations.get(&key).is_some_and(|record| {
                record
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .accepted
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_some_and(|scope| Arc::ptr_eq(&scope, previous))
            });
            if same {
                state.preparations.remove(&key)
            } else {
                None
            }
        };
        drop(retired); // Normal no-Core/native-retired old choice only, outside registry lock.
    }
    pub(super) async fn retire_unaccepted_after_join(self: &Arc<Self>) -> Result<(), String> {
        let records = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .preparations
            .values()
            .cloned()
            .collect::<Vec<_>>();
        drain_policy::drain_retirements(
            &self.tracker,
            &records,
            |actual| {
                self.retire_unaccepted(actual.clone(), true)
                    .map_err(|error| error.message)
            },
            |actual| {
                let progress = actual
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if progress.unknown
                    || !progress.finished
                    || (progress.accepted.is_none()
                        && !progress.retired
                        && progress.error.is_none())
                {
                    Err("client preparation retirement remains unobserved".into())
                } else {
                    Ok(())
                }
            },
        )
        .await
    }
}
impl ThreadRequestProcessor {
    pub(crate) async fn alab_check_client_preparation_origin(
        &self,
        context: &RequestContext,
    ) -> Result<(), JSONRPCErrorError> {
        self.alab_initial_session.actual()?.open()?;
        actual_origin(context)?
            .check_current()
            .await
            .map_err(|_| invalid_request("client preparation Native origin is not current"))
    }
    pub(crate) async fn alab_prepare_client_request(
        &self,
        context: RequestContext,
        claim: crate::outgoing_message::alab_initial_claim::PreparationClaim,
    ) -> Result<Completion, JSONRPCErrorError> {
        self.alab_initial_session
            .actual()?
            .prepare_client(&context, claim.0.as_ref())
            .await
    }
}

impl Scope {
    /// Original policy has returned and its future destruction was observed.
    /// The original Guard still tracks THIS worker through Native retirement.
    pub(super) async fn retire_failed_selection(self: &Arc<Self>) {
        let held = {
            let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            if progress.worker_lost
                || !progress.worker_finished
                || !matches!(progress.staged, Some(Reply::Error(_)))
            {
                return;
            }
            progress.held_selection.clone()
        };
        let Some(held) = held else {
            return;
        };
        let Some(owner) = self.operation.upgrade() else {
            return;
        };
        let (entered, born, resolved) = {
            let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            (
                progress.entered,
                progress.created.clone(),
                progress.resolved.is_some(),
            )
        };
        let check = {
            let scope = self.clone();
            let owner = owner.clone();
            let expected = born.clone();
            move || {
                if !scope.prior_selection.unchanged(&owner) {
                    return Err("prior selected owner changed during failed construction".into());
                }
                let progress = scope
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if progress.worker_lost
                    || !progress.worker_finished
                    || progress.selection_unknown
                    || !matches!(progress.staged, Some(Reply::Error(_)))
                {
                    return Err("original construction completion is not observed".into());
                }
                match &expected {
                    None if !progress.entered
                        && progress.created.is_none()
                        && progress.resolved.is_none() =>
                    {
                        Ok(())
                    }
                    Some((id, actual))
                        if progress.entered
                            && progress.resolved.is_none()
                            && progress.created.as_ref().is_some_and(
                                |(captured_id, captured)| {
                                    captured_id == id && Arc::ptr_eq(captured, actual)
                                },
                            )
                            && actual.wait_until_terminated().now_or_never().is_some() =>
                    {
                        Ok(())
                    }
                    _ => Err("failed construction retains existing/unknown Core custody".into()),
                }
            }
        };
        let result = if !entered && born.is_none() && !resolved {
            held.retire_before_core(check).await
        } else if entered && !resolved {
            if let Some((_, actual)) = &born {
                use crate::request_processors::thread_lifecycle::ThreadShutdownResult;
                use crate::request_processors::thread_lifecycle::wait_for_thread_shutdown;
                if !matches!(
                    wait_for_thread_shutdown(actual).await,
                    ThreadShutdownResult::Complete
                ) {
                    return;
                }
                held.retire_after_new_core_terminated(check).await
            } else {
                return;
            }
        } else {
            // A resolved existing Core is not a failed NEW allocation. Keep its
            // ticket/lease closed for real UI reconciliation or explicit cancel.
            return;
        };
        let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if result.is_ok() {
            progress.selection_retired = true;
        } else {
            progress.selection_unknown = true;
        }
        drop(progress);
        if result.is_err() {
            self.input.fail(std::io::Error::other(
                "failed construction retirement remains unobserved",
            ));
        }
    }
}
