// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual ORIGINAL passive-side installation, ordered store application and
//! successful pane presentation. Resource identity is local custody, not SDK
//! admission; Native/Source final operations remain the existing real Commit.
use super::*;
use crate::app_server_session::SidebarFork;
use crate::app_server_session::alab_retained_ui::Installation;
use crate::app_server_session::alab_retained_ui::Pending;
use crate::app_server_session::alab_retained_ui::ProjectionReplay;
use crate::tui::alab_retained_draw::DrawMark;
use codex_app_server_protocol::RetainedFrontierNotification;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use std::sync::Arc;
use std::sync::Mutex as SyncMutex;
use std::sync::PoisonError;

pub(crate) struct CreationReplay {
    replay: ProjectionReplay,
    history: super::history_pagination::alab_retained_side_history::Snapshot,
}
impl std::fmt::Debug for CreationReplay {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("CreationReplay(<actual original side projection; no grant>)")
    }
}
impl SidebarFork {
    /// The real project_existing producer supplies its actual non-clone replay
    /// AFTER the original Fork/Started conversion and real history capture. Keep that ORIGINAL object,
    /// boundary/storage/profile and history policy, without a second Resume
    /// Started conversion or replacement. Fresh runtime fields are reconciled
    /// by the peer's original mapper; this SAME history resource must reach the
    /// real store and companion before completion.
    pub(crate) fn with_retained_side_history(
        mut self,
        session: &AppServerSession,
        replay: ProjectionReplay,
        history: super::history_pagination::alab_retained_side_history::Snapshot,
    ) -> Result<Self> {
        let mut guard = CreationOutcomeGuard {
            pending: replay.pending().clone(),
            complete: false,
        };
        if self.boundary.is_err()
            || self.alab_retained_replay.is_some()
            || replay.is_primary_presentation()?
            || replay.pending().projection_target()
                != Some((self.thread.session.thread_id.to_string(), false))
        {
            bail!("side construction changed its original projection");
        }
        replay.apply_task_tool_observation(session, &mut self.thread)?;
        self.alab_retained_replay = Some(CreationReplay { replay, history });
        guard.complete = true;
        Ok(self)
    }
}
struct CreationOutcomeGuard {
    pending: Arc<Pending>,
    complete: bool,
}
impl Drop for CreationOutcomeGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.pending.abandon_ui();
        }
    }
}
pub(crate) struct CreationInstall {
    _replay: ProjectionReplay,
    pending: Arc<Pending>,
    installation: Installation,
    thread: ThreadId,
    backend: crate::backend_id::BackendId,
    history: super::history_pagination::alab_retained_side_history::Snapshot,
}
pub(crate) fn begin_creation(
    app: &mut App,
    session: &mut AppServerSession,
    fork: &mut SidebarFork,
) -> Result<Option<CreationInstall>> {
    let Some(CreationReplay {
        replay,
        mut history,
    }) = fork.alab_retained_replay.take()
    else {
        return Ok(None);
    };
    let mut guard = CreationOutcomeGuard {
        pending: replay.pending().clone(),
        complete: false,
    };
    if replay.is_primary_presentation()? {
        bail!("passive side cannot nominate primary");
    }
    let thread = fork.thread.session.thread_id;
    let pending = replay.pending().clone();
    session.adopt_pending(&pending)?; // SAME owner survives original side snapshot await.
    let installation = replay.begin_install(session, &thread.to_string())?;
    history.begin_install(app, session)?;
    guard.complete = true;
    Ok(Some(CreationInstall {
        _replay: replay,
        pending,
        installation,
        thread,
        backend: session.backend_id(),
        history,
    }))
}
/// Issued ONLY by actual entry allocation/lookup immediately before the
/// unchanged original side store installation; no caller supplies a ready flag.
pub(crate) struct CompanionBirth {
    identity: Arc<()>,
    fresh: bool,
}
pub(crate) fn ensure_companion(
    app: &mut App,
    thread: ThreadId,
    cwd: codex_utils_absolute_path::AbsolutePathBuf,
) -> CompanionBirth {
    use std::collections::hash_map::Entry;
    let (companion, fresh) = match app.side_companions.entry(thread) {
        Entry::Vacant(slot) => (
            slot.insert(super::side_companion::SideCompanion::empty(cwd)),
            true,
        ),
        Entry::Occupied(slot) => (slot.into_mut(), false),
    };
    CompanionBirth {
        identity: companion.alab_identity.clone(),
        fresh,
    }
}
pub(crate) struct PassiveBinding {
    pending: Arc<Pending>,
    thread: ThreadId,
    backend: crate::backend_id::BackendId,
    store: Arc<tokio::sync::Mutex<ThreadEventStore>>,
    companion: SyncMutex<Arc<()>>,
    presented: SyncMutex<Option<Presented>>,
}
impl std::fmt::Debug for PassiveBinding {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("PassiveBinding(<same actual side/store/projection; no grant>)")
    }
}
struct Presented {
    draw: DrawMark,
}
pub(crate) async fn finish_creation(
    app: &mut App,
    tui: &mut crate::tui::Tui,
    session: &AppServerSession,
    mut install: CreationInstall,
    birth: Option<CompanionBirth>,
) -> Result<()> {
    if !install.pending.current_on(session) || session.backend_id() != install.backend {
        bail!("side installation changed actual backend/delivery");
    }
    if let Some(birth) = birth {
        install
            .history
            .apply_companion(app, tui, session, &birth.identity, birth.fresh)
            .await?;
        let channel = app
            .thread_event_channels
            .get(&install.thread)
            .ok_or_else(|| eyre!("original side event channel missing"))?;
        if channel.backend_id != Some(install.backend) {
            bail!("side channel changed backend");
        }
        let store = channel.store.clone();
        let mut tails = Vec::new();
        {
            let guard = store.lock().await;
            if guard.active
                || guard
                    .session
                    .as_ref()
                    .is_none_or(|state| state.thread_id != install.thread)
            {
                bail!("passive side no longer owns original installed snapshot");
            }
            let companion = app
                .side_companions
                .get_mut(&install.thread)
                .ok_or_else(|| eyre!("original passive companion missing"))?;
            if !Arc::ptr_eq(&companion.alab_identity, &birth.identity)
                || companion.alab_retained.is_some()
            {
                bail!("passive companion resource changed or already retained");
            }
            // A newly allocated ORIGINAL empty side view needs its already-
            // received live prefix. Existing captured views already consumed
            // that original receive path and MUST NOT replay deltas twice.
            for event in &guard.buffer {
                if let ThreadBufferedEvent::Notification(notification) = event {
                    if birth.fresh && !install.history.prefix_applied() {
                        companion.apply_notification(notification, &guard, || {
                            app.chat_widget.new_status_indicator()
                        });
                    }
                    if let ServerNotification::AlabRetainedFrontier(tail) = notification.as_ref()
                        && install.pending.matches_frontier_data(&tail.frontier)
                    {
                        tails.push(tail.clone());
                    }
                }
            }
            let binding = Arc::new(PassiveBinding {
                pending: install.pending.clone(),
                thread: install.thread,
                backend: install.backend,
                store: store.clone(),
                companion: SyncMutex::new(birth.identity),
                presented: SyncMutex::new(None),
            });
            companion.alab_retained = Some(binding.clone());
            let side = app
                .side_threads
                .get_mut(&install.thread)
                .ok_or_else(|| eyre!("original side state missing"))?;
            if side.alab_retained.is_some() {
                bail!("original side state already owns projection");
            }
            side.alab_retained = Some(binding);
        }
        if tails.is_empty() && install.pending.received_own_tail() {
            // Do not promote a cached physical receipt to application of an
            // evicted original store event. No resend/cold Resume substitutes.
            bail!("required side tail was not retained in original event store");
        }
        for tail in tails {
            install.pending.consumed_tail(&tail)?;
        }
    }
    install.installation.finish()?;
    queue_end(app, &install.pending);
    Ok(())
}
fn queue_end(app: &App, pending: &Arc<Pending>) {
    if let Some(end) = pending.queue_end() {
        app.app_event_tx.send(AppEvent::AlabRetainedReplayEnd(end));
    }
}
fn current_binding(app: &App, thread: ThreadId) -> Option<Arc<PassiveBinding>> {
    let companion = app.side_companions.get(&thread)?;
    let binding = companion.alab_retained.as_ref()?;
    let channel = app.thread_event_channels.get(&thread)?;
    if !Arc::ptr_eq(
        &companion.alab_identity,
        &binding
            .companion
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    ) || !app
        .side_threads
        .get(&thread)
        .and_then(|side| side.alab_retained.as_ref())
        .is_some_and(|actual| Arc::ptr_eq(actual, binding))
        || !Arc::ptr_eq(&channel.store, &binding.store)
        || channel.backend_id != Some(binding.backend)
    {
        return None;
    }
    Some(binding.clone())
}
/// Only the ORIGINAL ParkSideCompanion event calls this after moving the
/// real display/draft/view from the live widget into its new parked owner.
pub(crate) fn rebind_parked(app: &mut App, thread: ThreadId) -> Result<()> {
    let Some(binding) = app
        .side_threads
        .get(&thread)
        .and_then(|side| side.alab_retained.clone())
    else {
        return Ok(());
    };
    let channel = app
        .thread_event_channels
        .get(&thread)
        .ok_or_else(|| eyre!("parked side channel missing"))?;
    if channel.backend_id != Some(binding.backend) || !Arc::ptr_eq(&channel.store, &binding.store) {
        binding.pending.abandon_ui();
        bail!("parked side changed original store/backend");
    }
    let companion = app
        .side_companions
        .get_mut(&thread)
        .ok_or_else(|| eyre!("original parked display missing"))?;
    *binding
        .companion
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = companion.alab_identity.clone();
    *binding
        .presented
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = None;
    companion.alab_retained = Some(binding);
    Ok(())
}
/// Called only AFTER original inactive companion application AND store.push,
/// after its guard is dropped. Active/focused events retain their old FIFO path.
pub(crate) fn consumed(
    app: &App,
    thread: ThreadId,
    tail: &RetainedFrontierNotification,
) -> Result<()> {
    let Some(binding) = current_binding(app, thread) else {
        return Ok(());
    };
    if !binding.pending.matches_frontier_data(&tail.frontier) {
        return Ok(());
    }
    binding.pending.consumed_tail(tail)?;
    queue_end(app, &binding.pending);
    Ok(())
}
pub(crate) struct TentativePaint {
    binding: Arc<PassiveBinding>,
    companion: Arc<()>,
}
/// Private candidate only from the ORIGINAL real pane paint, never an ACK.
pub(in crate::app) fn painted(
    companion: &super::side_companion::SideCompanion,
    panes: &super::side_pane::SidePaneLayout,
) -> Option<TentativePaint> {
    let binding = companion.alab_retained.as_ref()?;
    if panes.passive.is_empty()
        || panes.passive_thread != binding.thread
        || !Arc::ptr_eq(
            &companion.alab_identity,
            &binding
                .companion
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    {
        return None;
    }
    Some(TentativePaint {
        binding: binding.clone(),
        companion: companion.alab_identity.clone(),
    })
}
/// Invoked only after the ORIGINAL synchronized Tui draw returned success.
pub(crate) fn presented(app: &App, tui: &tui::Tui, candidate: Option<TentativePaint>) {
    let Some(candidate) = candidate else {
        return;
    };
    if current_binding(app, candidate.binding.thread)
        .is_none_or(|current| !Arc::ptr_eq(&current, &candidate.binding))
        || !Arc::ptr_eq(
            &candidate.companion,
            &candidate
                .binding
                .companion
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    {
        return;
    }
    *candidate
        .binding
        .presented
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(Presented {
        draw: tui.alab_draw_checkpoint(),
    });
}
pub(crate) async fn after_actual_draw(
    app: &mut App,
    tui: &mut tui::Tui,
    session: &AppServerSession,
    before: &DrawMark,
    pending: &Arc<Pending>,
) -> Result<()> {
    if !tui.alab_completed_draw_after(before) || !pending.current_on(session) {
        return Ok(());
    }
    let Some((thread, false)) = pending.projection_target() else {
        return Ok(());
    };
    let thread = ThreadId::from_string(&thread)?;
    let Some(binding) = current_binding(app, thread) else {
        return Ok(());
    };
    if !Arc::ptr_eq(&binding.pending, pending) || session.backend_id() != binding.backend {
        return Ok(());
    }
    let painted = binding
        .presented
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|actual| tui.alab_is_current_draw(&actual.draw));
    if !painted {
        return Ok(());
    }
    {
        let store = binding.store.lock().await;
        if store.active
            || store
                .session
                .as_ref()
                .is_none_or(|state| state.thread_id != thread)
        {
            bail!("passive side snapshot changed before UI acknowledgment");
        }
    }
    let primary = app.primary_thread_id;
    let displayed = app.current_displayed_thread_id();
    match app.alab_reconcile_callback_transfer(session, pending).await {
        Ok(true) => {
            tui.frame_requester().schedule_frame();
            return Ok(());
        }
        Ok(false) => {}
        Err(error) => {
            app.alab_callback_restore_blocked(session, &error);
            return Ok(());
        }
    }
    let Some(commit) = pending.begin_commit(session)? else {
        return Ok(());
    };
    if commit.is_primary_presentation() || commit.thread_data() != thread.to_string() {
        bail!("passive receipt changed actual side presentation");
    }
    // App's actual focused editor, passive display/draft/view, original event
    // store/requests and opening-message owners all remain borrowed/retained.
    // No serialization or cross-image view transfer is claimed here.
    let editor = app.chat_widget.alab_capture_runtime_editor()?;
    let input = tui.alab_prepare_upgrade_capture()?.with_editor(editor)?;
    if !input.valid() {
        bail!("passive input owner changed");
    }
    commit.acknowledge().await?;
    if !input.valid()
        || app.primary_thread_id != primary
        || app.current_displayed_thread_id() != displayed
        || current_binding(app, thread).is_none_or(|current| !Arc::ptr_eq(&current, &binding))
    {
        bail!("passive UI ownership changed during acknowledgment");
    }
    commit.finish()?;
    if let Err(error) = app.alab_ack_applied_callbacks(session).await {
        tracing::warn!(%error, "retained passive callback ACK remains owned");
    }
    drop(input);
    // Preserve normal passive semantics: do not submit opening_message until
    // the original side becomes the actual live focused thread.
    release_opening(app, session, pending)?;
    app.chat_widget.alab_observed_retained_startup(pending)?;
    Ok(())
}
pub(crate) fn may_release_opening(app: &App) -> bool {
    match &app.app_server_target {
        crate::AppServerTarget::OwnedLocal { target } => !target.holds_startup_submission(),
        _ => true,
    }
}
/// Hold the SAME accepted message before the immediate focused-side submit.
pub(crate) fn hold_opening(
    app: &mut App,
    thread: ThreadId,
    message: &mut Option<crate::chatwidget::UserMessage>,
) -> bool {
    if may_release_opening(app) {
        return false;
    }
    if let Some(actual) = message.take() {
        if let Some(side) = app.side_threads.get_mut(&thread)
            && side.opening_message.is_none()
        {
            side.opening_message = Some(actual);
        } else {
            // Never overwrite/drop another original pending input owner.
            app.restore_side_user_message(Some(actual));
        }
    }
    true
}
pub(crate) fn release_opening(
    app: &mut App,
    session: &AppServerSession,
    pending: &Arc<Pending>,
) -> Result<()> {
    if pending.holds_submission() || !pending.current_on(session) {
        bail!("side input lacks actual completion");
    }
    let Some((thread, false)) = pending.projection_target() else {
        return Ok(());
    };
    let thread = ThreadId::from_string(&thread)?;
    if app.active_thread_id != Some(thread)
        || app.current_displayed_thread_id() != Some(thread)
        || app.chat_widget.thread_id() != Some(thread)
        || app.app_server_backend_id != Some(session.backend_id())
        || app
            .thread_event_channels
            .get(&thread)
            .is_none_or(|channel| {
                channel.backend_id != Some(session.backend_id())
                    || channel.attachment() != ThreadEventAttachment::Live
            })
    {
        return Ok(());
    }
    if let Some(message) = app
        .side_threads
        .get_mut(&thread)
        .and_then(|side| side.opening_message.take())
    {
        app.chat_widget
            .submit_user_message_as_plain_user_turn(message);
    }
    Ok(())
}
pub(crate) fn matches_deferred_tail(app: &App, tail: &RetainedFrontierNotification) -> bool {
    let crate::AppServerTarget::OwnedLocal { target } = &app.app_server_target else {
        return false;
    };
    target
        .ui_frontier()
        .is_some_and(|pending| pending.matches_frontier_data(&tail.frontier))
}

/// Called under the original SAME store guard; failure retains Unknown and
/// finish_creation cannot publish End or a paint witness for this install.
pub(in crate::app) fn install_creation_store(
    install: &mut CreationInstall,
    actual: &Arc<tokio::sync::Mutex<ThreadEventStore>>,
    store: &mut ThreadEventStore,
    session: crate::session_state::ThreadSessionState,
    turns: Vec<Turn>,
) {
    if install
        .history
        .install_store(actual, store, session, turns)
        .is_err()
    {
        install.pending.abandon_ui();
    }
}
