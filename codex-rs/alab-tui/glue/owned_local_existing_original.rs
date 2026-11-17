// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Optional pre-send continuation over the SAME converted Start/Fork result.
//! Original Resume policy prepares its full request; this hook returns before
//! its ordinary send and Started conversion. No Core Resume or ChannelMode is
//! selected here. The caller retains its original Incoming/live-prefix owner.
use super::super::OriginalResume;
use super::*;
use crate::owned_local_frontend::ConstructionRecord;
use codex_alab_tui::frontend_control::FrontendFrontier;
use codex_app_server_client::ClientConstructionOffer;
use codex_app_server_client::ClientPresentation;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadResumeParams;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Moved original fields, never a reconstructed side policy or history view.
struct SideFields {
    boundary: std::result::Result<(), String>,
    inherited_profile: bool,
    storage: crate::app_server_session::SideChatStorage,
    legacy: bool,
}
struct OriginalParts {
    started: AppServerStartedThread,
    presentation: ClientPresentation,
    side: Option<SideFields>,
}

/// A cancellation-restoring lease used by the actual callback. The retained
/// record owns this slot; a lost observer cannot discard the original object.
struct Payload<T>(Mutex<PayloadState<T>>);
struct PayloadState<T> {
    value: Option<T>,
    quarantined: Vec<T>,
}
impl<T> Payload<T> {
    fn new(value: T) -> Self {
        Self(Mutex::new(PayloadState {
            value: Some(value),
            quarantined: Vec::new(),
        }))
    }
    fn take(&self) -> Result<PayloadLease<'_, T>> {
        let value = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .value
            .take()
            .ok_or_else(|| eyre!("original projection object already leased"))?;
        Ok(PayloadLease {
            owner: self,
            value: Some(value),
        })
    }
}
struct PayloadLease<'a, T> {
    owner: &'a Payload<T>,
    value: Option<T>,
}
impl<T> PayloadLease<'_, T> {
    fn get_mut(&mut self) -> Result<&mut T> {
        self.value
            .as_mut()
            .ok_or_else(|| eyre!("original projection object moved"))
    }
    fn into_value(mut self) -> Result<T> {
        self.value
            .take()
            .ok_or_else(|| eyre!("original projection object moved"))
    }
}
impl<T> Drop for PayloadLease<'_, T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            let mut slot = self.owner.0.lock().unwrap_or_else(PoisonError::into_inner);
            // The single real callback never submits another lease. If this
            // invariant fails, retain BOTH original values rather than drop one.
            if slot.value.is_none() {
                slot.value = Some(value);
            } else {
                slot.quarantined.push(value);
            }
        }
    }
}
/// Only the real pre-send callback consumes this actual project guard. A
/// changed/repeated invocation cannot arm or submit another operation.
struct CallbackOnce<T> {
    guard: Option<T>,
    called: bool,
}
impl<T> CallbackOnce<T> {
    fn empty() -> Self {
        Self {
            guard: None,
            called: false,
        }
    }
    fn arm(&mut self, guard: T) -> Result<()> {
        if self.called || self.guard.is_some() {
            bail!("original callback is already armed/consumed");
        }
        self.guard = Some(guard);
        Ok(())
    }
    fn take(&mut self) -> Result<T> {
        if self.called {
            bail!("original pre-send callback already used");
        }
        self.called = true;
        self.guard
            .take()
            .ok_or_else(|| eyre!("original project guard lost"))
    }
    fn consumed(&self) -> bool {
        self.called && self.guard.is_none()
    }
}
/// Returned native resources are retained even after observer uncertainty.
/// The bool below only classifies local observation; it grants no admission.
struct LateValue<T> {
    value: Option<T>,
    quarantined: Vec<T>,
}
impl<T> LateValue<T> {
    fn empty() -> Self {
        Self {
            value: None,
            quarantined: Vec::new(),
        }
    }
    fn capture(&mut self, value: T, observer_unknown: bool) -> bool {
        if self.value.is_some() {
            self.quarantined.push(value);
            return false;
        }
        self.value = Some(value);
        !observer_unknown
    }
}
struct NativeProgress {
    started: bool,
    worker: Option<JoinHandle<()>>,
    native: LateValue<FrontendSelectionFrontier>,
    joined: bool,
    quarantined: Vec<JoinHandle<()>>,
    unknown: bool,
}
struct CallbackProgress {
    pending: Weak<Pending>,
    once: CallbackOnce<super::super::super::ProjectGuard>,
    captured: Option<CapturedProjection>,
    side: Option<SideFields>,
}
/// ConstructionRecord strongly owns Work. The reverse reference and project
/// guard are weak; the stored original has its Record back-link temporarily
/// removed, so completed/Unknown retention creates no strong cycle.
pub(crate) struct Work {
    record: Weak<ConstructionRecord>,
    original: Arc<Payload<OriginalParts>>,
    base: FrontendFrontier,
    prepared: ClientConstructionOffer,
    thread: codex_protocol::ThreadId,
    view: uuid::Uuid,
    presentation: ClientPresentation,
    native: Mutex<NativeProgress>,
    callback: Mutex<CallbackProgress>,
    changed: watch::Sender<Option<std::result::Result<(), std::io::ErrorKind>>>,
}
tokio::task_local! { static EXISTING: Arc<Work>; }

