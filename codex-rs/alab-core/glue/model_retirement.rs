// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::Session;
use super::SessionConfiguration;
use super::TurnContext;
use super::TurnSettingsUpdate;
use super::TurnSettingsUpdateOutcome;
use crate::config::Config;
use crate::session::step_context::StepContext;
use crate::session::step_settings::ResolvedStepSettings;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::RefreshedModelCatalog;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::Result;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelOrigin;
use codex_protocol::protocol::SessionSource;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::SemaphorePermit;

#[path = "model_retirement_input.rs"]
mod input;
#[path = "model_retirement_stream.rs"]
mod stream;

struct RetriedModel {
    destination: String,
    catalog: Arc<[ModelInfo]>,
    continuation: bool,
}

struct RequestedModel;

struct RejectedModel {
    owner: Arc<RefreshedModelCatalog>,
    replay: bool,
}

pub(super) struct ConfirmedRetirement {
    turn: Arc<TurnContext>,
    settings: Arc<ResolvedStepSettings>,
    config: Arc<Config>,
    catalog: Arc<[ModelInfo]>,
    destination: String,
}

impl ConfirmedRetirement {
    pub(super) fn still_targets(
        &self,
        session: &Session,
        turn: &Arc<TurnContext>,
        configuration: &SessionConfiguration,
    ) -> bool {
        let model = configuration.step_settings.collaboration_mode.model();
        Arc::ptr_eq(turn, &self.turn)
            && Arc::ptr_eq(&turn.next_step_settings.load_full(), &self.settings)
            && Arc::ptr_eq(&configuration.provider, &turn.provider)
            && Arc::ptr_eq(&configuration.original_config_do_not_use, &self.config)
            && (model == self.settings.model_info.slug || model == self.destination)
            && session
                .services
                .models_manager
                .refreshed_model_catalog()
                .is_some_and(|latest| Arc::ptr_eq(&latest, &self.catalog))
    }
}

impl TurnContext {
    pub(in crate::session) fn alab_keep_requested_model(&self, explicit: bool) {
        if explicit && codex_alab_core::active() {
            self.extension_data.insert(RequestedModel);
        }
    }
}

impl StepContext {
    /// Capture the credential-scoped owner before opening the transport. A rejection after
    /// delivery (including a status-bearing stream error) never reaches this boundary.
    pub(crate) fn alab_observe_model_open<T>(
        &self,
        session: &Session,
        send: impl Future<Output = Result<T>>,
        first: bool,
        replay: bool,
    ) -> impl Future<Output = Result<T>> {
        // The task-local observer and cancellation guard nest this transport future.
        // Pin it before constructing either future so their poll frames only carry a pointer.
        let send = Box::pin(send);
        async move {
            let turn = &self.turn;
            let manager = &session.services.models_manager;
            let owner = (first && codex_alab_core::active()
                && self.settings.model_info.origin == ModelOrigin::Provider
                && turn.config.alab_follows_saved_model()
                && turn.config.model_catalog.is_none()
                && turn.parent_thread_id.is_none()
                && !turn.session_source.is_non_root_agent()
                && !crate::guardian::is_basic_session_source(&turn.session_source)
                && !matches!(&turn.session_source, SessionSource::Custom(name) if name == "thread_sample")
                && turn.extension_data.get::<RequestedModel>().is_none()
                && (Arc::ptr_eq(&self.settings, &turn.initial_settings) || turn.extension_data.get::<RetriedModel>().is_some_and(|retry| retry.destination == self.settings.model_info.slug)))
                .then(|| {
                    let catalog = manager.refreshed_model_catalog()?;
                    manager.model_catalog_updates()?.borrow().clone().filter(|owner| owner.alab_confirms_model_recovery(owner, &catalog))
                })
                .flatten();
            let Some(owner) = owner else { return send.await };
            let result = stream::open(send, self, Arc::clone(&owner), replay).await;
            if result.as_ref().is_err_and(http_model_refusal) {
                self.extension_data.insert(RejectedModel { owner, replay });
            }
            result
        }
    }
}

