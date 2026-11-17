// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original attachment owns its policy, result and single replay together.
//! Saved children keep their existing Incoming; selection keeps ReplaceLive.
use super::thread_attachment::Prepared;
use super::thread_attachment::PreparedAttachment;
use super::*;
use crate::app_server_session::AttachmentMetadata;
use crate::app_server_session::ResumeModelSettings;
use codex_app_server_client::ClientPresentation;

pub(super) async fn prepare_saved_result(
    view: &mut AppServerSession,
    settings: &crate::local_settings::LocalSettings,
    config: &Config,
    child: ThreadId,
    mode: ResumeModelSettings,
) -> Result<PreparedAttachment> {
    // This caller is the original saved-child chooser, including loaded children
    // whose independent model-settings policy is PreserveExistingThread.
    crate::boxed_future!(AppServerSession::alab_with_original_resume_presentation(
        ClientPresentation::SideConversation,
        super::thread_attachment::prepare(view, settings, config, child, mode),
    ))
    .await
}

pub(super) async fn prepare_selection(
    app: &App,
    server: &mut AppServerSession,
    thread: ThreadId,
    mode: ResumeModelSettings,
) -> Result<PreparedAttachment> {
    let presentation = if app.side_threads.contains_key(&thread) {
        ClientPresentation::SideConversation
    } else {
        ClientPresentation::Primary
    };
    #[cfg(unix)]
    if server.retained_frontend().is_some() {
        // The request-only view exists before the original Resume/S1. Its
        // namespace and original unread/pagination boundary survive projection.
        let mut view = server.attachment_view(thread);
        let mut prepared =
            crate::boxed_future!(AppServerSession::alab_with_original_resume_presentation(
                presentation,
                super::thread_attachment::prepare(
                    &mut view,
                    &app.local_settings,
                    &app.config,
                    thread,
                    mode,
                ),
            ))
            .await?;
        let metadata = take_metadata(&mut view, &mut prepared, thread)?;
        server.install_attachment_metadata(thread, metadata, false);
        return Ok(prepared);
    }
    crate::boxed_future!(AppServerSession::alab_with_original_resume_presentation(
        presentation,
        super::thread_attachment::prepare(server, &app.local_settings, &app.config, thread, mode),
    ))
    .await
}

#[cfg(unix)]
pub(crate) struct OriginalPolicy {
    config: Config,
    mode: ResumeModelSettings,
}

#[cfg(unix)]
pub(super) fn capture_policy(
    server: &AppServerSession,
    config: &Config,
    mode: ResumeModelSettings,
) -> Option<OriginalPolicy> {
    server.retained_frontend().map(|_| OriginalPolicy {
        config: config.clone(),
        mode,
    })
}

#[cfg(unix)]
pub(crate) struct RetainedAttachment {
    started: Option<AppServerStartedThread>,
    current: Option<codex_app_server_protocol::ThreadResumeResponse>,
    metadata: Option<AttachmentMetadata>,
    replay: Option<crate::app_server_session::alab_retained_ui::ProjectionReplay>,
}

#[cfg(unix)]
impl std::fmt::Debug for RetainedAttachment {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RetainedAttachment(<original result and sole replay>)")
    }
}

#[cfg(unix)]
impl Drop for RetainedAttachment {
    fn drop(&mut self) {
        if let Some(replay) = &self.replay {
            replay.pending().abandon_ui();
        }
    }
}

#[cfg(unix)]
pub(super) async fn prepare_result(
    view: &mut AppServerSession,
    settings: &crate::local_settings::LocalSettings,
    policy: Option<OriginalPolicy>,
    started: AppServerStartedThread,
) -> Result<PreparedAttachment> {
    use crate::app_server_session::alab_retained_ui::original_resume::OriginalResume;
    use color_eyre::eyre::eyre;
    let Some(record) = started.alab_construction.clone() else {
        return Ok(PreparedAttachment::Live(Box::new(started)));
    };
    let thread_id = started.session.thread_id;
    // Capture the complete original before validating any later App/view input.
    let (started, current, metadata, replay) = match record.presentation_data() {
        ClientPresentation::Primary => {
            let preparation = record
                .capture_existing_primary(started)
                .map_err(|failed| record.retain_rejected_primary(failed.original, failed.error))?;
            let Some(policy) = policy else {
                return Err(preparation.fail(eyre!("original attachment policy missing")));
            };
            let operation = preparation.prepare(view)?;
            let projected = crate::boxed_future!(operation.reconcile(
                view,
                OriginalResume {
                    local_settings: settings,
                    config: policy.config,
                    thread_id,
                    model_settings: policy.mode,
                    permission_overrides: crate::resume_permissions::ResumePermissions::default(),
                },
            ))
            .await?;
            let (started, metadata, replay) = projected.into_parts();
            (started, None, metadata, replay)
        }
        ClientPresentation::SideConversation => {
            let preparation = record
                .capture_existing_side_attachment(started)
                .map_err(|failed| record.retain_rejected_primary(failed.original, failed.error))?;
            let Some(policy) = policy else {
                return Err(preparation.fail(eyre!("original side attachment policy missing")));
            };
            let operation = preparation.prepare(view)?;
            let projected = crate::boxed_future!(operation.reconcile(
                view,
                OriginalResume {
                    local_settings: settings,
                    config: policy.config,
                    thread_id,
                    model_settings: policy.mode,
                    permission_overrides: crate::resume_permissions::ResumePermissions::default(),
                },
            ))
            .await?;
            let (started, current, metadata, replay) = projected.into_parts();
            (started, Some(current), metadata, replay)
        }
    };
    Ok(PreparedAttachment::Retained(Box::new(RetainedAttachment {
        started: Some(started),
        current,
        metadata: Some(metadata),
        replay: Some(replay),
    })))
}

