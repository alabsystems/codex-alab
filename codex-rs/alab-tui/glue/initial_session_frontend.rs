// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retain the same construction owner before polling the complete original
//! Start/Fork/Resume future. Ordinary clients keep their original path.
use super::*;
use codex_app_server_client::ClientPresentation;
use codex_app_server_client::InitialAction;

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
    pub(crate) fn alab_original_construction(
        &self,
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
            let actual = tenure.begin_original_construction(action, presentation, prospective)?;
            return Ok(ConstructionScope::Retained(actual));
        }
        let _ = (action, config, presentation);
        Ok(ConstructionScope::Ordinary)
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