fn http_model_refusal(error: &CodexErr) -> bool {
    matches!(
        error.details(),
        CodexErrorDetails::ModelNotFound {
            status: Some(_),
            ..
        }
    )
}

impl Session {
    pub(super) async fn alab_serialize_model_activation(&self) -> Option<SemaphorePermit<'_>> {
        // A live ALab process may change experience while a sparse client update resolves.
        // The upstream binary has no automatic writer. Its submission actor serializes clients.
        if !codex_alab_core::is_alab_binary() {
            return None;
        }
        Some(crate::session::thread_settings::acquire_persistence_lock(self).await)
    }

    pub(in crate::session) async fn alab_recover_retired_model(
        &self,
        step: &StepContext,
        error: &CodexErr,
        initial_sampling_step: bool,
    ) -> bool {
        if !matches!(error.details(), CodexErrorDetails::ModelNotFound { .. }) {
            return false;
        }
        let Some(rejection) = step.extension_data.remove::<RejectedModel>() else {
            return false;
        };
        let config = {
            let state = self.state.lock().await;
            let configuration = &state.session_configuration;
            let config = &configuration.original_config_do_not_use;
            if !config.alab_follows_saved_model()
                || configuration.step_settings.collaboration_mode.model()
                    != step.settings.model_info.slug
                || !Arc::ptr_eq(&configuration.provider, &step.turn.provider)
            {
                return false;
            }
            Arc::clone(config)
        };
        let manager = &self.services.models_manager;
        let Some(mut updates) = manager.model_catalog_updates() else {
            return false;
        };
        let Some(previous) = manager.refreshed_model_catalog() else {
            return false;
        };
        if !updates.borrow_and_update().as_ref().is_some_and(|current| {
            current.alab_confirms_model_recovery(&rejection.owner, &previous)
        }) {
            return false;
        }
        // The serving endpoint bounds its actual fetch. Waiting behind another
        // catalog request remains cancellable by the original turn.
        manager
            .list_models(RefreshStrategy::Online, config.http_client_factory())
            .await;
        if !matches!(updates.has_changed(), Ok(true)) {
            return false;
        }
        let Some(latest) = updates.borrow().clone() else {
            return false;
        };
        let Some(catalog) = manager.refreshed_model_catalog() else {
            return false;
        };
        if !latest.alab_confirms_model_recovery(&rejection.owner, &catalog) {
            return false;
        }
        let available = manager.build_available_models(catalog.to_vec());
        let selected = step.settings.selected_collaboration_mode();
        let Some(index) = config.alab_catalog_successor(
            &available,
            &step.settings.model_info.slug,
            selected.reasoning_effort().as_ref(),
        ) else {
            return false;
        };
        let destination = available[index].model.clone();
        if destination == step.settings.model_info.slug {
            return false;
        }
        let continuation_is_ready =
            initial_sampling_step || step.alab_has_complete_retirement_input(self).await;
        if !rejection.replay
            || !continuation_is_ready
            || step.turn.extension_data.get::<RetriedModel>().is_some()
        {
            // Prior output and model-request hooks belong to this turn. Publish the
            // verified successor before its terminal result can release the next input.
            crate::session::model_catalog_settings::apply(self).await;
            return false;
        }
        let proof = ConfirmedRetirement {
            turn: Arc::clone(&step.turn),
            settings: Arc::clone(&step.settings),
            config,
            catalog,
            destination: destination.clone(),
        };
        let applied = matches!(
            self.apply_checked_turn_settings(
                &step.turn.sub_id,
                TurnSettingsUpdate {
                    model: Some(destination),
                    ..Default::default()
                },
                Some(&proof)
            )
            .await,
            TurnSettingsUpdateOutcome::Applied
        );
        if applied {
            step.turn.extension_data.insert(RetriedModel {
                destination: proof.destination,
                catalog: proof.catalog,
                continuation: !initial_sampling_step,
            });
        }
        applied
    }
}

#[cfg(test)]
#[path = "model_retirement_tests.rs"]
mod tests;