pub(super) fn is_live(prepared: &PreparedAttachment) -> bool {
    match prepared {
        PreparedAttachment::Live(_) => true,
        #[cfg(unix)]
        PreparedAttachment::Retained(_) => true,
        PreparedAttachment::ReplayOnly(_) => false,
    }
}

pub(super) fn take_metadata(
    view: &mut AppServerSession,
    prepared: &mut PreparedAttachment,
    thread: ThreadId,
) -> Result<AttachmentMetadata> {
    #[cfg(unix)]
    if let PreparedAttachment::Retained(actual) = prepared {
        return actual.metadata.take().ok_or_else(|| {
            color_eyre::eyre::eyre!("original attachment metadata already consumed")
        });
    }
    let _ = prepared;
    Ok(view.take_attachment_metadata(thread))
}

pub(super) fn finish_saved(
    view: &mut AppServerSession,
    mut result: std::result::Result<PreparedAttachment, String>,
    thread: ThreadId,
) -> Prepared {
    let metadata = match result
        .as_mut()
        .map(|prepared| take_metadata(view, prepared, thread))
    {
        Ok(Ok(metadata)) => metadata,
        Ok(Err(error)) => {
            result = Err(error.to_string());
            view.take_attachment_metadata(thread)
        }
        Err(_) => view.take_attachment_metadata(thread),
    };
    Prepared {
        result,
        metadata,
        close_check: Ok(false),
    }
}

#[cfg(unix)]
struct BeginOutcome {
    pending: Arc<crate::app_server_session::alab_retained_ui::Pending>,
    complete: bool,
}

#[cfg(unix)]
impl Drop for BeginOutcome {
    fn drop(&mut self) {
        if !self.complete {
            self.pending.abandon_ui();
        }
    }
}

#[cfg(unix)]
pub(super) struct AttachmentInstallation {
    replay: crate::app_server_session::alab_retained_ui::ProjectionReplay,
    _current: Option<codex_app_server_protocol::ThreadResumeResponse>,
    installation: crate::app_server_session::alab_retained_ui::Installation,
}

#[cfg(unix)]
pub(super) fn begin_install(
    server: &mut AppServerSession,
    thread: ThreadId,
    prepared: PreparedAttachment,
) -> Result<(PreparedAttachment, Option<AttachmentInstallation>)> {
    let PreparedAttachment::Retained(mut actual) = prepared else {
        return Ok((prepared, None));
    };
    if actual.metadata.is_some() {
        color_eyre::eyre::bail!("original attachment metadata was not delivered");
    }
    let replay = actual
        .replay
        .take()
        .ok_or_else(|| color_eyre::eyre::eyre!("original attachment replay missing"))?;
    let pending = replay.pending();
    let mut outcome = BeginOutcome {
        pending: pending.clone(),
        complete: false,
    };
    server.adopt_pending(pending)?;
    let installation = replay.begin_install(server, &thread.to_string())?;
    let mut started = actual
        .started
        .take()
        .ok_or_else(|| color_eyre::eyre::eyre!("original attachment result missing"))?;
    let current = actual.current.take();
    if let Some(current) = &current {
        // The existing attachment consumer owns its original settings/live-turn
        // precedence. Supply only the actual fresh hydrated history vector here.
        started.turns = current.thread.turns.clone();
    }
    outcome.complete = true;
    Ok((
        PreparedAttachment::Live(Box::new(started)),
        Some(AttachmentInstallation {
            replay,
            _current: current,
            installation,
        }),
    ))
}

#[cfg(unix)]
pub(super) fn finish_install(
    app: &App,
    install: Option<AttachmentInstallation>,
    live: bool,
) -> Result<()> {
    let Some(install) = install else {
        return Ok(());
    };
    if !live {
        color_eyre::eyre::bail!("retained attachment closed during original installation");
    }
    let pending = install.replay.pending().clone();
    install.installation.finish()?;
    if let Some(end) = pending.queue_end() {
        app.app_event_tx.send(AppEvent::AlabRetainedReplayEnd(end));
    }
    Ok(())
}
