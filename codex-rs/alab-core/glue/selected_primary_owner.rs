// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source UI-selection custody, never Native permission or manager Current.
//! Observation handles hold weak lease identity. Only provisional Scope,
//! current selected slot and an unobserved transition own actual leases.
use super::super::super::thread_lifecycle::alab_primary_lifetime::PrimaryLease;
use super::super::super::thread_lifecycle::alab_primary_lifetime::WeakPrimaryLease;
use super::*;
use codex_alab_aterm::frontend_control::retained_service::HeldSourceTransition;
use codex_alab_aterm::frontend_control::retained_service::SelectionAck;
use codex_alab_aterm::frontend_control::retained_service::SelectionCommit;
use codex_alab_aterm::frontend_control::retained_service::SelectionFrontier;
use codex_alab_aterm::frontend_control::retained_service::SelectionReleased;
use std::sync::MutexGuard;

struct Version(u64);
struct Record {
    scope: Arc<Scope>,
    id: ThreadId,
    actual: Arc<CodexThread>,
    lease: WeakPrimaryLease,
}
struct OwnedSelected {
    record: Arc<Record>,
    _lease: PrimaryLease,
}
struct Transition {
    expected: Arc<Version>,
    next: Arc<Version>,
    prior: Mutex<Option<Arc<OwnedSelected>>>,
    candidate: Arc<Record>,
    candidate_lease: Mutex<Option<PrimaryLease>>,
    uncertain: std::sync::atomic::AtomicBool,
}
pub(super) struct SelectionState {
    runtime: String,
    version: Arc<Version>,
    selected: Option<Arc<OwnedSelected>>,
    installing: Option<Arc<Transition>>,
    retired: bool,
}
impl SelectionState {
    pub(super) fn new() -> Self {
        Self {
            runtime: InitialSessionPlan::fresh_claim(),
            version: Arc::new(Version(0)),
            selected: None,
            installing: None,
            retired: false,
        }
    }
    pub(super) fn take_after_threads(&mut self) -> RetiredSelection {
        self.retired = true;
        RetiredSelection {
            _selected: self.selected.take(),
            _installing: self.installing.take(),
        }
    }
}
pub(super) struct RetiredSelection {
    _selected: Option<Arc<OwnedSelected>>,
    _installing: Option<Arc<Transition>>,
}
#[derive(Clone)]
pub(super) struct PriorSelection {
    version: Arc<Version>,
    selected: Option<Weak<Record>>,
}
impl PriorSelection {
    pub(super) fn unchanged(&self, operation: &InitialSessionOperation) -> bool {
        let slot = operation
            .selected_primary
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        !slot.retired
            && slot.installing.is_none()
            && Arc::ptr_eq(&slot.version, &self.version)
            && match (&slot.selected, &self.selected) {
                (None, None) => true,
                (Some(actual), Some(expected)) => expected
                    .upgrade()
                    .is_some_and(|expected| Arc::ptr_eq(&actual.record, &expected)),
                _ => false,
            }
    }
}
/// Empty means no observed UI installation, NOT absence of a live Core.
#[derive(Clone)]
pub(crate) struct EmptyPrimary {
    operation: Arc<InitialSessionOperation>,
    runtime: Arc<str>,
    version: Arc<Version>,
}
impl EmptyPrimary {
    pub(crate) fn runtime_id_data(&self) -> &str {
        &self.runtime
    }
    pub(crate) fn version_data(&self) -> u64 {
        self.version.0
    }
    pub(crate) fn belongs_to(&self, input: &Incoming) -> bool {
        self.operation.input == *input
    }
    pub(crate) fn same_version(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.operation, &other.operation) && Arc::ptr_eq(&self.version, &other.version)
    }
    pub(crate) fn recheck_owned(&self) -> Result<(), JSONRPCErrorError> {
        drop(self.lock_unchanged()?);
        Ok(())
    }
    /// Wait only for an actual Source installation already in progress, then
    /// recheck the real selected resource. SDK Hold absence is never success.
    /// Exact Unknown stays closed; owner stop and real installation completion
    /// wake the original observer without polling/yield/retry assumptions.
    pub(crate) async fn wait_selected_handoff(
        &self,
    ) -> Result<SelectionHandoff, JSONRPCErrorError> {
        loop {
            let changed = self.operation.selection_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.source_stopped() {
                return Ok(SelectionHandoff::Stopped);
            }
            if let Err(error) = self.operation.selection_open() {
                if self.source_stopped() {
                    return Ok(SelectionHandoff::Stopped);
                }
                return Err(error);
            }
            let selected = {
                let slot = self
                    .operation
                    .selected_primary
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if slot.retired {
                    return Ok(SelectionHandoff::Stopped);
                }
                if slot.version.0 < self.version.0 {
                    return Err(invalid_request(
                        "configured primary handoff changed owner/version",
                    ));
                }
                if let Some(installing) = &slot.installing {
                    if !Arc::ptr_eq(&installing.expected, &slot.version)
                        || installing
                            .uncertain
                            .load(std::sync::atomic::Ordering::Acquire)
                    {
                        return Err(invalid_request("actual primary installation is unobserved"));
                    }
                    None
                } else {
                    let actual = slot.selected.as_ref().ok_or_else(|| {
                        invalid_request("configured custody has no completed Source handoff")
                    })?;
                    // Validate and retain the ACTUAL selected owner under the
                    // same slot guard. A later legitimate install cannot turn
                    // this completed handoff into an obsolete-observer error.
                    if let Err(error) = actual.record.recheck_owned(&self.operation) {
                        if self.source_stopped() {
                            return Ok(SelectionHandoff::Stopped);
                        }
                        return Err(error);
                    }
                    Some(CompletedSelectionHandoff {
                        operation: self.operation.clone(),
                        _selected: actual.clone(),
                    })
                }
            };
            if let Some(selected) = selected {
                return Ok(SelectionHandoff::Selected(selected));
            }
            tokio::select! {
                biased;
                () = self.operation.input.withdrawn() => return Ok(SelectionHandoff::Stopped),
                () = changed.as_mut() => {},
            }
        }
    }

    pub(crate) fn source_stopped(&self) -> bool {
        self.operation.input.is_stopped()
            || self
                .operation
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .closed
    }

    /// SDK mutation semaphore/state -> this synchronous Source guard; no await.
    pub(crate) fn lock_unchanged(&self) -> Result<EmptyGuard<'_>, JSONRPCErrorError> {
        self.operation.selection_open()?;
        let slot = self
            .operation
            .selected_primary
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.retired
            || slot.installing.is_some()
            || slot.selected.is_some()
            || !Arc::ptr_eq(&slot.version, &self.version)
            || slot.runtime != self.runtime.as_ref()
        {
            return Err(invalid_request(
                "configured empty-primary selection changed",
            ));
        }
        self.operation.selection_open()?;
        Ok(EmptyGuard { _slot: slot })
    }
}
/// Private Source handoff observation, not a reusable current-primary or
/// Native/CoreCurrent/model receipt. Holds the SAME actual selected lease only
/// through termination of the obsolete initial Empty keeper.
pub(crate) enum SelectionHandoff {
    Selected(CompletedSelectionHandoff),
    // Exact original Source operation/slot stop, NOT Native retirement or
    // effects settlement. Its original workers/ledger remain drain-owned.
    Stopped,
}
pub(crate) struct CompletedSelectionHandoff {
    operation: Arc<InitialSessionOperation>,
    _selected: Arc<OwnedSelected>,
}
impl CompletedSelectionHandoff {
    pub(crate) fn belongs_to(&self, input: &Incoming) -> bool {
        self.operation.input == *input
    }
}

