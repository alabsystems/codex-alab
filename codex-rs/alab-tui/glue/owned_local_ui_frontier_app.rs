// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual original App FIFO and draw consumer; this is not a ready facade.
use super::*;
use crate::app::session_lifecycle::ThreadAttachPresentation;
use crate::app_server_session::alab_retained_ui::Pending;
use crate::app_server_session::alab_retained_ui::ProjectionReplay;
use crate::app_server_session::alab_retained_ui::original_resume::ProjectedStartedThread;
use crate::session_state::ThreadSessionState;
use crate::tui::alab_retained_draw::DrawMark;
use codex_app_server_protocol::RetainedFrontierNotification;
use codex_app_server_protocol::Turn;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;

fn current(app: &App) -> Option<Arc<Pending>> {
    let crate::AppServerTarget::OwnedLocal { target } = &app.app_server_target else {
        return None;
    };
    target.ui_frontier()
}
/// Called on the actual session event before hidden-thread buffering. A failed
/// tail must not silently wait for that thread to become visible.
pub(super) fn failed_tail(
    app: &App,
    session: &AppServerSession,
    tail: &RetainedFrontierNotification,
) -> bool {
    let Some(pending) = current(app) else {
        return false;
    };
    if !pending.current_on(session) {
        return false;
    }
    match pending.terminal_failure(tail) {
        Ok(false) => false,
        Ok(true) | Err(_) => {
            app.app_event_tx.send(AppEvent::FatalExitRequest(
                "retained projection unavailable".into(),
            ));
            true
        }
    }
}
pub(super) fn thread_tail(app: &App, tail: &RetainedFrontierNotification) {
    let Some(pending) = current(app) else {
        return;
    };
    if pending.consumed_tail(tail).is_err() {
        app.app_event_tx.send(AppEvent::FatalExitRequest(
            "retained projection correlation failed".into(),
        ));
        return;
    }
    queue_end(app, &pending);
}
/// The original response consumer retains the token alongside the response
/// through unchanged history hydration and started-thread conversion, then
/// calls this instead of its original enqueue_primary_thread_session call.
/// No arbitrary later replay or queue-empty observation can finish this token.
pub(crate) async fn install_primary(
    app: &mut App,
    session: &mut AppServerSession,
    replay: ProjectionReplay,
    configured: ThreadSessionState,
    turns: Vec<Turn>,
) -> Result<()> {
    install_primary_with_presentation(
        app,
        session,
        replay,
        configured,
        turns,
        ThreadAttachPresentation::SessionLineage,
    )
    .await
}
pub(in crate::app) async fn install_primary_with_presentation(
    app: &mut App,
    session: &mut AppServerSession,
    replay: ProjectionReplay,
    configured: ThreadSessionState,
    turns: Vec<Turn>,
    presentation: ThreadAttachPresentation,
) -> Result<()> {
    if !replay.is_primary_presentation()? {
        bail!("side projection cannot replace the original primary consumer");
    }
    let pending = replay.pending();
    session.adopt_pending(pending)?;
    let installation = replay.begin_install(session, &configured.thread_id.to_string())?;
    // Existing full response processing remains the caller's original path;
    // this invokes that path's original complete installation/replay consumer.
    app.enqueue_primary_thread_session_with_presentation(configured, turns, presentation)
        .await?;
    installation.finish()?;
    queue_end(app, pending);
    Ok(())
}
/// Preserve the actual side attachment consumer and its original channel mode.
/// Its full started result remains paired with the one replay token; no side
/// installation is routed through the primary-setting enqueue method.
pub(in crate::app) async fn install_side(
    app: &mut App,
    session: &mut AppServerSession,
    projected: ProjectedStartedThread,
    channel_mode: super::thread_attachment::ChannelMode<'_>,
) -> Result<bool> {
    let (started, replay) = projected.into_parts();
    if replay.is_primary_presentation()? {
        bail!("primary projection cannot use the original side consumer");
    }
    let thread_id = started.session.thread_id;
    let primary = app.primary_thread_id;
    if primary == Some(thread_id) {
        bail!("side projection disagrees with the actual primary");
    }
    let pending = replay.pending();
    session.adopt_pending(pending)?;
    let installation = replay.begin_install(session, &thread_id.to_string())?;
    let installed = app
        .install_thread_attachment(
            session,
            thread_id,
            super::thread_attachment::PreparedAttachment::Live(Box::new(started)),
            channel_mode,
        )
        .await;
    if app.primary_thread_id != primary {
        bail!("side attachment changed the original primary");
    }
    if !installed {
        return Ok(false);
    }
    installation.finish()?;
    queue_end(app, pending);
    Ok(true)
}
fn queue_end(app: &App, pending: &Arc<Pending>) {
    if let Some(end) = pending.queue_end() {
        app.app_event_tx.send(AppEvent::AlabRetainedReplayEnd(end));
    }
}
pub(super) async fn after_actual_draw(
    app: &mut App,
    tui: &mut tui::Tui,
    session: &AppServerSession,
    before: &DrawMark,
) -> Result<()> {
    // Real ordinary C1/C2 handler application can retire endpoint cells without
    // waiting for another handoff. Errors retain same jobs for authenticated retry.
    if let Err(error) = app.alab_flush_callback_retirements(session).await {
        tracing::warn!(%error, "retained callback retirement remains owned");
    }
    // Both legacy/owned transcript and every overlay draw through the same Tui
    // success hooks. An event label, skipped paste tick or failed draw cannot
    // advance this exact Tui's private completion sequence.
    if !tui.alab_completed_draw_after(before) {
        return Ok(());
    }
    // An unmerged real provisional editor is still part of this App. A draw
    // of only the destination cannot acknowledge/promote the whole UI graph.
    if app.alab_has_pending_startup_editor() {
        return Ok(());
    }
    let Some(pending) = current(app) else {
        return Ok(());
    };
    if crate::retained_image_renderer::holds_selected_ui_ack(tui, &pending) {
        return Ok(());
    }
    let Some((projected, is_primary)) = pending.projection_target() else {
        return Ok(());
    };
    let Some(displayed) = app.current_displayed_thread_id() else {
        return Ok(());
    };
    // A completed draw of another thread does not advance this replay. Wait
    // until the actual selected projection occupies the real widget.
    if displayed.to_string() != projected || app.chat_widget.thread_id() != Some(displayed) {
        if !is_primary {
            return super::alab_retained_passive::after_actual_draw(
                app, tui, session, before, &pending,
            )
            .await;
        }
        return Ok(());
    }
    if app.app_server_backend_id != Some(session.backend_id()) {
        bail!("retained UI changed its actual backend");
    }
    if is_primary {
        if app.primary_thread_id != Some(displayed)
            || app
                .primary_session_configured
                .as_ref()
                .is_none_or(|state| state.thread_id != displayed)
        {
            bail!("retained UI changed its installed primary projection");
        }
    } else if app.primary_thread_id == Some(displayed) {
        bail!("retained side UI became the primary projection");
    }
    let primary = app.primary_thread_id;
    match app
        .alab_reconcile_callback_transfer(session, &pending)
        .await
    {
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
    if commit.thread_data() != projected || commit.is_primary_presentation() != is_primary {
        bail!("retained UI changed its actual presentation");
    }
    // Borrow actual pending requests/views/drafts and exact input owner across
    // both real observations. This is not a cross-process view/editor codec.
    let editor = app.chat_widget.alab_capture_runtime_editor()?;
    let input = tui.alab_prepare_upgrade_capture()?.with_editor(editor)?;
    if !input.valid() {
        bail!("retained UI input ownership changed");
    }
    commit.acknowledge().await?;
    if !input.valid()
        || app.primary_thread_id != primary
        || app.current_displayed_thread_id() != Some(displayed)
        || app.chat_widget.thread_id() != Some(displayed)
    {
        bail!("retained UI changed during acknowledgment");
    }
    commit.finish()?;
    if let Err(error) = app.alab_ack_applied_callbacks(session).await {
        tracing::warn!(%error, "retained postcommit callback ACK remains owned");
    }
    drop(input); // Uncommitted Drop resumes SAME queued parser; never exec-commit it.
    // The actual messages never left the original widget. Invoke its original
    // consumers only after actual Native+Source observation, not Gate alone.
    super::alab_retained_passive::release_opening(app, session, &pending)?;
    app.alab_release_configured_inputs(session, &pending)?;
    app.chat_widget.alab_observed_retained_startup(&pending)?;
    Ok(())
}

#[path = "owned_local_configured_submission_app.rs"]
mod configured_submission;
