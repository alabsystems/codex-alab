// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Pair the original result and sole replay through the original App install.
//! An ordinary materialized view needs no live session. A retained construction
//! always requires its actual session and independently observed projection.
use super::*;
use crate::app::session_lifecycle::ThreadAttachPresentation;
use crate::app_server_session::AppServerStartedThread;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

pub(crate) struct PrimaryInstallation {
    #[cfg(unix)]
    replay: Option<crate::app_server_session::alab_retained_ui::ProjectionReplay>,
}

pub(crate) async fn prepare_primary(
    app: &App,
    session: Option<&mut AppServerSession>,
    original: AppServerStartedThread,
) -> Result<(AppServerStartedThread, PrimaryInstallation)> {
    #[cfg(unix)]
    if let Some(record) = original.alab_construction.clone() {
        use crate::app_server_session::ResumeModelSettings;
        use crate::app_server_session::alab_retained_ui::original_resume::OriginalResume;
        let thread_id = original.session.thread_id;
        // Capture in the SAME actual Record before session/backend/native
        // preflight. On failure the whole original stays there, no error cast.
        let preparation = match record.capture_existing_primary(original) {
            Ok(preparation) => preparation,
            Err(failed) => {
                return Err(record.retain_rejected_primary(failed.original, failed.error));
            }
        };
        let Some(session) = session else {
            return Err(
                preparation.fail(eyre!("retained construction lost its actual App session"))
            );
        };
        if app.app_server_backend_id != Some(session.backend_id()) {
            return Err(preparation.fail(eyre!("retained construction changed the App backend")));
        }
        let mut view = session.attachment_view(thread_id);
        let operation = preparation.prepare(&view)?;
        // This Core is already the original construction result. The original
        // saved-thread attachment policy receives the complete actual App inputs.
        let projected = crate::boxed_future!(operation.reconcile(
            &mut view,
            OriginalResume {
                local_settings: &app.local_settings,
                config: app.config.clone(),
                thread_id,
                model_settings: ResumeModelSettings::PreserveExistingThread,
                permission_overrides: crate::resume_permissions::ResumePermissions::default(),
            },
        ))
        .await?;
        let (started, metadata, replay) = projected.into_parts();
        // Keep Pending in the actual live session before any widget/UI await.
        session.adopt_pending(replay.pending())?;
        session.install_attachment_metadata(thread_id, metadata, false);
        return Ok((
            started,
            PrimaryInstallation {
                replay: Some(replay),
            },
        ));
    }
    let _ = (app, session);
    Ok((
        original,
        PrimaryInstallation {
            #[cfg(unix)]
            replay: None,
        },
    ))
}

impl PrimaryInstallation {
    pub(crate) async fn install(
        self,
        app: &mut App,
        session: Option<&mut AppServerSession>,
        started: AppServerStartedThread,
        presentation: ThreadAttachPresentation,
    ) -> Result<()> {
        #[cfg(unix)]
        if let Some(replay) = self.replay {
            let session =
                session.ok_or_else(|| eyre!("retained replay lost its actual App session"))?;
            return crate::boxed_future!(
                super::alab_retained_ui::install_primary_with_presentation(
                    app,
                    session,
                    replay,
                    started.session,
                    started.turns,
                    presentation,
                )
            )
            .await;
        }
        #[cfg(unix)]
        if started.alab_construction.is_some() {
            bail!("retained result lost its single original replay carrier");
        }
        let _ = session;
        crate::boxed_future!(app.enqueue_primary_thread_session_with_presentation(
            started.session,
            started.turns,
            presentation,
        ))
        .await
    }
}

pub(crate) async fn install_primary(
    app: &mut App,
    session: &mut AppServerSession,
    started: AppServerStartedThread,
    presentation: ThreadAttachPresentation,
) -> Result<()> {
    let (started, installation) =
        crate::boxed_future!(prepare_primary(app, Some(&mut *session), started)).await?;
    crate::boxed_future!(installation.install(app, Some(&mut *session), started, presentation)).await
}
