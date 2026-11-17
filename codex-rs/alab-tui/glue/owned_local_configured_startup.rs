// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Child of the ORIGINAL StartupDraftPump. Retain the actual configuration
//! installation and real composer/input observation; this never releases a
//! startup submission or the Source automatic-work Hold.
use super::StartupDraftPump;
use super::Tui;
use crate::app_server_session::AppServerSession;
use crate::owned_local_frontend::FrontendTenure;
use codex_alab_tui::frontend_control::FrontendFrontier;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::retained_configured::ConfigurationCommit;
use codex_app_server_client::retained_configured::ConfiguredReplay;
use codex_app_server_client::retained_configured::{self};
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Phase {
    Unstarted,
    Projecting,
    Installed,
    WaitingTail,
    Ready,
    Observing,
    Complete,
    Unknown,
}
struct Submission {
    original: bool,
    draft: crate::bottom_pane::ComposerDraftSnapshot,
}
struct Progress {
    phase: Phase,
    replay: Option<ConfiguredReplay>,
    _completion: Option<Arc<ConfigurationCommit>>,
    tail: Option<codex_app_server_protocol::RetainedConfiguredNotification>,
    // The exact original startup submission intent is DATA, not permission.
    // No NoPrimary completion method removes or dispatches it.
    _submission: Option<Submission>,
    _initial_message: Option<Option<crate::chatwidget::UserMessage>>,
    _callbacks: callback_cut::Owner,
}
pub(crate) struct Pending {
    tenure: FrontendTenure,
    handle: AppServerRequestHandle,
    native: FrontendFrontier,
    status: Arc<crate::app_server_session::alab_retained_ui::UiStatus>,
    progress: Mutex<Progress>,
}
fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "configured startup observation is unavailable",
    )
}
impl Pending {
    pub(crate) fn new(
        session: &AppServerSession,
        native: FrontendFrontier,
    ) -> io::Result<Arc<Self>> {
        let tenure = session.retained_frontend().ok_or_else(unavailable)?;
        if !native.is_for_controller(tenure.controller()) {
            return Err(unavailable());
        }
        let pending = Arc::new(Self {
            tenure: tenure.clone(),
            handle: session.request_handle(),
            native,
            status: crate::app_server_session::alab_retained_ui::UiStatus::new(),
            progress: Mutex::new(Progress {
                phase: Phase::Unstarted,
                replay: None,
                _completion: None,
                tail: None,
                _submission: None,
                _initial_message: None,
                _callbacks: callback_cut::Owner::Unprepared,
            }),
        });
        tenure.install_configured_frontier(&pending)?;
        Ok(pending)
    }
    pub(crate) fn current_on(&self, session: &AppServerSession) -> bool {
        session
            .retained_frontend()
            .is_some_and(|tenure| self.belongs_to(tenure) && tenure.is_configured_frontier(self))
    }
    pub(crate) fn belongs_to(&self, tenure: &FrontendTenure) -> bool {
        tenure.same_delivery(&self.tenure)
            && self.native.is_for_controller(tenure.controller())
            && self.native.service_data() == tenure.target.service_data()
    }
    pub(crate) fn native_frontier(&self) -> &FrontendFrontier {
        &self.native
    }
    pub(crate) fn ui_status(&self) -> Arc<crate::app_server_session::alab_retained_ui::UiStatus> {
        self.status.clone()
    }
    pub(crate) fn same_native(
        &self,
        native: &crate::app_server_session::alab_retained_ui::kind::Native,
    ) -> bool {
        match native {
            crate::app_server_session::alab_retained_ui::kind::Native::Runtime(actual) => {
                actual.is_for_controller(self.tenure.controller())
                    && actual.service_data() == self.native.service_data()
                    && actual.go_data() == self.native.go_data()
                    && actual.frontier_data() == self.native.frontier_data()
            }
            crate::app_server_session::alab_retained_ui::kind::Native::Source(_) => false,
        }
    }
    /// The actual configured renderer must apply callback DATA BEFORE its
    /// subsequent successful draw/UIACK; no Core or startup release is supplied.
    pub(crate) fn callback_receipt(
        &self,
        session: &AppServerSession,
    ) -> io::Result<Option<retained_configured::Receipt>> {
        if !self.current_on(session) {
            return Err(unavailable());
        }
        let state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state.phase == Phase::Unknown {
            return Err(unavailable());
        }
        if !matches!(state.phase, Phase::Ready | Phase::Complete) {
            return Ok(None);
        }
        Ok(Some(
            state
                .replay
                .as_ref()
                .ok_or_else(unavailable)?
                .receipt()
                .clone(),
        ))
    }
    pub(crate) fn callback_commit(
        &self,
        session: &AppServerSession,
    ) -> io::Result<Option<Arc<ConfigurationCommit>>> {
        if !self.current_on(session) {
            return Err(unavailable());
        }
        let state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state.phase != Phase::Complete {
            return Ok(None);
        }
        Ok(Some(
            state._completion.as_ref().ok_or_else(unavailable)?.clone(),
        ))
    }
    pub(crate) async fn project<T, E>(
        self: &Arc<Self>,
        original: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E>
    where
        E: From<io::Error>,
    {
        {
            let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            if state.phase != Phase::Unstarted {
                return Err(unavailable().into());
            }
            state.phase = Phase::Projecting;
        }
        let mut guard = ProjectGuard {
            pending: self.clone(),
            complete: false,
        };
        let projected =
            retained_configured::project_scope(&self.handle, self.native.frontier_data(), original)
                .await?;
        let (value, replay) = projected.into_parts();
        if !replay.is_for_client(&self.handle)
            || replay.receipt().frontier_data() != self.native.frontier_data()
        {
            // Preserve the actual non-clone completion even on a mismatch.
            self.progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .replay = Some(replay);
            return Err(unavailable().into());
        }
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        state.replay = Some(replay);
        state.phase = Phase::Installed;
        guard.complete = true;
        Ok(value)
    }
    pub(crate) fn begin_tail(
        self: &Arc<Self>,
        session: &AppServerSession,
    ) -> io::Result<TailGuard> {
        if !self.current_on(session) {
            return Err(unavailable());
        }
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state.phase != Phase::Installed {
            return Err(unavailable());
        }
        state.phase = Phase::WaitingTail;
        Ok(TailGuard {
            pending: self.clone(),
            complete: false,
        })
    }
    fn take_replay(self: &Arc<Self>, session: &AppServerSession) -> io::Result<ReplayLease> {
        if !self.current_on(session) {
            return Err(unavailable());
        }
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state.phase != Phase::Ready {
            return Err(unavailable());
        }
        let replay = state.replay.take().ok_or_else(unavailable)?;
        state.phase = Phase::Observing;
        Ok(ReplayLease {
            pending: self.clone(),
            replay: Some(replay),
            complete: false,
        })
    }
    pub(crate) fn capture_initial_message(
        &self,
    ) -> Option<crate::owned_local_entered_readers::constructor::InitialMessageData> {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            ._initial_message
            .as_ref()
            .map(crate::owned_local_entered_readers::constructor::InitialMessageData::from_actual)
    }
    fn hold_initial_message(&self, widget: &mut crate::chatwidget::ChatWidget) -> io::Result<()> {
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        hold_original_initial_message(&mut state._initial_message, widget)
    }
    fn hold_submission(
        &self,
        original: bool,
        draft: crate::bottom_pane::ComposerDraftSnapshot,
    ) -> io::Result<()> {
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state._submission.is_some() {
            return Err(unavailable());
        }
        state._submission = Some(Submission { original, draft });
        Ok(())
    }
}

/// Original synchronous transfer, shared with lower real-widget controls. This
/// is DATA/owner placement only; no Native/configured completion follows.
pub(crate) fn hold_original_initial_message(
    slot: &mut Option<Option<crate::chatwidget::UserMessage>>,
    widget: &mut crate::chatwidget::ChatWidget,
) -> io::Result<()> {
    if slot.is_some() {
        return Err(unavailable());
    }
    *slot = Some(widget.initial_user_message.take());
    Ok(())
}
pub(crate) struct TailGuard {
    pending: Arc<Pending>,
    complete: bool,
}
impl TailGuard {
    pub(crate) fn consume(
        &mut self,
        actual: &codex_app_server_protocol::RetainedConfiguredNotification,
    ) -> io::Result<bool> {
        if actual.frontier != self.pending.native.frontier_data() {
            return Ok(false);
        }
        let mut state = self
            .pending
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let replay = state.replay.as_ref().ok_or_else(unavailable)?;
        let receipt = replay.receipt();
        let matches = actual.frontier == receipt.frontier_data()
            && actual.snapshot == receipt.snapshot_data()
            && actual.runtime == receipt.runtime_data()
            && actual.version == receipt.slot_version_data();
        state.tail = Some(actual.clone()); // Own the actual consumed tail, including terminal failure.
        if state.phase != Phase::WaitingTail
            || !matches
            || !matches!(
                &actual.outcome,
                codex_app_server_protocol::RetainedFrontierOutcome::Ready {}
            )
        {
            state.phase = Phase::Unknown;
            self.pending.status.unknown();
            return Err(unavailable());
        }
        state.phase = Phase::Ready;
        self.complete = true;
        Ok(true)
    }
}
impl Drop for TailGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.pending
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .phase = Phase::Unknown;
            self.pending.status.unknown();
        }
    }
}
struct ProjectGuard {
    pending: Arc<Pending>,
    complete: bool,
}
impl Drop for ProjectGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.pending
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .phase = Phase::Unknown;
            self.pending.status.unknown();
        }
    }
}
struct ReplayLease {
    pending: Arc<Pending>,
    replay: Option<ConfiguredReplay>,
    complete: bool,
}
impl ReplayLease {
    async fn acknowledge(
        &mut self,
        session: &AppServerSession,
        input: &codex_alab_tui::upgrade_terminal::Prepared,
    ) -> io::Result<()> {
        let replay = self.replay.as_ref().ok_or_else(unavailable)?;
        if !self.pending.current_on(session)
            || !replay.is_for_client(&self.pending.handle)
            || !input.valid()
        {
            return Err(unavailable());
        }
        let ui = self.pending.native.acknowledge_ui().await?;
        if !input.valid() || !self.pending.current_on(session) {
            return Err(unavailable());
        }
        let observed = retained_configured::observe(&self.pending.handle, replay.receipt())
            .await
            .map_err(io::Error::other)?;
        if !input.valid() || !self.pending.current_on(session) {
            return Err(unavailable());
        }
        if self.pending.native.go_data().is_some() {
            let owner = self.pending.native.promote_in_app(&ui).await?;
            // Retain the exact actual App owner synchronously before observing
            // it or the final Source commit. A cancelled observer cannot lose it.
            let owner = self
                .pending
                .tenure
                .retain_app_owner(&self.pending.native, owner)?;
            owner.check_current().await?;
        }
        if !input.valid() || !self.pending.current_on(session) {
            return Err(unavailable());
        }
        let completion = retained_configured::commit(&self.pending.handle, observed.receipt())
            .await
            .map_err(io::Error::other)?;
        let valid = input.valid() && self.pending.current_on(session);
        let mut state = self
            .pending
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state._completion = Some(Arc::new(completion));
        if !valid {
            return Err(unavailable());
        }
        state.phase = Phase::Complete;
        drop(state);
        self.pending
            .tenure
            .finish_configured_frontier(&self.pending)?;
        self.pending.status.finish().map_err(io::Error::other)?;
        self.complete = true;
        Ok(())
    }
}
impl Drop for ReplayLease {
    fn drop(&mut self) {
        let mut state = self
            .pending
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.replay = self.replay.take(); // Restore SAME non-clone original completion.
        if !self.complete {
            state.phase = Phase::Unknown;
            self.pending.status.unknown();
        }
    }
}
impl StartupDraftPump {
    /// The actual SDK entry/frontier consumer calls this BEFORE polling the
    /// original preparation. A selector, label or decoded token cannot call it
    /// without already owning a genuine opaque FrontendFrontier.
    pub(crate) fn alab_accept_configured(
        &mut self,
        session: &AppServerSession,
        native: FrontendFrontier,
    ) -> io::Result<()> {
        if self.alab_configured.is_some() {
            return Err(unavailable());
        }
        self.alab_configured = Some(Pending::new(session, native)?);
        Ok(())
    }
    pub(crate) fn alab_adopt_configured_pending(
        &mut self,
        session: &AppServerSession,
        pending: Arc<Pending>,
    ) -> io::Result<()> {
        if !pending.current_on(session) {
            return Err(unavailable());
        }
        if let Some(original) = &self.alab_configured {
            if Arc::ptr_eq(original, &pending) {
                return Ok(());
            }
            return Err(unavailable());
        }
        self.alab_configured = Some(pending);
        Ok(())
    }
    pub(crate) fn alab_configured_pending(&self) -> Option<Arc<Pending>> {
        self.alab_configured.clone()
    }
    /// Called only after the WHOLE original prepare_fresh_startup_config has
    /// installed its overlay/projectless/managed defaults into actual Config.
    pub(crate) async fn alab_observe_configured(
        &mut self,
        tui: &mut Tui,
        session: &AppServerSession,
        config: &crate::legacy_core::config::Config,
    ) -> io::Result<Observation> {
        let pending = self.alab_configured.clone().ok_or_else(unavailable)?;
        // Arm BEFORE original event/input work and the real draw. A matching
        // plan can finish or a binding can be replaced during that draw.
        let image_binding = crate::retained_image_renderer::ensure_configured_binding(
            tui, config, session, &pending,
        )?;
        let image_changes = image_binding.as_ref().map(|actual| actual.changes());
        let close_changes = pending.tenure.close_changes();
        observation::check_close(&close_changes)?;
        self.flush_pending_events(tui).await?;
        self.flush_pending_paste_newline(tui).await?;
        self.apply_settings(
            &crate::local_settings::LocalSettings::for_tui(config, tui),
            config.cwd.as_path(),
        );
        let original = self.alab_constructor.clone().ok_or_else(unavailable)?;
        pending.observe_callback_cut(session, &original).await?;
        let draw = tui.alab_draw_checkpoint();
        self.redraw_if_visible(tui)?;
        // redraw_if_visible and source-exec deferred draw may return Ok without
        // drawing. Neither is a successful UI observation.
        if !tui.alab_completed_draw_after(&draw) || !self.bottom_pane.alab_draft_has_layout() {
            return Err(unavailable());
        }
        if crate::retained_image_renderer::holds_ui_ack(tui) {
            // No input capture/replay take/UI ACK occurred. Keep both actual
            // native states and wait for a real relevant change, not redraws.
            let changes = image_changes.ok_or_else(unavailable)?;
            return Ok(Observation::Held(observation::Held::new(
                changes,
                close_changes,
            )));
        }
        let editor = self.bottom_pane.alab_capture_no_primary_checkpoint(
            codex_alab_tui::upgrade_exec_custody::MAX_STATE_BYTES,
        )?;
        let input = tui.alab_prepare_upgrade_capture()?.with_editor(editor)?;
        let mut replay = pending.take_replay(session)?;
        replay.acknowledge(session, &input).await?;
        pending.acknowledge_callback_cut(session).await?;
        // No input commit_if, startup submission or Source Hold release. Drop
        // returns the SAME paused parser/terminal owner to original startup.
        Ok(Observation::Observed)
    }
    /// Original ChatWidget construction has not processed any session events.
    /// Keep the SAME CLI UserMessage out of its automatic submission path.
    pub(crate) fn alab_hold_configured_initial_message(
        &self,
        widget: &mut crate::chatwidget::ChatWidget,
    ) -> io::Result<()> {
        if let Some(pending) = &self.alab_configured {
            pending.hold_initial_message(widget)?;
        }
        Ok(())
    }
    /// Move the original confirmed intent and owner to App before this pump is
    /// consumed. NoPrimary completion NEVER returns an intent to dispatch.
    pub(crate) fn alab_hold_configured_submission(
        &mut self,
        original: bool,
    ) -> io::Result<(bool, Option<Arc<Pending>>)> {
        let Some(pending) = self.alab_configured.clone() else {
            return Ok((original, None));
        };
        pending.hold_submission(original, self.bottom_pane.composer_draft_snapshot())?;
        // Every fallible condition precedes removing the exact pump owner.
        self.alab_configured = None;
        Ok((false, Some(pending)))
    }
}

#[path = "owned_local_configured_callback_cut.rs"]
mod callback_cut;

#[path = "owned_local_configured_submission.rs"]
mod submission;

#[path = "retained_configured_image_wait.rs"]
mod observation;
pub(crate) use observation::Observation;
