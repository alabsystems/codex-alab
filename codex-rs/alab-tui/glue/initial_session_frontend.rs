// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retain the same construction owner before polling the complete original
//! Start/Fork/Resume future. Ordinary clients keep their original path.
use super::*;
#[cfg(unix)]
use crate::owned_local_ui_transfer::SavedResumeHandoff;
use codex_app_server_client::ClientPresentation;
use codex_app_server_client::InitialAction;
use codex_app_server_client::SavedResumeQuery;

pub(crate) enum ConstructionScope {
    Ordinary,
    #[cfg(unix)]
    Retained(crate::owned_local_frontend::ConstructionObservation),
}
impl ConstructionScope {
    pub(crate) fn run(
        self,
        original: impl Future<Output = Result<AppServerStartedThread>>,
    ) -> impl Future<Output = Result<AppServerStartedThread>> {
        // Keep the complete original future on the heap at this extension
        // boundary, including the ordinary branch of an enlarged caller.
        crate::boxed_future!(async move {
            match self {
                Self::Ordinary => original.await,
                #[cfg(unix)]
                Self::Retained(actual) => crate::boxed_future!(actual.run(original)).await,
            }
        })
    }
}

impl AppServerSession {
    /// Only the provisional chooser view records an unreadable original success.
    /// Other callers keep the ordinary client and its existing fallback policy.
    pub(crate) async fn alab_request_resume(
        &mut self,
        request_id: RequestId,
        params: ThreadResumeParams,
    ) -> std::result::Result<ThreadResumeResponse, TypedRequestError> {
        if let Some(unreadable) = &mut self.alab_saved_resume_unreadable {
            let (result, received_unreadable, lost_original) = crate::boxed_future!(
                self.request_handle
                    .request_saved_resume_observed(request_id, params)
            )
            .await;
            *unreadable = received_unreadable;
            #[cfg(unix)]
            if lost_original
                && let Some(saved) = &mut self.alab_saved_opening
                && let Some(record) = &saved.record
            {
                // Preparation and projection failures cannot produce this ticket.
                saved.missing = record.saved_resume_query(saved.child);
            }
            #[cfg(not(unix))]
            let _ = lost_original;
            return result;
        }
        self.request_handle
            .request_typed(ClientRequest::ThreadResume { request_id, params })
            .await
    }

    pub(crate) fn alab_original_construction(
        &mut self,
        action: InitialAction,
        config: &Config,
        presentation: ClientPresentation,
    ) -> Result<ConstructionScope> {
        let presentation = resume_intent::for_action(&action, presentation);
        #[cfg(unix)]
        if let Some(tenure) = self.retained_frontend() {
            // The genuine R/S2 consumer already owns its Pending and exact
            // original Resume route. It must not create another construction.
            if codex_app_server_client::retained_frontier::original_policy_active() {
                if !matches!(action, InitialAction::Resume(_))
                    || self
                        .alab_retained_ui
                        .as_ref()
                        .is_none_or(|actual| !actual.current_on(self))
                {
                    color_eyre::eyre::bail!("original projection changed its actual pending owner");
                }
                return Ok(ConstructionScope::Ordinary);
            }
            if codex_alab_tui::upgrade_state::pending().is_some() {
                color_eyre::eyre::bail!(
                    "retained construction cannot restore a source-exec forest"
                );
            }
            let prospective = match action {
                InitialAction::Start => None,
                InitialAction::Fork(_) | InitialAction::Resume(_) => {
                    Some(self.prospective_task_start_params(config))
                }
            };
            let saved = self
                .alab_saved_opening
                .as_ref()
                .map(|s| (s.opening, s.child));
            let actual =
                tenure.begin_original_construction(action, presentation, prospective, saved)?;
            if let Some(saved) = &mut self.alab_saved_opening {
                saved.record = Some(actual.saved_resume_record());
            }
            return Ok(ConstructionScope::Retained(actual));
        }
        let _ = (action, config, presentation);
        Ok(ConstructionScope::Ordinary)
    }
}

#[cfg(unix)]
pub(crate) struct SavedOpening {
    opening: uuid::Uuid,
    child: ThreadId,
    record: Option<Arc<crate::owned_local_frontend::ConstructionRecord>>,
    missing: Option<SavedResumeQuery>,
}