/// Own immediately after original conversion and BEFORE its first await. The
/// actual ConstructionRecord retains the same object/worker independently.
pub(crate) struct ExistingPrimary {
    work: Arc<Work>,
    complete: bool,
}
pub(crate) struct ExistingSide {
    work: Arc<Work>,
    complete: bool,
}
/// Original Started side attachment; no SidebarFork outer fields exist.
pub(crate) struct ExistingSideAttachment {
    work: Arc<Work>,
    complete: bool,
}

struct NativeGuard {
    work: Arc<Work>,
    complete: bool,
}
impl Drop for NativeGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.work.unknown();
        }
    }
}
/// A failed synchronous preparation RETURNS the entire original owner. There
/// is deliberately no Into<Report>: callers must retain/handle the object,
/// including side storage/boundary, before reporting the original failure.
pub(crate) struct ExistingPreparationFailure<T> {
    pub(crate) original: T,
    pub(crate) error: color_eyre::eyre::Report,
}
#[path = "owned_local_preparation_keeper.rs"]
pub(crate) mod preparation;
use preparation::CapturedOriginal;

impl Work {
    fn from_capture(
        session: &AppServerSession,
        record: &Arc<ConstructionRecord>,
        captured: &Arc<CapturedOriginal>,
    ) -> Result<Arc<Self>> {
        if !captured.belongs_to(record) {
            bail!("preparation changed its actual original owner");
        }
        let mut lease = captured.original.take()?;
        let original = lease.get_mut()?;
        if original.started.alab_construction.is_some() {
            bail!("stored original retained a strong Record backlink");
        }
        if original.presentation != record.presentation_data()
            || (original.side.is_some()
                && original.presentation != ClientPresentation::SideConversation)
        {
            bail!("stored original changed its pre-S1 presentation declaration");
        }

        if original
            .side
            .as_ref()
            .is_some_and(|side| side.boundary.is_err())
            || (original.side.is_some() && original.started.session.daybreak_enabled)
        {
            bail!("side reconciliation lost its original successful boundary/presentation");
        }
        if codex_alab_tui::upgrade_state::pending().is_some()
            || session.attachment_subscribed != Some(false)
        {
            bail!("original reconciliation requires the unchanged unused retained attachment view");
        }
        let tenure = session
            .retained_frontend()
            .ok_or_else(|| eyre!("original reconciliation requires its actual final delivery"))?;
        if !record.belongs_to(tenure) {
            bail!("original construction belongs to another physical tenure");
        }
        let prepared = record
            .prepared_offer()
            .ok_or_else(|| eyre!("original construction has no observed prepared offer"))?;
        let base = tenure.runtime_frontier()?;
        if !base.is_for_controller(tenure.controller())
            || base.service_data() != tenure.target.service_data()
        {
            bail!("original constructor context changed actual controller/service");
        }
        let thread = original.started.session.thread_id;
        let view = session
            .request_id_namespace
            .ok_or_else(|| eyre!("original reconciliation lost its exact attachment view"))?;
        let presentation = original.presentation;
        let (changed, _) = watch::channel(None);
        let work = Arc::new(Self {
            record: Arc::downgrade(record),
            original: captured.original.clone(),
            base,
            prepared,
            thread,
            view,
            presentation,
            native: Mutex::new(NativeProgress {
                started: false,
                worker: None,
                native: LateValue::empty(),
                joined: false,
                quarantined: Vec::new(),
                unknown: false,
            }),
            callback: Mutex::new(CallbackProgress {
                pending: Weak::new(),
                once: CallbackOnce::empty(),
                captured: None,
                side: None,
            }),
            changed,
        });
        drop(lease); // Restore same full object before Work publication/first await.
        record.start_existing_capture(captured, work.clone())?;
        Ok(work)
    }
    fn unknown(&self) {
        self.native
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .unknown = true;
        if let Some(record) = self.record.upgrade() {
            record.original_projection_unknown();
        }
        if let Some(pending) = self
            .callback
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .upgrade()
        {
            pending.unknown();
        }
        self.changed.send_if_modified(|result| {
            if result.is_some() {
                false
            } else {
                *result = Some(Err(std::io::ErrorKind::Other));
                true
            }
        });
    }
    async fn observe_native(self: &Arc<Self>) -> Result<FrontendSelectionFrontier> {
        let mut changed = self.changed.subscribe();
        let start = {
            let mut state = self.native.lock().unwrap_or_else(PoisonError::into_inner);
            if state.unknown {
                bail!("original Source begin is uncertain");
            }
            if state.started {
                false
            } else {
                state.started = true;
                true
            }
        };
        if start {
            let work = self.clone();
            let guard = NativeGuard {
                work: work.clone(),
                complete: false,
            };
            // The same Native operation/immutable offer belongs to this stored
            // task, not the lifetime of an observing frontend future.
            let worker = tokio::spawn(async move {
                let mut guard = guard;
                let result = work
                    .base
                    .begin_source_selection(
                        &work.prepared.source_operation,
                        &work.prepared.selection,
                    )
                    .await;
                match result {
                    Ok(native) => {
                        let mut state = work.native.lock().unwrap_or_else(PoisonError::into_inner);
                        let observer_unknown = state.unknown;
                        // Store the actual opaque success BEFORE classifying a
                        // canceled observer. Unknown never discards/resubmits it.
                        let observed = state.native.capture(native, observer_unknown);
                        if !observed {
                            state.unknown = true;
                        }
                        drop(state);
                        if observed {
                            work.changed.send_replace(Some(Ok(())));
                        } else {
                            work.unknown();
                        }
                    }
                    Err(_) => work.unknown(),
                }
                guard.complete = true;
            });
            self.native
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .worker = Some(worker);
        }
        loop {
            let result = *changed.borrow_and_update();
            if let Some(result) = result {
                result.map_err(std::io::Error::from)?;
                let native = {
                    let state = self.native.lock().unwrap_or_else(PoisonError::into_inner);
                    if state.unknown {
                        bail!("original Source begin became uncertain");
                    }
                    state
                        .native
                        .value
                        .clone()
                        .ok_or_else(|| eyre!("original Source begin lost actual result"))?
                };
                NativeJoin::take(self.clone())?.observe().await?;
                return Ok(native);
            }
            changed.changed().await?;
        }
    }
    async fn run(
        self: &Arc<Self>,
        session: &mut AppServerSession,
        original: OriginalResume<'_>,
    ) -> Result<(
        AppServerStartedThread,
        CapturedProjection,
        ProjectionReplay,
        Option<SideFields>,
    )> {
        if original.thread_id != self.thread
            || session.request_id_namespace != Some(self.view)
            || codex_alab_tui::upgrade_state::pending().is_some()
        {
            bail!("original projection changed target or source-exec mode");
        }
        let native = self.observe_native().await?;
        let record = self
            .record
            .upgrade()
            .ok_or_else(|| eyre!("original construction owner retired"))?;
        let tenure = session
            .retained_frontend()
            .ok_or_else(|| eyre!("original delivery retired"))?;
        if !record.belongs_to(tenure) {
            bail!("original construction changed physical delivery");
        }
        let pending = Pending::new_source(session, native.clone(), record.owner_for_projection())?;
        session.install_pending(pending.clone())?;
        let mut outcome = ReconciliationGuard {
            pending: pending.clone(),
            complete: false,
        };
        let project = pending.begin_original_project()?;
        let replay = ProjectionReplay(pending.clone()); // Sole nonclone replay BEFORE original awaits.
        {
            let mut state = self.callback.lock().unwrap_or_else(PoisonError::into_inner);
            if state.pending.upgrade().is_some() {
                bail!("original policy projection already attempted");
            }
            state.once.arm(project)?;
            state.pending = Arc::downgrade(&pending);
        }
        let handle = session.request_handle();
        let target = self.thread.to_string();
        // This real shared SDK scope suppresses a second construction wrapper
        // and excludes configured/R/other construction owners. The pre-send
        // hook itself calls project_existing's LOW-LEVEL remote send, so it
        // never recursively enters the typed request dispatcher.
        let (started, dispatched) = source_selection::scope_original_resume(
            &handle,
            &native,
            &self.prepared,
            &target,
            EXISTING.scope(
                self.clone(),
                session.resume_thread_with_permission_overrides(
                    original.local_settings,
                    original.config,
                    original.thread_id,
                    original.model_settings,
                    original.permission_overrides,
                ),
            ),
        )
        .await?;
        if dispatched.is_some() {
            bail!("original S2 callback was bypassed by a typed Resume send");
        }
        let mut started = started?;
        if started.session.thread_id != self.thread || !pending.current_on(session) {
            bail!("original reconciliation changed selected object/delivery");
        }
        started.alab_construction = Some(record);
        let (captured, side) = {
            let mut state = self.callback.lock().unwrap_or_else(PoisonError::into_inner);
            if !state.once.consumed() {
                bail!("original pre-send callback did not complete");
            }
            (
                state
                    .captured
                    .take()
                    .ok_or_else(|| eyre!("original snapshot lost"))?,
                state.side.take(),
            )
        };
        outcome.complete = true;
        Ok((started, captured, replay, side))
    }
}
/// Borrow-observe the SAME Native worker handle. Cancelled observations put
/// the unfinished handle back; completed handles are consumed immediately.
struct NativeJoin {
    work: Arc<Work>,
    worker: Option<JoinHandle<()>>,
}
impl NativeJoin {
    fn take(work: Arc<Work>) -> Result<Self> {
        let worker = {
            let mut state = work.native.lock().unwrap_or_else(PoisonError::into_inner);
            if state.joined {
                bail!("original Native begin was already joined");
            }
            state
                .worker
                .take()
                .ok_or_else(|| eyre!("original Native worker is already observed"))?
        };
        Ok(Self {
            work,
            worker: Some(worker),
        })
    }
    async fn observe(mut self) -> Result<()> {
        let result = self
            .worker
            .as_mut()
            .ok_or_else(|| eyre!("original Native worker lost"))?
            .await;
        self.worker.take();
        self.work
            .native
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .joined = true;
        if let Err(error) = result {
            self.work.unknown();
            return Err(error.into());
        }
        Ok(())
    }
}
impl Drop for NativeJoin {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let mut state = self
                .work
                .native
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if state.worker.is_none() {
                state.worker = Some(worker);
            } else {
                state.quarantined.push(worker);
                state.unknown = true;
            }
        }
    }
}

