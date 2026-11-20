// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual same-session Runtime/Source projection custody. Local status is an
//! inhibitor/retirement fact, never Native permission or a selected Core grant.
use super::*;
use codex_alab_tui::frontend_control::FrontendFrontier;
use codex_alab_tui::frontend_control::FrontendSelectionFrontier;
use codex_app_server_client::ClientConstructionCheckpoint;
use codex_app_server_client::ClientConstructionOffer;
use codex_app_server_client::ClientConstructionOperation;
use codex_app_server_client::InitialConstructionCheckpoint;
use codex_app_server_client::InitialConstructionOperation;
use codex_app_server_client::retained_frontier;
use codex_app_server_protocol::RetainedFrontierNotification;
use codex_app_server_protocol::RetainedFrontierOutcome;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

#[path = "owned_local_ui_frontier_kind.rs"]
pub(crate) mod kind;
use kind::Native;
use kind::Receipt;

const ACTIVE: u8 = 0;
const COMPLETE: u8 = 1;
const UNKNOWN: u8 = 2;
/// Delivery keeps this tiny allocation even after its Weak<Pending> expires.
/// Only genuine commit completion changes Active→Complete. Configured
/// completion remains a startup inhibitor; Drop never completes either kind.
pub(crate) struct UiStatus(AtomicU8);
impl UiStatus {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self(AtomicU8::new(ACTIVE)))
    }
    pub(crate) fn is_complete(&self) -> bool {
        self.0.load(Ordering::Acquire) == COMPLETE
    }
    pub(crate) fn is_active(&self) -> bool {
        self.0.load(Ordering::Acquire) == ACTIVE
    }
    pub(crate) fn unknown(&self) {
        let _previous =
            self.0
                .compare_exchange(ACTIVE, UNKNOWN, Ordering::AcqRel, Ordering::Acquire);
    }
    pub(crate) fn finish(&self) -> Result<()> {
        self.0
            .compare_exchange(ACTIVE, COMPLETE, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| eyre!("UI completion became uncertain"))?;
        Ok(())
    }
}
#[derive(Clone)]
pub(crate) enum ConstructionOwner {
    Initial(InitialConstructionOperation),
    Later(ClientConstructionOperation),
    Caller(Arc<crate::owned_local_frontend::ConstructionRecord>),
}
pub(crate) enum ConstructionCheckpoint {
    Initial {
        _data: Box<InitialConstructionCheckpoint>,
    },
    Later {
        _data: Box<ClientConstructionCheckpoint>,
    },
}
struct ConstructionResource {
    _owner: ConstructionOwner,
    _checkpoint: ConstructionCheckpoint,
}
impl ConstructionOwner {
    pub(crate) fn prepared_offer(&self) -> Option<ClientConstructionOffer> {
        match self {
            Self::Initial(actual) => actual.prepared_offer(),
            Self::Later(actual) => actual.prepared_offer(),
            Self::Caller(actual) => actual.prepared_offer(),
        }
    }
    pub(crate) fn checkpoint(&self) -> ConstructionCheckpoint {
        match self {
            Self::Initial(actual) => ConstructionCheckpoint::Initial {
                _data: Box::new(actual.checkpoint()),
            },
            Self::Later(actual) => ConstructionCheckpoint::Later {
                _data: Box::new(actual.checkpoint()),
            },
            Self::Caller(actual) => actual.checkpoint(),
        }
    }
}
enum Project {
    Unstarted,
    Running,
    Observed(Box<Receipt>),
    Unknown,
}
enum Replay {
    Waiting,
    Installing,
    EndQueued,
    Applied,
    Committing,
    Complete,
    Unknown,
}
struct Progress {
    project: Project,
    _unmatched_receipt: Option<Receipt>,
    activation: Option<kind::Activation>,
    _unmatched_activations: Vec<kind::Activation>,
    tail: Option<RetainedFrontierNotification>,
    consumed_tail: bool,
    original_replay_finished: bool,
    replay: Replay,
}
pub(crate) struct Pending {
    tenure: crate::owned_local_frontend::FrontendTenure,
    handle: AppServerRequestHandle,
    native: Native,
    // Actual operation owner AND its closed checkpoint, retained before the
    // first projection await. The original constructor caller already owned
    // this SAME operation before polling its original construction future.
    _construction: Option<ConstructionResource>,
    status: Arc<UiStatus>,
    progress: Mutex<Progress>,
}
impl std::fmt::Debug for Pending {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RetainedUiPending(<same physical session; no permission>)")
    }
}
#[derive(Clone, Debug)]
pub(crate) struct ReplayEnd(Arc<Pending>);
/// This single installation token survives original history/config conversion.
pub(crate) struct ProjectionReplay(Arc<Pending>);
impl ProjectionReplay {
    pub(crate) fn pending(&self) -> &Arc<Pending> {
        &self.0
    }
    pub(crate) fn side_display_boundary(
        &self,
    ) -> Result<Option<codex_app_server_client::source_selection::SideDisplayBoundary>> {
        let state = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Project::Observed(receipt) = &state.project else {
            bail!("replay has no actual response")
        };
        if !self.0.native.matches(receipt) {
            bail!("replay changed actual route")
        }
        Ok(receipt.side_display_boundary().cloned())
    }
    pub(crate) fn is_primary_presentation(&self) -> Result<bool> {
        let state = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Project::Observed(receipt) = &state.project else {
            bail!("replay has no actual response");
        };
        if !self.0.native.matches(receipt) {
            bail!("replay changed its actual route");
        }
        Ok(receipt.is_primary_presentation())
    }
    pub(crate) fn apply_task_tool_observation(
        &self,
        session: &AppServerSession,
        started: &mut AppServerStartedThread,
    ) -> Result<()> {
        self.0.apply_task_tool_observation(session, started)
    }
    pub(crate) fn begin_install(
        &self,
        session: &AppServerSession,
        thread_data: &str,
    ) -> Result<Installation> {
        if !self.0.current_on(session) {
            bail!("retained replay changed its physical session");
        }
        let mut state = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Project::Observed(receipt) = &state.project else {
            bail!("retained replay lacks its actual response");
        };
        if !self.0.native.matches(receipt)
            || receipt.thread_id_data() != thread_data
            || !matches!(state.replay, Replay::Waiting)
        {
            bail!("retained replay changed its accepted projection");
        }
        state.replay = Replay::Installing;
        Ok(Installation {
            pending: self.0.clone(),
            complete: false,
        })
    }
}
pub(crate) struct Installation {
    pending: Arc<Pending>,
    complete: bool,
}
impl Installation {
    pub(crate) fn finish(mut self) -> Result<()> {
        let mut state = self
            .pending
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !matches!(state.replay, Replay::Installing) || !self.pending.status.is_active() {
            bail!("retained installation became uncertain");
        }
        state.original_replay_finished = true;
        state.replay = Replay::Waiting;
        self.complete = true;
        Ok(())
    }
}
impl Drop for Installation {
    fn drop(&mut self) {
        if !self.complete {
            self.pending.unknown();
        }
    }
}
impl Pending {
    fn apply_task_tool_observation(
        &self,
        session: &AppServerSession,
        started: &mut AppServerStartedThread,
    ) -> Result<()> {
        if !self.current_on(session) {
            bail!("task capability changed its physical session");
        }
        let receipt = {
            let state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            let Project::Observed(receipt) = &state.project else {
                bail!("task capability has no actual projection response");
            };
            if !self.native.matches(receipt)
                || receipt.thread_id_data() != started.session.thread_id.to_string()
            {
                bail!("task capability belongs to another actual converted thread");
            }
            (**receipt).clone()
        };
        // No progress guard is held while accessing delivery state. Genuine
        // false always replaces the historical cache for THIS physical tenure.
        self.tenure.observe_task_tools(&self.native, &receipt)?;
        started.task_tools_available = receipt.task_tools_available();
        Ok(())
    }