pub(crate) struct EmptyGuard<'a> {
    _slot: MutexGuard<'a, SelectionState>,
}
#[derive(Clone)]
pub(crate) enum CurrentPrimary {
    Empty(EmptyPrimary),
    Selected(SelectedPrimary),
}
#[derive(Clone)]
pub(crate) struct SelectedPrimary {
    operation: Arc<InitialSessionOperation>,
    record: Arc<Record>,
    version: Arc<Version>,
}
impl SelectedPrimary {
    pub(crate) fn thread_id(&self) -> ThreadId {
        self.record.id
    }
    pub(crate) fn thread(&self) -> &Arc<CodexThread> {
        &self.record.actual
    }
    pub(crate) fn lease(&self) -> Option<PrimaryLease> {
        self.record.lease.upgrade()
    }
    pub(crate) fn belongs_to(&self, input: &Incoming) -> bool {
        self.operation.input == *input
    }
    pub(crate) fn backend_task_tools(&self) -> Result<bool, JSONRPCErrorError> {
        self.recheck_owned()?;
        self.record
            .scope
            .task_tools_observed(self.record.id, &self.record.actual)
    }
    pub(crate) fn recheck_owned(&self) -> Result<(), JSONRPCErrorError> {
        self.operation.selection_open()?;
        {
            let slot = self
                .operation
                .selected_primary
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if slot.retired
                || slot.installing.is_some()
                || !Arc::ptr_eq(&slot.version, &self.version)
                || !slot
                    .selected
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(&current.record, &self.record))
            {
                return Err(invalid_request(
                    "selected primary changed or installation unobserved",
                ));
            }
        }
        self.record.recheck_owned(&self.operation)
    }
    pub(crate) async fn recheck(&self) -> Result<(), JSONRPCErrorError> {
        self.recheck_owned()?;
        self.record.recheck_current(&self.operation).await?;
        self.recheck_owned()
    }
}
#[derive(Clone)]
pub(crate) struct PrimaryCandidate {
    operation: Arc<InitialSessionOperation>,
    record: Arc<Record>,
}
impl PrimaryCandidate {
    pub(super) fn scope_for_side(&self) -> Arc<Scope> {
        self.record.scope.clone()
    }
    pub(crate) fn side_boundary(
        &self,
    ) -> Result<Option<codex_core::RecordedSideBoundary>, JSONRPCErrorError> {
        self.recheck_owned()?;
        if let Some(actual) = self
            .record
            .scope
            .recorded_side_boundary(&self.record.actual)?
        {
            return Ok(Some(actual));
        }
        if !self.is_primary_presentation() {
            return self.operation.retained_side_boundary(&self.record.actual);
        }
        Ok(None)
    }
    pub(crate) fn thread_id(&self) -> ThreadId {
        self.record.id
    }
    pub(crate) fn thread(&self) -> &Arc<CodexThread> {
        &self.record.actual
    }
    pub(crate) fn lease(&self) -> Option<PrimaryLease> {
        self.record.lease.upgrade()
    }
    pub(crate) fn belongs_to(&self, input: &Incoming) -> bool {
        self.operation.input == *input
    }
    pub(crate) fn is_primary_presentation(&self) -> bool {
        self.record.scope.frozen.retains_candidate()
    }
    pub(crate) fn backend_task_tools(&self) -> Result<bool, JSONRPCErrorError> {
        self.recheck_owned()?;
        self.record
            .scope
            .task_tools_observed(self.record.id, &self.record.actual)
    }
    pub(crate) fn recheck_owned(&self) -> Result<(), JSONRPCErrorError> {
        self.record.recheck_owned(&self.operation)
    }
    pub(crate) async fn recheck(&self) -> Result<(), JSONRPCErrorError> {
        self.record.recheck_current(&self.operation).await
    }
    pub(crate) fn source_selection(&self) -> Result<Arc<HeldSourceTransition>, JSONRPCErrorError> {
        self.recheck_owned()?;
        self.record
            .scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .held_selection
            .clone()
            .ok_or_else(|| invalid_request("candidate has no actual Source transition"))
    }
    pub(crate) fn source_offer(
        &self,
    ) -> Result<crate::outgoing_message::alab_initial_claim::PreparationOffer, JSONRPCErrorError>
    {
        self.recheck_owned()?;
        self.record
            .scope
            .prepared_offer
            .clone()
            .ok_or_else(|| invalid_request("candidate has no actual prepared operation"))
    }
    fn prepare_transition(&self) -> Result<Arc<Transition>, JSONRPCErrorError> {
        self.recheck_owned()?;
        if !self.is_primary_presentation() {
            return Err(invalid_request(
                "side conversation cannot replace selected primary",
            ));
        }
        let lease = self
            .lease()
            .ok_or_else(|| invalid_request("candidate lease owner ended"))?;
        let slot = self
            .operation
            .selected_primary
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.retired || slot.installing.is_some() {
            return Err(invalid_request("primary installation unobserved"));
        }
        let next = slot
            .version
            .0
            .checked_add(1)
            .ok_or_else(|| internal_error("primary version exhausted"))?;
        Ok(Arc::new(Transition {
            expected: slot.version.clone(),
            next: Arc::new(Version(next)),
            prior: Mutex::new(slot.selected.clone()),
            candidate: self.record.clone(),
            candidate_lease: Mutex::new(Some(lease)),
            uncertain: std::sync::atomic::AtomicBool::new(false),
        }))
    }
    pub(crate) fn prepare_source_install(
        &self,
        frontier: &SelectionFrontier,
    ) -> Result<PreparedSelectionInstall, JSONRPCErrorError> {
        let held = self.source_selection()?;
        if !frontier.same_transition(&held)
            || !frontier.goal_gate().same(&held.goal_gate())
            || !self.record.scope.prior_selection.unchanged(&self.operation)
        {
            return Err(invalid_request(
                "candidate Source resource or prior selection changed",
            ));
        }
        Ok(PreparedSelectionInstall {
            candidate: self.clone(),
            frontier: frontier.clone(),
            transition: self.prepare_transition()?,
        })
    }
    pub(crate) async fn finish_source_selection(
        &self,
        released: &SelectionReleased,
    ) -> Result<(), JSONRPCErrorError> {
        let held = self
            .record
            .scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .held_selection
            .clone()
            .ok_or_else(|| invalid_request("candidate Source custody missing"))?;
        if !released.same_transition(&held) {
            return Err(invalid_request("Source release resource disagrees"));
        }
        held.finish_selection(released).await.map_err(|_| {
            self.record
                .scope
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .selection_unknown = true;
            self.operation.input.fail(std::io::Error::other(
                "Source selection retirement unobserved",
            ));
            invalid_request("Source selection retirement unobserved")
        })?;
        self.record
            .scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .selection_retired = true;
        if !self.is_primary_presentation() {
            // Genuine SelectionCommit/release AND observed Native finish above
            // precede this Source-only ownership move. It opens no admission.
            self.operation.retain_side_view(
                &self.record.scope,
                self.record.id,
                &self.record.actual,
            )?;
        }
        Ok(())
    }
}
impl Record {
    fn recheck_owned(&self, operation: &InitialSessionOperation) -> Result<(), JSONRPCErrorError> {
        operation.selection_open()?;
        let state = operation
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !state
            .accepted
            .as_ref()
            .into_iter()
            .chain(state.clients.values())
            .any(|scope| Arc::ptr_eq(scope, &self.scope))
        {
            return Err(invalid_request("candidate has no actual accepted Scope"));
        }
        let progress = self
            .scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if progress.worker_lost
            || !progress.worker_finished
            || (progress.entered && progress.created.is_none())
            || !matches!(
                progress.complete,
                Some(Reply::Start(..) | Reply::Fork(..) | Reply::Resume(..))
            )
            || !progress
                .created
                .as_ref()
                .or(progress.resolved.as_ref())
                .is_some_and(|(id, actual)| *id == self.id && Arc::ptr_eq(actual, &self.actual))
            || !progress.retained_primary().is_some_and(|lease| {
                self.lease
                    .upgrade()
                    .is_some_and(|same| same.same_owner(&lease))
                    && lease.is_for(&self.actual)
            })
            || self.actual.has_shutdown_status()
        {
            return Err(invalid_request("actual candidate completion/lease changed"));
        }
        Ok(())
    }
    async fn recheck_current(
        &self,
        operation: &InitialSessionOperation,
    ) -> Result<(), JSONRPCErrorError> {
        self.recheck_owned(operation)?;
        if !matches!(
            operation
                .processor
                .thread_manager
                .thread_registration(self.id, &self.actual)
                .await,
            codex_core::ThreadRegistration::Current
        ) {
            return Err(invalid_request("actual candidate runtime withdrew"));
        }
        self.recheck_owned(operation)
    }
}
fn install_local(
    candidate: &PrimaryCandidate,
    transition: &Arc<Transition>,
) -> Result<(), JSONRPCErrorError> {
    candidate.recheck_owned()?;
    let mut slot = candidate
        .operation
        .selected_primary
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if slot.retired
        || transition
            .uncertain
            .load(std::sync::atomic::Ordering::Acquire)
        || !Arc::ptr_eq(&slot.version, &transition.expected)
        || slot
            .installing
            .as_ref()
            .is_some_and(|actual| !Arc::ptr_eq(actual, transition))
        || transition
            .candidate_lease
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
    {
        return Err(invalid_request("stale or unobserved primary install"));
    }
    candidate.operation.selection_open()?;
    if candidate.thread().has_shutdown_status() {
        return Err(invalid_request("candidate shut down"));
    }
    slot.installing = Some(transition.clone());
    drop(slot);
    candidate.operation.selection_changed.notify_waiters();
    Ok(())
}
fn finish_install(
    candidate: &PrimaryCandidate,
    transition: &Arc<Transition>,
) -> Result<SelectedPrimary, JSONRPCErrorError> {
    let retired = {
        let mut slot = candidate
            .operation
            .selected_primary
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.retired
            || !Arc::ptr_eq(&slot.version, &transition.expected)
            || !slot
                .installing
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, transition))
        {
            return Err(invalid_request(
                "observed release lost original primary slot",
            ));
        }
        let lease = transition
            .candidate_lease
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| internal_error("observed transition lost actual lease"))?;
        slot.version = transition.next.clone();
        let old = slot.selected.replace(Arc::new(OwnedSelected {
            record: transition.candidate.clone(),
            _lease: lease,
        }));
        (old, slot.installing.take())
    };
    let provisional = {
        let mut progress = candidate
            .record
            .scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        progress.primary_weak = Some(candidate.record.lease.clone());
        progress.primary.take()
    };
    let prior = transition
        .prior
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    drop(provisional);
    drop(retired);
    drop(prior);
    candidate.operation.selection_changed.notify_waiters();
    Ok(SelectedPrimary {
        operation: candidate.operation.clone(),
        record: candidate.record.clone(),
        version: transition.next.clone(),
    })
}
/// Covers the actual SDK release await after local installation can begin.
/// A lost/error observer retains both owners and publishes exact Unknown only
/// if THIS transition is still installed. No unaccepted attempt is dirtied.
struct InstallObservation {
    operation: Arc<InitialSessionOperation>,
    transition: Arc<Transition>,
    complete: bool,
}
impl InstallObservation {
    fn new(candidate: &PrimaryCandidate, transition: &Arc<Transition>) -> Self {
        Self {
            operation: candidate.operation.clone(),
            transition: transition.clone(),
            complete: false,
        }
    }
    fn finish(mut self) {
        self.complete = true;
    }
}
impl Drop for InstallObservation {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        let installed = {
            let slot = self
                .operation
                .selected_primary
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let installed = slot
                .installing
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, &self.transition));
            if installed {
                self.transition
                    .uncertain
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            installed
        };
        if installed {
            self.operation.selection_changed.notify_waiters();
        }
    }
}