impl ExistingPrimary {
    pub(crate) async fn reconcile(
        mut self,
        session: &mut AppServerSession,
        original: OriginalResume<'_>,
    ) -> Result<ReconciledPrimary> {
        if self.work.presentation != ClientPresentation::Primary {
            bail!("primary route changed original presentation");
        }
        let (mut started, captured, replay, side) = self.work.run(session, original).await?;
        if side.is_some() {
            bail!("primary reconciliation changed original presentation");
        }
        started.turns = captured.current.thread.turns;
        self.complete = true;
        Ok(ReconciledPrimary {
            started,
            replay,
            attachment: captured.attachment,
        })
    }
}
impl ExistingSide {
    pub(crate) async fn reconcile(
        mut self,
        session: &mut AppServerSession,
        original: OriginalResume<'_>,
    ) -> Result<ReconciledSide> {
        if self.work.presentation != ClientPresentation::SideConversation {
            bail!("side creation route changed original presentation");
        }
        let (thread, captured, replay, side) = self.work.run(session, original).await?;
        let side = side.ok_or_else(|| eyre!("side reconciliation lost original fields"))?;
        let original = SidebarFork {
            thread,
            boundary: side.boundary,
            inherited_profile: side.inherited_profile,
            storage: side.storage,
            legacy: side.legacy,
            alab_retained_replay: None,
        };
        self.complete = true;
        Ok(ReconciledSide {
            original,
            current: captured.current,
            attachment: captured.attachment,
            replay,
        })
    }
}
impl ExistingSideAttachment {
    pub(crate) async fn reconcile(
        mut self,
        session: &mut AppServerSession,
        original: OriginalResume<'_>,
    ) -> Result<ReconciledSideAttachment> {
        if self.work.presentation != ClientPresentation::SideConversation {
            bail!("side attachment route changed original presentation");
        }
        let (started, captured, replay, side) = self.work.run(session, original).await?;
        if side.is_some() {
            bail!("side attachment unexpectedly acquired fork-creation outer fields");
        }
        self.complete = true;
        Ok(ReconciledSideAttachment {
            original: started,
            current: captured.current,
            attachment: captured.attachment,
            replay,
        })
    }
}
impl Drop for ExistingSideAttachment {
    fn drop(&mut self) {
        if !self.complete {
            self.work.unknown();
        }
    }
}
impl Drop for ExistingPrimary {
    fn drop(&mut self) {
        if !self.complete {
            self.work.unknown();
        }
    }
}
impl Drop for ExistingSide {
    fn drop(&mut self) {
        if !self.complete {
            self.work.unknown();
        }
    }
}