/// Historical read authority is supplied by the connected session independently.
pub(crate) struct MissingSavedResume {
    #[cfg(unix)]
    target: crate::owned_local_frontend::OwnedLocalTarget,
    #[cfg(unix)]
    source: SavedResumeSource,
    #[cfg(not(unix))]
    query: Arc<SavedResumeQuery>,
}

#[cfg(unix)]
enum SavedResumeSource {
    Original {
        query: Arc<SavedResumeQuery>,
        record: Arc<crate::owned_local_frontend::ConstructionRecord>,
    },
    Handoff(SavedResumeHandoff),
}
impl std::fmt::Debug for MissingSavedResume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MissingSavedResume")
    }
}
impl MissingSavedResume {
    /// Test data for a decoded opening whose owned target has no current delivery.
    #[cfg(all(unix, test))]
    pub(crate) fn for_disconnected_target(
        target: crate::owned_local_frontend::OwnedLocalTarget,
        handoff: SavedResumeHandoff,
    ) -> Self {
        Self {
            target,
            source: SavedResumeSource::Handoff(handoff),
        }
    }

    #[cfg(unix)]
    pub(crate) fn restored(
        server: &AppServerSession,
        handoff: SavedResumeHandoff,
    ) -> std::io::Result<Self> {
        let target = server
            .retained_frontend()
            .ok_or_else(|| {
                std::io::Error::other("saved resume history requires the connected owned backend")
            })?
            .target
            .clone();
        Ok(Self {
            target,
            source: SavedResumeSource::Handoff(handoff),
        })
    }

    pub(crate) fn query(&self) -> &Arc<SavedResumeQuery> {
        #[cfg(unix)]
        {
            match &self.source {
                SavedResumeSource::Original { query, .. } => query,
                SavedResumeSource::Handoff(handoff) => handoff.query(),
            }
        }
        #[cfg(not(unix))]
        {
            &self.query
        }
    }

    pub(crate) fn finish_local_history(&self) {
        #[cfg(unix)]
        if let SavedResumeSource::Original { record, .. } = &self.source {
            record.finish_saved_resume_history();
        }
    }
    pub(crate) fn belongs_to(&self, server: &AppServerSession) -> bool {
        #[cfg(unix)]
        {
            server
                .retained_frontend()
                .is_some_and(|tenure| self.target == tenure.target)
        }
        #[cfg(not(unix))]
        {
            let _ = server;
            false
        }
    }
}
impl AppServerSession {
    pub(crate) fn alab_saved_attachment_view(&self, child: ThreadId, opening: uuid::Uuid) -> Self {
        let mut view = self.attachment_view(child);
        #[cfg(unix)]
        if view.retained_frontend().is_some() {
            view.alab_saved_opening = Some(SavedOpening {
                opening,
                child,
                record: None,
                missing: None,
            });
        }
        #[cfg(not(unix))]
        let _ = opening;
        view
    }

    pub(crate) fn alab_saved_resume_missing(&self) -> bool {
        #[cfg(unix)]
        {
            self.alab_saved_opening
                .as_ref()
                .is_some_and(|s| s.missing.is_some())
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    pub(crate) fn alab_take_missing_saved_resume(&mut self) -> Option<MissingSavedResume> {
        #[cfg(unix)]
        {
            let saved = self.alab_saved_opening.take()?;
            let query = saved.missing?;
            let record = saved.record?;
            let target = self.retained_frontend()?.target.clone();
            record.retain_saved_resume_history();
            Some(MissingSavedResume {
                target,
                source: SavedResumeSource::Original {
                    query: Arc::new(query),
                    record,
                },
            })
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
}
#[path = "owned_local_resume_intent.rs"]
mod resume_intent;
impl AppServerSession {
    /// Caller chooses actual presentation DATA before its original Resume
    /// future is polled/S1 is registered. Does not classify generic prepare().
    pub(crate) async fn alab_with_original_resume_presentation<T>(
        presentation: ClientPresentation,
        original: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        resume_intent::scope(presentation, original).await
    }
}