pub(crate) struct PreparedSelectionInstall {
    candidate: PrimaryCandidate,
    frontier: SelectionFrontier,
    transition: Arc<Transition>,
}
impl PreparedSelectionInstall {
    pub(crate) async fn release_committed(
        &self,
        ack: &SelectionAck,
        committed: &SelectionCommit,
        check: impl FnOnce() -> Result<(), String>,
    ) -> Result<SelectionReleased, JSONRPCErrorError> {
        let held = self.candidate.source_selection()?;
        if !ack.same_frontier(&self.frontier)
            || !committed.same_frontier(&self.frontier)
            || !self.frontier.same_transition(&held)
        {
            return Err(invalid_request("S2 install resource disagrees"));
        }
        self.candidate.recheck().await?;
        let observation = InstallObservation::new(&self.candidate, &self.transition);
        let released = ack
            .release_committed(&held, committed, || {
                check()?;
                install_local(&self.candidate, &self.transition)
                    .map_err(|_| "actual Source primary ownership changed".into())
            })
            .await
            .map_err(|_| invalid_request("S2 release unobserved; both owners retained"))?;
        finish_install(&self.candidate, &self.transition)?;
        observation.finish();
        Ok(released)
    }
}
impl Progress {
    pub(super) fn retained_primary(&self) -> Option<PrimaryLease> {
        self.primary.clone().or_else(|| {
            self.primary_weak
                .as_ref()
                .and_then(WeakPrimaryLease::upgrade)
        })
    }
}
impl InitialSessionOperation {
    fn selection_open(&self) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        if self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closed
        {
            return Err(invalid_request("configured primary owner closed"));
        }
        Ok(())
    }
    pub(super) fn snapshot_selection(&self) -> Result<PriorSelection, JSONRPCErrorError> {
        let slot = self
            .selected_primary
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.retired || slot.installing.is_some() {
            return Err(invalid_request("primary install unobserved"));
        }
        Ok(PriorSelection {
            version: slot.version.clone(),
            selected: slot
                .selected
                .as_ref()
                .map(|owned| Arc::downgrade(&owned.record)),
        })
    }
    pub(crate) fn current_primary(self: &Arc<Self>) -> Result<CurrentPrimary, JSONRPCErrorError> {
        self.selection_open()?;
        let slot = self
            .selected_primary
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.retired || slot.installing.is_some() {
            return Err(invalid_request("primary install unobserved"));
        }
        Ok(match &slot.selected {
            Some(owned) => CurrentPrimary::Selected(SelectedPrimary {
                operation: self.clone(),
                record: owned.record.clone(),
                version: slot.version.clone(),
            }),
            None => CurrentPrimary::Empty(EmptyPrimary {
                operation: self.clone(),
                runtime: slot.runtime.clone().into(),
                version: slot.version.clone(),
            }),
        })
    }
    pub(crate) async fn primary_candidate(
        self: &Arc<Self>,
        data: &str,
    ) -> Result<PrimaryCandidate, JSONRPCErrorError> {
        self.selection_open()?;
        let scope = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .accepted
                .iter()
                .chain(state.clients.values())
                .find(|scope| scope.candidate == data)
                .cloned()
                .ok_or_else(|| invalid_request("actual candidate unavailable"))?
        };
        let mut changed = scope.completed.subscribe();
        loop {
            let pending = {
                let p = scope
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                !p.worker_finished && !p.worker_lost
            };
            if !pending {
                break;
            }
            tokio::select! { biased; () = self.input.withdrawn() => return Err(invalid_request("candidate owner withdrew")),
            result = changed.changed() => result.map_err(|_| internal_error("candidate observation unavailable"))?, }
        }
        let (id, actual, lease) = {
            let progress = scope
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if progress.worker_lost
                || !progress.worker_finished
                || (progress.entered && progress.created.is_none())
            {
                return Err(invalid_request("candidate completion unobserved"));
            }
            let (id, actual) = progress
                .created
                .clone()
                .or_else(|| progress.resolved.clone())
                .ok_or_else(|| invalid_request("candidate Core missing"))?;
            let lease = progress
                .retained_primary()
                .filter(|lease| lease.is_for(&actual))
                .ok_or_else(|| invalid_request("candidate lease missing"))?;
            (id, actual, lease)
        };
        let candidate = PrimaryCandidate {
            operation: self.clone(),
            record: Arc::new(Record {
                scope,
                id,
                actual,
                lease: lease.downgrade(),
            }),
        };
        candidate.recheck().await?;
        Ok(candidate)
    }
}
impl InitialSlot {
    pub(crate) fn current_primary(&self) -> Result<CurrentPrimary, JSONRPCErrorError> {
        self.0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid_request("actual configured owner unavailable"))?
            .current_primary()
    }
    pub(crate) async fn source_candidate(
        &self,
        claim: &str,
    ) -> Result<PrimaryCandidate, JSONRPCErrorError> {
        self.0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid_request("actual Source owner unavailable"))?
            .source_candidate(claim)
            .await
    }
}
impl InitialSessionOperation {
    pub(super) async fn source_candidate(
        self: &Arc<Self>,
        claim: &str,
    ) -> Result<PrimaryCandidate, JSONRPCErrorError> {
        let operation = self;
        let scope = {
            let state = operation
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state
                .accepted
                .iter()
                .chain(state.clients.values())
                .find(|scope| {
                    scope
                        .prepared_offer
                        .as_ref()
                        .is_some_and(|offer| offer.claim == claim)
                })
                .cloned()
                .ok_or_else(|| invalid_request("actual accepted Source selection unavailable"))?
        };
        if scope
            .prepared_offer
            .as_ref()
            .is_none_or(|offer| offer.claim != claim)
            || scope.registered_selection.is_none()
        {
            return Err(invalid_request(
                "candidate has no actual accepted reservation",
            ));
        }
        operation.primary_candidate(&scope.candidate).await
    }
}
#[cfg(test)]
#[path = "selected_primary_owner_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "selected_handoff_wait_tests.rs"]
mod handoff_wait_tests;