/// One tiny original helper hook. None is ordinary behavior. Some consumes the
/// exact already-prepared original request once while the ORIGINAL maintenance
/// guard remains on its stack through this await.
pub(crate) async fn before_original_send(
    session: &mut AppServerSession,
    request_id: RequestId,
    params: ThreadResumeParams,
    config: &Config,
    local_settings: &crate::local_settings::LocalSettings,
) -> Result<Option<AppServerStartedThread>> {
    let Some(work) = EXISTING.try_with(Arc::clone).ok() else {
        return Ok(None);
    };
    if params.thread_id != work.thread.to_string()
        || session.request_id_namespace != Some(work.view)
        || codex_alab_tui::upgrade_state::pending().is_some()
    {
        bail!("original callback changed actual target or source mode");
    }
    let (pending, project) = {
        let mut state = work.callback.lock().unwrap_or_else(PoisonError::into_inner);
        let project = state.once.take()?;
        (
            state
                .pending
                .upgrade()
                .ok_or_else(|| eyre!("original Pending retired"))?,
            project,
        )
    };
    let mut lease = work.original.take()?;
    let parts = lease.get_mut()?;
    let captured = session
        .alab_project_existing_in_pending(
            pending,
            project,
            &mut parts.started,
            ExistingProjectionInputs {
                request: ClientRequest::ThreadResume { request_id, params },
                local_settings,
                config,
            },
            work.presentation == ClientPresentation::Primary,
            &work.prepared,
        )
        .await?;
    let OriginalParts {
        started,
        presentation: _,
        side,
    } = lease.into_value()?;
    {
        let mut state = work.callback.lock().unwrap_or_else(PoisonError::into_inner);
        state.captured = Some(captured);
        state.side = side;
    }
    Ok(Some(started))
}

#[cfg(test)]
#[path = "owned_local_existing_original_tests.rs"]
mod tests;
