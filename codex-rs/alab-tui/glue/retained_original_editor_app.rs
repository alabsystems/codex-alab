// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual original App editor job, with the old inline-editor event policy.
//! The original outer frame lends a real update-observer future while retaining
//! ALL App/server/Tui queues. No Codec value creates a Child or Native authority.
use super::*;
use crate::copy_input_guard::alab_retained_graph::Census;
use codex_alab_tui::frontend_control::retained_service::original_editor::CancelledBeforeStart;
use codex_alab_tui::frontend_control::retained_service::original_editor::ExecFence;
use codex_alab_tui::frontend_control::retained_service::original_editor::Finished;
use codex_alab_tui::frontend_control::retained_service::original_editor::Operation;
use std::future::Future;
use std::io;

#[derive(Default)]
pub(crate) struct Book {
    driver: Option<Census>,
    active: Option<Arc<Job>>,
}
pub(crate) struct Job {
    registration: u64,
    cut: std::sync::Mutex<Option<Arc<ExecFence>>>,
    cut_gate: tokio::sync::Semaphore,
    c0: std::sync::Mutex<Option<Arc<c0::Transfer>>>,
    // Set before the actual Native offer is started, never from response loss.
    native_started: std::sync::atomic::AtomicBool,
    operation: Arc<Operation>,
    handoff: std::sync::Mutex<crate::tui::ExternalHandoff>,
    // Transport/task/handback uncertainty is not editor completion or an App
    // exit. Keep the exact error and actual job while the real outer observer
    // can continue; never poll an immediate error in a spin.
    observation_error: std::sync::Mutex<Option<Arc<io::Error>>>,
    prechild: std::sync::Mutex<Option<codex_alab_tui::input::OriginalEditorResources>>,
    cancelled: std::sync::Mutex<Option<Arc<CancelledBeforeStart>>>,
    late_errors: std::sync::Mutex<Vec<Arc<io::Error>>>,
    stdio: std::sync::Mutex<Option<Arc<crate::tui::OriginalEditorStdio>>>,
    // SAME genuine pre-child mode source; refusal keeps this actual Job and
    // cancels before Child start, then performs only exact local handback.
    mode_source: std::result::Result<
        Arc<std::sync::Mutex<codex_alab_tui::terminal_modes::Prepared>>,
        Arc<io::Error>,
    >,
}
#[must_use = "the original outer frame must keep Held editor/TTY resources and queues owned"]
pub(crate) enum Pump<U> {
    Finished,
    Update(U),
    Held(Arc<io::Error>),
}
impl Job {
    pub(crate) fn operation(&self) -> &Arc<Operation> {
        &self.operation
    }
    fn retain_observation_error(&self, error: io::Error) -> Arc<io::Error> {
        let mut retained = self
            .observation_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let error = Arc::new(error);
        if let Some(first) = retained.as_ref() {
            self.late_errors
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(error);
            first.clone()
        } else {
            *retained = Some(error.clone());
            error
        }
    }
    pub(crate) async fn freeze_before_h1(&self) -> io::Result<Arc<ExecFence>> {
        let _serial = self.cut_gate.acquire().await.map_err(io::Error::other)?;
        self.require_local_c0_released()?;
        if self
            .observation_error
            .lock()
            .map_err(|_| io::Error::other("original editor observation poisoned"))?
            .is_some()
        {
            return Err(io::Error::other(
                "original editor pre-child resources remain held",
            ));
        }
        let retained = {
            self.cut
                .lock()
                .map_err(|_| io::Error::other("original editor cut poisoned"))?
                .clone()
        };
        if let Some(actual) = retained {
            return Ok(actual);
        }
        let actual = Arc::new(self.operation.freeze_before_h1().await?);
        // Publication has no await gap. The actual registry-owned Job retains
        // this SAME root/read exclusion before ANY Native custody/H1 await.
        *self
            .cut
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(actual.clone());
        Ok(actual)
    }
    pub(crate) fn finished_before_native(
        &self,
        actual: &Arc<ExecFence>,
    ) -> io::Result<Option<Arc<Finished>>> {
        let cut = self
            .cut
            .lock()
            .map_err(|_| io::Error::other("original editor cut poisoned"))?;
        if self
            .native_started
            .load(std::sync::atomic::Ordering::Acquire)
            || !cut
                .as_ref()
                .is_some_and(|stored| Arc::ptr_eq(stored, actual))
        {
            return Err(io::Error::other(
                "original finished-pending query changed local pre-Native cut",
            ));
        }
        actual.finished_pending()
    }
    pub(crate) fn retain_native_start(&self, actual: &Arc<ExecFence>) -> io::Result<()> {
        let cut = self
            .cut
            .lock()
            .map_err(|_| io::Error::other("original editor cut poisoned"))?;
        if !cut
            .as_ref()
            .is_some_and(|stored| Arc::ptr_eq(stored, actual))
        {
            return Err(io::Error::other(
                "Native offer changed exact original editor cut",
            ));
        }
        self.native_started
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    /// Definite LOCAL pre-Native rollback only. Once the actual offer starts,
    /// only a real Native handback/retirement companion may release this cut.
    pub(crate) async fn release_local_cut_before_native(
        &self,
        actual: &Arc<ExecFence>,
    ) -> io::Result<()> {
        let _serial = self.cut_gate.acquire().await.map_err(io::Error::other)?;
        let retired = {
            let mut cut = self
                .cut
                .lock()
                .map_err(|_| io::Error::other("original editor cut poisoned"))?;
            if self
                .native_started
                .load(std::sync::atomic::Ordering::Acquire)
                || !cut
                    .as_ref()
                    .is_some_and(|stored| Arc::ptr_eq(stored, actual))
            {
                return Err(io::Error::other(
                    "original editor Native custody already started or changed",
                ));
            }
            cut.take()
        };
        drop(retired);
        Ok(())
    }
    /// Actual C1 cut, after the original Tui handoff and SAME root/read fence,
    /// before any Native escrow/H1 await. Every accepted restriction is stored
    /// immediately in this actual Job; a later mode/metadata refusal drops none.
    pub(crate) fn prepare_original_c1_resources(
        &self,
        tui: &crate::tui::Tui,
        fence: &ExecFence,
    ) -> io::Result<()> {
        self.handoff
            .lock()
            .map_err(|_| io::Error::other("original editor handoff poisoned"))?
            .is_for(tui)?;
        if !self.operation.is_for_fence(fence) {
            return Err(io::Error::other(
                "original editor capture changed actual root fence",
            ));
        }
        // The complete bundle was already retained before Child start. H1
        // may correlate it but never creates late parser/mode restrictions.
        let resources = self
            .prechild
            .lock()
            .map_err(|_| io::Error::other("original editor pre-child owner poisoned"))?;
        let resources = resources
            .as_ref()
            .ok_or_else(|| io::Error::other("pre-child resources absent"))?;
        let modes = resources
            .modes()
            .ok_or_else(|| io::Error::other("pre-child modes absent"))?;
        let input = resources
            .input()
            .ok_or_else(|| io::Error::other("pre-child parser absent"))?;
        modes.snapshot_for_exec(fence, input.fence())?;
        let mut stdio = self
            .stdio
            .lock()
            .map_err(|_| io::Error::other("original editor stdio owner poisoned"))?;
        if stdio.is_none() {
            *stdio = Some(tui.alab_capture_original_editor_stdio(
                &self.operation,
                modes,
                fence,
                input,
            )?);
        }
        Ok(())
    }
    /// This is the observed pre-child source only. Generic Prepared.valid()
    /// must NOT compare the live editor's changed running modes to old raw TUI.
    pub(crate) fn original_mode_source(
        &self,
    ) -> std::result::Result<
        &Arc<std::sync::Mutex<codex_alab_tui::terminal_modes::Prepared>>,
        &io::Error,
    > {
        self.mode_source.as_ref().map_err(Arc::as_ref)
    }
}
impl App {
    /// Call at the real original outer-loop boundary, with its SAME receiver
    /// census. No flag decoded from E1 may install an outer driver.
    pub(crate) fn alab_bind_original_editor_driver(&mut self, actual: &Census) -> io::Result<()> {
        actual.validate_sender(&self.app_event_tx)?;
        if let Some(previous) = &self.alab_external_editor.driver {
            previous.validate_sender(&self.app_event_tx)?;
            return Ok(());
        }
        self.alab_external_editor.driver = Some(actual.clone());
        Ok(())
    }
    pub(crate) fn alab_pending_original_editor(&self) -> Option<Arc<Job>> {
        self.alab_external_editor.active.clone()
    }
    /// The ordinary local caller has no Native update observer installed yet.
    /// It preserves the old inline wait policy. The actual retained outer
    /// caller lends its real observer through alab_original_editor_or_update;
    /// no fake Candidate/Ready/Native result is manufactured here.
    pub(crate) async fn alab_wait_local_original_editor(&mut self, tui: &mut crate::tui::Tui) {
        while let Some(job) = self.alab_pending_original_editor() {
            match self
                .alab_original_editor_or_update(
                    tui,
                    &job,
                    std::future::pending::<std::convert::Infallible>(),
                )
                .await
            {
                Ok(Pump::Finished) => {}
                Ok(Pump::Update(never)) => match never {},
                Ok(Pump::Held(error)) => {
                    tracing::warn!(%error, "original editor observation remains held");
                    std::future::pending::<()>().await;
                }
                Err(error) => {
                    job.retain_observation_error(error);
                    std::future::pending::<()>().await;
                }
            }
        }
    }
    /// Called instead of old launch_external_editor ONLY when the actual outer
    /// driver was installed. False means the unchanged ordinary inline route.
    pub(crate) fn alab_begin_original_editor(
        &mut self,
        tui: &mut crate::tui::Tui,
    ) -> io::Result<bool> {
        let Some(driver) = &self.alab_external_editor.driver else {
            return Ok(false);
        };
        driver.validate_sender(&self.app_event_tx)?;
        if self.alab_external_editor.active.is_some() {
            return Err(io::Error::other("original editor/handoff already retained"));
        }
        self.chat_widget
            .empty_state_animation
            .borrow_mut()
            .pause_clock();
        let command = match external_editor::resolve_editor_command() {
            Ok(command) => command,
            Err(external_editor::EditorError::MissingEditor) => {
                self.chat_widget
                    .add_to_history(history_cell::new_error_event(
                    "Cannot open external editor: set $VISUAL or $EDITOR before starting Codex."
                        .to_string(),
                ));
                self.reset_external_editor_state(tui);
                return Ok(true);
            }
            Err(error) => {
                self.alab_original_editor_error(tui, error);
                return Ok(true);
            }
        };
        let seed = self.chat_widget.composer_text_with_pending();
        let config = self.chat_widget.config_ref();
        let policy = config.permissions.file_system_sandbox_policy();
        let prepared = match external_editor::alab_prepared_editor::prepare(
            &seed,
            &command,
            config.codex_home.as_path(),
            &policy,
            config.cwd.as_path(),
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.alab_original_editor_error(tui, error);
                return Ok(true);
            }
        };
        let (command, document) = prepared.into_original_parts();
        let registration = match jobs::Reservation::reserve() {
            Ok(registration) => registration,
            Err(error) => {
                self.alab_original_editor_error(tui, error);
                return Ok(true);
            }
        };
        // All fallible admission precedes the original Tty handoff. This stores
        // actual Command/TempPath but starts NO task, child or Native reservation.
        let operation = match Operation::prepare(command, document) {
            Ok(operation) => operation,
            Err(error) => {
                self.alab_original_editor_error(tui, error);
                return Ok(true);
            }
        };
        let handoff = match tui.alab_begin_external_handoff(crate::tui::TerminalHandoff::KeepScreen)
        {
            Ok(handoff) => handoff,
            Err(error) => {
                // Same lifecycle gate positively prevents any later start.
                operation.cancel_before_start()?;
                self.alab_original_editor_error(tui, error);
                return Ok(true);
            }
        };
        let mode_source = codex_alab_tui::terminal_modes::prepare_upgrade()
            .map(|source| Arc::new(std::sync::Mutex::new(source)))
            .map_err(Arc::new);
        let job = Arc::new(Job {
            registration: registration.key(),
            cut: std::sync::Mutex::new(None),
            cut_gate: tokio::sync::Semaphore::new(1),
            c0: std::sync::Mutex::new(None),
            native_started: std::sync::atomic::AtomicBool::new(false),
            operation,
            handoff: std::sync::Mutex::new(handoff),
            mode_source,
            observation_error: std::sync::Mutex::new(None),
            prechild: std::sync::Mutex::new(None),
            cancelled: std::sync::Mutex::new(None),
            late_errors: std::sync::Mutex::new(Vec::new()),
            stdio: std::sync::Mutex::new(None),
        });
        // Acquire before publishing this Job to any other real observer. Even
        // a failed gate acquisition publishes the actual handoff/Job unchanged.
        let start = job.operation.before_start();
        registration.install(job.clone());
        self.alab_external_editor.active = Some(job.clone());
        // ONE actual lifecycle guard excludes every wait/start observer through
        // BOTH factories and publication of every accepted partial/full wrapper.
        let before = match start {
            Ok(before) => before,
            Err(error) => {
                job.retain_observation_error(error);
                return Ok(true);
            }
        };
        let preparation_error = match job.original_mode_source() {
            Ok(source) => match codex_alab_tui::input::OriginalEditorResources::prepare(
                &job.operation,
                &before,
                source,
            ) {
                Ok(actual) => {
                    *job.prechild
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(actual);
                    None
                }
                Err(refused) => {
                    let codex_alab_tui::input::OriginalEditorResourcesRefused { actual, error } =
                        *refused;
                    *job.prechild
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(actual);
                    Some(job.retain_observation_error(error))
                }
            },
            Err(_) => {
                let error = job.mode_source.as_ref().err().cloned();
                if let Some(error) = &error {
                    *job.observation_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.clone());
                }
                error
            }
        };
        if preparation_error.is_some() {
            // No fabricated Finished and no child started just to roll back.
            // Failed cancellation keeps all wrappers globally retained; the
            // lifecycle itself is sticky cancelled before any registry error.
            match before.cancel() {
                Ok(cancelled) => {
                    *job.cancelled
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(Arc::new(cancelled));
                    if let Err(error) = self.alab_cancel_original_editor(tui, &job) {
                        job.retain_observation_error(error);
                    }
                }
                Err(error) => {
                    job.retain_observation_error(error);
                }
            }
        } else {
            drop(before); // BOTH actual owners are already globally published.
            // This is the original processed Launch event, before control is
            // returned to an update select, not an escrow-driven child birth.
            if let Err(error) = job.operation.start_original() {
                job.retain_observation_error(error);
            }
        }
        Ok(true)
    }
    fn alab_cancel_original_editor(
        &mut self,
        tui: &mut crate::tui::Tui,
        job: &Arc<Job>,
    ) -> io::Result<()> {
        self.alab_require_original_editor(job)?;
        let cancelled = job
            .cancelled
            .lock()
            .map_err(|_| io::Error::other("original cancellation poisoned"))?
            .as_ref()
            .cloned()
            .ok_or_else(|| io::Error::other("actual no-child cancellation absent"))?;
        if !cancelled.is_for(&job.operation) {
            return Err(io::Error::other("cancelled editor operation changed"));
        }
        let mut handoff = job
            .handoff
            .lock()
            .map_err(|_| io::Error::other("original editor handoff poisoned"))?;
        handoff.is_for(tui)?;
        let mut resources = job
            .prechild
            .lock()
            .map_err(|_| io::Error::other("original pre-child bundle poisoned"))?;
        if let Some(modes) = resources.as_ref().and_then(|actual| actual.modes()) {
            modes.release_for_cancelled_handback(&cancelled)?;
        } else if let Some(input) = resources.as_ref().and_then(|actual| actual.input()) {
            let source = job
                .original_mode_source()
                .map_err(|error| io::Error::other(error.to_string()))?;
            codex_alab_tui::terminal_modes::restore_cancelled_original_editor(
                &job.operation,
                &cancelled,
                input,
                source,
            )?;
        }
        handoff.finish_with(tui, || {
            if let Some(input) = resources.as_mut().and_then(|actual| actual.input_mut()) {
                input.finish_after_cancelled_handoff(&cancelled)?;
            }
            Ok(())
        })?;
        let original_error = job
            .observation_error
            .lock()
            .map_err(|_| io::Error::other("original editor observation poisoned"))?
            .as_ref()
            .cloned()
            .ok_or_else(|| io::Error::other("original preparation refusal absent"))?;
        self.alab_original_editor_error(tui, original_error);
        jobs::retire_after_cancelled_handback(job, &cancelled)?;
        self.alab_external_editor.active = None;
        Ok(())
    }
    fn alab_original_editor_error(
        &mut self,
        tui: &mut crate::tui::Tui,
        error: impl std::fmt::Display,
    ) {
        self.reset_external_editor_state(tui);
        self.chat_widget
            .add_to_history(history_cell::new_error_event(format!(
                "Failed to open editor: {error}"
            )));
        tui.frame_requester().schedule_frame();
    }
    /// Call BEFORE any ordinary outer-loop replay/reconnect/mouse/timer work.
    /// A real update observation returns the SAME live App/Tui to that original
    /// frame. App/server/keyboard/Draw queues remain completely unpolled here,
    /// preserving original inline-editor ordering while update work can proceed.
    pub(crate) async fn alab_original_editor_or_update<U>(
        &mut self,
        tui: &mut crate::tui::Tui,
        job: &Arc<Job>,
        update: impl Future<Output = U>,
    ) -> io::Result<Pump<U>> {
        self.alab_require_original_editor(job)?;
        let held = {
            job.observation_error
                .lock()
                .map_err(|_| io::Error::other("original editor observation poisoned"))?
                .is_some()
        };
        if held || job.c0_resources_held()? {
            // SAME real update observer only. Original App/server/input/Draw
            // queues stay parked, and an immediate cached Err cannot spin.
            return Ok(Pump::Update(update.await));
        }
        let observed = tokio::select! {
            finished = job.operation.wait() => finished,
            update = update => return Ok(Pump::Update(update)),
        };
        let actual = match observed {
            Ok(actual) => actual,
            Err(error) => return Ok(Pump::Held(job.retain_observation_error(error))),
        };
        match self.alab_apply_original_editor(tui, job, &actual) {
            Ok(()) => Ok(Pump::Finished),
            Err(error) => Ok(Pump::Held(job.retain_observation_error(error))),
        }
    }
    fn alab_require_original_editor(&self, actual: &Arc<Job>) -> io::Result<()> {
        if !self
            .alab_external_editor
            .active
            .as_ref()
            .is_some_and(|stored| Arc::ptr_eq(stored, actual))
        {
            return Err(io::Error::other(
                "original editor belongs to another actual App job",
            ));
        }
        self.alab_external_editor
            .driver
            .as_ref()
            .ok_or_else(|| io::Error::other("original editor outer driver absent"))?
            .validate_sender(&self.app_event_tx)
    }
    fn alab_apply_original_editor(
        &mut self,
        tui: &mut crate::tui::Tui,
        job: &Arc<Job>,
        actual: &Finished,
    ) -> io::Result<()> {
        self.alab_require_original_editor(job)?;
        job.require_local_c0_released()?;
        if !actual.is_for(&job.operation) {
            return Err(io::Error::other(
                "original editor result changed actual job",
            ));
        }
        let mut handoff = job
            .handoff
            .lock()
            .map_err(|_| io::Error::other("original editor handoff poisoned"))?;
        handoff.is_for(tui)?; // Preflight before any callback or actual handback.
        actual.apply_and_retire(|| {
            // An accepted Native adoption must first supply its real original
            // parent stdio/handback result here (that companion is not wired).
            // These are SAME local owners; no generic raw-mode reset can free
            // a parser or an editor-owned mode restriction.
            let stdio = {
                job.stdio
                    .lock()
                    .map_err(|_| io::Error::other("original editor stdio owner poisoned"))?
                    .clone()
            };
            if let Some(stdio) = stdio {
                stdio.restore_original_parent(actual)?;
            }
            let mut resources = job
                .prechild
                .lock()
                .map_err(|_| io::Error::other("original editor pre-child owner poisoned"))?;
            let resources = resources
                .as_mut()
                .ok_or_else(|| io::Error::other("original pre-child bundle absent"))?;
            resources
                .modes()
                .ok_or_else(|| io::Error::other("original editor modes absent"))?
                .release_for_original_handback(actual)?;
            handoff.finish_with(tui, || {
                resources
                    .input_mut()
                    .ok_or_else(|| io::Error::other("original editor parser absent"))?
                    .finish_after_original_handoff(actual)
            })?;
            // EXACT original completion order: Tui handback, reset, trim/apply or
            // original error history, then schedule a new native frame.
            self.reset_external_editor_state(tui);
            match actual.outcome() {
                Ok(new_text) => self
                    .chat_widget
                    .apply_external_edit(new_text.trim_end().to_string()),
                Err(error) => self
                    .chat_widget
                    .add_to_history(history_cell::new_error_event(format!(
                        "Failed to open editor: {error}"
                    ))),
            }
            tui.frame_requester().schedule_frame();
            Ok(())
        })?;
        jobs::retire_after_original_callback(job)?;
        self.alab_external_editor.active = None;
        Ok(())
    }
}

#[path = "retained_original_editor_jobs.rs"]
mod jobs;

#[path = "retained_original_editor_c0_stage.rs"]
mod c0;
pub(crate) use c0::Transfer as C0EditorTransfer;