    fn new(session: &AppServerSession, native: FrontendFrontier) -> Result<Arc<Self>> {
        Self::with_resource(session, Native::Runtime(native), None)
    }
    fn new_source(
        session: &AppServerSession,
        native: FrontendSelectionFrontier,
        owner: ConstructionOwner,
    ) -> Result<Arc<Self>> {
        let prepared = owner
            .prepared_offer()
            .ok_or_else(|| eyre!("Source projection has no actual construction offer"))?;
        if prepared.source_operation != native.source_operation_data()
            || prepared.selection != native.selection_data()
        {
            bail!("Source projection changed its real construction reservation");
        }
        let checkpoint = owner.checkpoint();
        Self::with_resource(
            session,
            Native::Source(native),
            Some(ConstructionResource {
                _owner: owner,
                _checkpoint: checkpoint,
            }),
        )
    }
    fn with_resource(
        session: &AppServerSession,
        native: Native,
        construction: Option<ConstructionResource>,
    ) -> Result<Arc<Self>> {
        let tenure = session
            .retained_frontend()
            .ok_or_else(|| eyre!("retained UI requires its actual final delivery"))?;
        if !native.is_for_controller(tenure.controller())
            || native.service_data() != tenure.target.service_data()
        {
            bail!("retained UI changed its actual controller/service");
        }
        Ok(Arc::new(Self {
            tenure: tenure.clone(),
            handle: session.request_handle(),
            native,
            _construction: construction,
            status: UiStatus::new(),
            progress: Mutex::new(Progress {
                project: Project::Unstarted,
                _unmatched_receipt: None,
                activation: None,
                _unmatched_activations: Vec::new(),
                tail: None,
                consumed_tail: false,
                original_replay_finished: false,
                replay: Replay::Waiting,
            }),
        }))
    }
    pub(crate) fn ui_status(&self) -> Arc<UiStatus> {
        self.status.clone()
    }
    pub(crate) fn belongs_to(&self, tenure: &crate::owned_local_frontend::FrontendTenure) -> bool {
        tenure.same_delivery(&self.tenure)
            && self.native.is_for_controller(tenure.controller())
            && self.native.service_data() == tenure.target.service_data()
    }
    /// SAME current opaque Runtime/Source frontier before UI ACK; this supplies
    /// renderer resource binding only, never native permission or activation.
    pub(crate) fn image_frontier(&self, session: &AppServerSession) -> std::io::Result<Native> {
        if !self.current_on(session) {
            return Err(std::io::Error::other(
                "image projection changed physical delivery",
            ));
        }
        Ok(self.native.clone())
    }
    pub(crate) fn runtime_frontier(&self) -> Option<FrontendFrontier> {
        match &self.native {
            Native::Runtime(actual) => Some(actual.clone()),
            Native::Source(_) => None,
        }
    }
    pub(crate) fn same_native(&self, actual: &Native) -> bool {
        if !actual.is_for_controller(self.tenure.controller())
            || actual.service_data() != self.native.service_data()
            || actual.frontier_data() != self.native.frontier_data()
        {
            return false;
        }
        match (&self.native, actual) {
            (Native::Runtime(left), Native::Runtime(right)) => left.go_data() == right.go_data(),
            (Native::Source(left), Native::Source(right)) => {
                left.go_data() == right.go_data()
                    && left.source_operation_data() == right.source_operation_data()
                    && left.selection_data() == right.selection_data()
            }
            _ => false,
        }
    }
    pub(crate) fn expects_promotion(&self) -> bool {
        match &self.native {
            Native::Runtime(actual) => actual.go_data().is_some(),
            Native::Source(actual) => actual.go_data().is_some(),
        }
    }
    fn unknown(&self) {
        self.status.unknown();
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if !matches!(state.project, Project::Observed(_)) {
            state.project = Project::Unknown;
        }
        state.replay = Replay::Unknown;
    }
    fn begin_original_project(self: &Arc<Self>) -> Result<ProjectGuard> {
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if !matches!(state.project, Project::Unstarted) || !self.status.is_active() {
            bail!("retained projection already attempted");
        }
        state.project = Project::Running;
        Ok(ProjectGuard {
            pending: Arc::downgrade(self),
            finished: false,
        })
    }
    fn observe_original_project(&self, receipt: Receipt, mut guard: ProjectGuard) -> Result<()> {
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if !matches!(state.project, Project::Running)
            || !self.status.is_active()
            || !self.native.matches(&receipt)
            || state
                .tail
                .as_ref()
                .is_some_and(|tail| !matches_receipt(tail, &receipt))
        {
            state._unmatched_receipt = Some(receipt);
            bail!("retained projection changed before its actual response");
        }
        state.project = Project::Observed(Box::new(receipt));
        guard.finished = true;
        Ok(())
    }
    pub(crate) fn received_tail(&self, tail: &RetainedFrontierNotification) -> Result<()> {
        if tail.frontier != self.native.frontier_data() {
            return Ok(());
        }
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state.tail.as_ref().is_some_and(|original| original != tail)
            || matches!(&state.project, Project::Observed(receipt) if !self.native.matches(receipt) || !matches_receipt(tail, receipt))
        {
            state.replay = Replay::Unknown;
            self.status.unknown();
            bail!("retained tail changed its actual projection");
        }
        state.tail = Some(tail.clone());
        Ok(())
    }
    pub(crate) fn consumed_tail(&self, tail: &RetainedFrontierNotification) -> Result<()> {
        if tail.frontier != self.native.frontier_data() {
            return Ok(());
        }
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state.tail.as_ref() != Some(tail) {
            state.replay = Replay::Unknown;
            self.status.unknown();
            bail!("retained tail bypassed its actual session");
        }
        state.consumed_tail = true;
        Ok(())
    }
    pub(crate) fn terminal_failure(&self, tail: &RetainedFrontierNotification) -> Result<bool> {
        if tail.frontier != self.native.frontier_data() {
            return Ok(false);
        }
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state.tail.as_ref() != Some(tail) {
            state.replay = Replay::Unknown;
            self.status.unknown();
            bail!("retained failure bypassed its actual session");
        }
        if matches!(&tail.outcome, RetainedFrontierOutcome::Failed { .. }) {
            state.replay = Replay::Unknown;
            self.status.unknown();
            return Ok(true);
        }
        Ok(false)
    }
    pub(crate) fn queue_end(self: &Arc<Self>) -> Option<ReplayEnd> {
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.status.is_active()
            || !state.original_replay_finished
            || !state.consumed_tail
            || !matches!(state.replay, Replay::Waiting)
        {
            return None;
        }
        let Project::Observed(receipt) = &state.project else {
            return None;
        };
        let tail = state.tail.as_ref()?;
        if !self.native.matches(receipt) || !matches_receipt(tail, receipt) {
            state.replay = Replay::Unknown;
            self.status.unknown();
            return None;
        }
        state.replay = Replay::EndQueued;
        Some(ReplayEnd(self.clone()))
    }
    pub(crate) fn holds_submission(&self) -> bool {
        !self.status.is_complete()
    }
    pub(crate) fn current_on(&self, session: &AppServerSession) -> bool {
        session
            .retained_frontend()
            .is_some_and(|tenure| self.belongs_to(tenure) && tenure.is_ui_frontier(self))
    }
    /// Only the original ordered replay End permits a callback fact cut.
    /// This returns the real private receipt, never synthesizes one from IDs.
    pub(crate) fn callback_receipt(&self, session: &AppServerSession) -> Result<Option<Receipt>> {
        if !self.current_on(session) {
            bail!("callback projection changed its physical session");
        }
        let state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        let Project::Observed(receipt) = &state.project else {
            bail!("callback projection lacks original response");
        };
        if matches!(state.replay, Replay::Unknown) {
            bail!("callback UI replay outcome remains uncertain");
        }
        if !matches!(
            state.replay,
            Replay::Applied | Replay::Committing | Replay::Complete
        ) {
            return Ok(None);
        }
        if !state.original_replay_finished
            || !state.consumed_tail
            || !self.native.matches(receipt)
            || state.tail.as_ref().is_none_or(|tail| {
                !matches_receipt(tail, receipt)
                    || !matches!(&tail.outcome, RetainedFrontierOutcome::Ready {})
            })
        {
            bail!("callback cut precedes the actual ordered replay End");
        }
        Ok(Some((**receipt).clone()))
    }
    /// A post-commit semantic ACK uses only the exact retained private result.
    /// A failed/cancelled Commit cannot acquire this surface via a UI flag.
    pub(crate) fn callback_activation(
        &self,
        session: &AppServerSession,
    ) -> Result<Option<kind::Activation>> {
        if !self.current_on(session) {
            bail!("callback commit changed its physical session");
        }
        let state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.status.is_complete() || !matches!(state.replay, Replay::Complete) {
            return Ok(None);
        }
        let Project::Observed(receipt) = &state.project else {
            bail!("callback commit lost its actual response");
        };
        let actual = state
            .activation
            .as_ref()
            .ok_or_else(|| eyre!("callback commit lost its genuine result"))?;
        if !actual.matches(receipt) {
            bail!("callback commit result belongs to another projection");
        }
        Ok(Some(actual.clone()))
    }
    pub(crate) fn projection_target(&self) -> Option<(String, bool)> {
        let state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        let Project::Observed(receipt) = &state.project else {
            return None;
        };
        self.native.matches(receipt).then(|| {
            (
                receipt.thread_id_data().to_owned(),
                receipt.is_primary_presentation(),
            )
        })
    }
    pub(crate) fn begin_commit(
        self: &Arc<Self>,
        session: &AppServerSession,
    ) -> Result<Option<Commit>> {
        if !self.current_on(session) {
            bail!("retained UI lost its exact session");
        }
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(state.replay, Replay::Complete) && self.status.is_complete() {
            return Ok(None);
        }
        if matches!(state.replay, Replay::Unknown) || !self.status.is_active() {
            bail!("retained UI outcome remains unobserved");
        }
        if !matches!(state.replay, Replay::Applied) {
            return Ok(None);
        }
        let tail = state
            .tail
            .as_ref()
            .ok_or_else(|| eyre!("retained UI lost its terminal tail"))?;
        if !matches!(&tail.outcome, RetainedFrontierOutcome::Ready { .. }) {
            bail!("retained projection unavailable after response");
        }
        let Project::Observed(receipt) = &state.project else {
            bail!("retained UI lost its actual response");
        };
        if !self.native.matches(receipt) || !matches_receipt(tail, receipt) {
            bail!("retained commit changed its route/frontier");
        }
        let receipt = (**receipt).clone();
        state.replay = Replay::Committing;
        Ok(Some(Commit {
            pending: self.clone(),
            receipt,
            acknowledgment: AtomicU8::new(ACTIVE),
            complete: false,
        }))
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.status.unknown();
    }
}
impl ReplayEnd {
    pub(crate) fn apply(&self, session: &AppServerSession) -> Result<()> {
        if !self.0.current_on(session) {
            bail!("retained replay End changed physical session");
        }
        let mut state = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !matches!(state.replay, Replay::EndQueued) || !self.0.status.is_active() {
            bail!("retained replay End is stale");
        }
        state.replay = Replay::Applied;
        Ok(())
    }
}
pub(crate) struct Commit {
    pending: Arc<Pending>,
    receipt: Receipt,
    acknowledgment: AtomicU8,
    complete: bool,
}
impl Commit {
    pub(crate) fn thread_data(&self) -> &str {
        self.receipt.thread_id_data()
    }
    pub(crate) fn is_primary_presentation(&self) -> bool {
        self.receipt.is_primary_presentation()
    }
    pub(crate) async fn acknowledge(&self) -> Result<()> {
        self.acknowledgment
            .compare_exchange(ACTIVE, UNKNOWN, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| eyre!("UI acknowledgment already attempted"))?;
        let mut guard = AcknowledgmentGuard {
            commit: self,
            finished: false,
        };
        let activation = self
            .pending
            .native
            .commit(&self.receipt, &self.pending.tenure, &self.pending.handle)
            .await?;
        let side_view = activation.side_view_data();
        {
            let mut state = self
                .pending
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            // Retain the genuine private result BEFORE checking local state or
            // allowing another await. Cancellation cannot replace it by DATA.
            let matches = activation.matches(&self.receipt);
            let duplicate = state.activation.is_some();
            if duplicate {
                // This path is unreachable through the one-attempt guard;
                // retain both actual results if a future caller violates it.
                state._unmatched_activations.push(activation);
                self.pending.status.unknown();
                bail!("duplicate actual Source activation");
            }
            state.activation = Some(activation);
            if !matches
                || !matches!(state.replay, Replay::Committing)
                || !self.pending.status.is_active()
            {
                bail!("actual Source activation changed its UI owner");
            }
        }
        // The real private activation is already stored above even if this
        // bounded local DATA admission fails. Never drop an accepted outcome.
        if let Some(view) = side_view {
            self.pending.tenure.target.retain_side_view_data(view)?;
        }
        self.acknowledgment.store(COMPLETE, Ordering::Release);
        guard.finished = true;
        Ok(())
    }
    pub(crate) fn finish(mut self) -> Result<()> {
        if self.acknowledgment.load(Ordering::Acquire) != COMPLETE {
            bail!("actual UI acknowledgment was not observed");
        }
        {
            let mut state = self
                .pending
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !matches!(state.replay, Replay::Committing) || !self.pending.status.is_active() {
                bail!("retained UI changed during actual activation");
            }
            state.replay = Replay::Complete;
        }
        // No progress lock enters delivery state. Old committed promotion is
        // retired only after actual Native/Source commit, outside all locks.
        self.pending.tenure.finish_ui_frontier(&self.pending)?;
        if let Some(ConstructionResource {
            _owner: ConstructionOwner::Caller(actual),
            ..
        }) = &self.pending._construction
        {
            actual.applied(&self.pending.tenure)?;
        }
        self.pending.status.finish()?;
        self.complete = true;
        Ok(())
    }
}
struct AcknowledgmentGuard<'a> {
    commit: &'a Commit,
    finished: bool,
}
impl Drop for AcknowledgmentGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.commit.pending.unknown();
        }
    }
}
impl Drop for Commit {
    fn drop(&mut self) {
        if !self.complete {
            self.pending.unknown();
        }
    }
}
struct ProjectGuard {
    pending: std::sync::Weak<Pending>,
    finished: bool,
}
impl Drop for ProjectGuard {
    fn drop(&mut self) {
        if !self.finished
            && let Some(pending) = self.pending.upgrade()
        {
            pending.unknown();
        }
    }
}
fn matches_receipt(tail: &RetainedFrontierNotification, receipt: &Receipt) -> bool {
    tail.frontier == receipt.frontier_data()
        && tail.snapshot == receipt.snapshot_data()
        && tail.thread_id == receipt.thread_id_data()
}
impl AppServerSession {
    /// Replace only an actually finished prior commit. Expired Weak custody,
    /// installation, failed observation and Unknown never permit a new attempt.
    pub(crate) fn install_pending(&mut self, pending: Arc<Pending>) -> Result<()> {
        if self
            .alab_retained_ui
            .as_ref()
            .is_some_and(|old| !old.status.is_complete())
        {
            bail!("retained UI prior projection has not actually completed");
        }
        let tenure = self
            .retained_frontend()
            .ok_or_else(|| eyre!("retained UI lost final delivery"))?;
        if !pending.status.is_active() || !pending.belongs_to(tenure) {
            bail!("new projection changed actual delivery");
        }
        tenure.install_ui_frontier(&pending)?;
        let retired = self.alab_retained_ui.replace(pending);
        drop(retired);
        Ok(())
    }
    /// Transfer ONLY the same accepted local owner from its real request view
    /// to the live event session. No new SDK call, slot, status or RPC begins.
    pub(crate) fn adopt_pending(&mut self, pending: &Arc<Pending>) -> Result<()> {
        if !pending.current_on(self) {
            bail!("pending owner changed canonical physical delivery");
        }
        if self
            .alab_retained_ui
            .as_ref()
            .is_some_and(|old| !Arc::ptr_eq(old, pending) && !old.status.is_complete())
        {
            bail!("live session already retains another unfinished projection");
        }
        let retired = self.alab_retained_ui.replace(pending.clone());
        drop(retired); // No progress/delivery guard is held during old owner Drop.
        Ok(())
    }
    pub(crate) fn alab_retained_ui(&self) -> Option<Arc<Pending>> {
        self.alab_retained_ui.clone()
    }
}
#[path = "owned_local_projected_resume.rs"]
pub(crate) mod original_resume;

#[path = "owned_local_ui_frontier_passive_state.rs"]
mod passive_state;
