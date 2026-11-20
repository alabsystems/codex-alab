// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Shared catalog selection for thread startup and its client-facing default.
use std::sync::Arc;

use super::Config;
use codex_model_provider_info::AMAZON_BEDROCK_PROVIDER_ID;
use codex_model_provider_info::AMAZON_BEDROCK_RUNTIME_PROVIDER_ID;
use codex_model_provider_info::GCP_PROVIDER_ID;
use codex_model_provider_info::OPENAI_PROVIDER_ID;
use codex_models_manager::ModelOrigin;
use codex_models_manager::manager::ModelsManager;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::sol_family::declared_successor;
use codex_models_manager::sol_family::newest_eligible_routed_successor;
use codex_models_manager::sol_family::newest_eligible_sol;
use codex_models_manager::sol_family::newest_eligible_successor;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;

fn default_tui_instructions() -> Option<String> {
    codex_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets()
        .into_iter()
        .find(|preset| preset.mode == Some(codex_protocol::config_types::ModeKind::Default))?
        .developer_instructions
        .flatten()
}

impl Config {
    /// Keep an explicit catalog across provider selections. A packaged catalog
    /// belongs to its provider and must be selected again for the destination.
    pub fn alab_select_provider_catalog(&mut self, provider_id: &str) -> std::io::Result<()> {
        if self.alab_packaged_kimi_catalog || self.model_catalog.is_none() {
            self.model_catalog =
                codex_alab_kimi::model_catalog(codex_alab_core::active(), provider_id, None)?;
            self.alab_packaged_kimi_catalog = self.model_catalog.is_some();
        }
        Ok(())
    }

    /// The host chooses its source. A remote client name alone cannot select
    /// the embedded TUI's startup behavior.
    pub fn alab_is_tui_start(
        source: &codex_protocol::protocol::SessionSource,
        client: Option<&str>,
    ) -> bool {
        codex_alab_core::active()
            && matches!(source, codex_protocol::protocol::SessionSource::Cli)
            && client == Some("codex-tui")
    }

    pub(crate) fn alab_start_mode_instructions(
        &self,
        source: &codex_protocol::protocol::SessionSource,
        parent: Option<codex_protocol::ThreadId>,
        history: &codex_history::InitialHistory,
    ) -> Option<String> {
        if self.alab_fresh_tui_preset
            && codex_alab_core::active()
            && matches!(source, codex_protocol::protocol::SessionSource::Cli)
            && parent.is_none()
            && matches!(
                history,
                codex_history::InitialHistory::New | codex_history::InitialHistory::Cleared
            )
        {
            default_tui_instructions()
        } else {
            None
        }
    }

    /// The embedded thread/start response reports model and effort. Both ends
    /// use the upstream preset for its instructions.
    pub fn alab_fresh_tui_mode(
        model: &str,
        effort: Option<ReasoningEffort>,
    ) -> Option<codex_protocol::config_types::CollaborationMode> {
        codex_alab_core::active().then(|| codex_protocol::config_types::CollaborationMode {
            mode: codex_protocol::config_types::ModeKind::Default,
            settings: codex_protocol::config_types::Settings {
                model: model.to_owned(),
                reasoning_effort: effort,
                developer_instructions: default_tui_instructions(),
            },
        })
    }

    pub fn alab_uses_sol_default(&self) -> bool {
        codex_alab_core::active()
            && self.model_provider_id == OPENAI_PROVIDER_ID
            && self.model_provider.supports_codex_backend_routes()
            && self.model_provider.model_catalog_url.is_none()
            && self.model_catalog.is_none()
    }

    pub(crate) fn alab_follows_explicit_api_catalog(&self) -> bool {
        self.model_provider_id == OPENAI_PROVIDER_ID
            && self.model_catalog.is_none()
            && codex_model_provider::alab_openai_api_catalog::DefaultApi::configured(
                &self.model_provider,
            )
            .is_some()
    }

    pub fn alab_loads_startup_catalog(&self) -> bool {
        match &self.model {
            Some(_) => self.alab_follows_saved_model(),
            None => self.alab_uses_sol_default() || self.alab_follows_explicit_api_catalog(),
        }
    }

    /// Explicit per-launch selections are authoritative. A saved or global
    /// preference may advance when its serving catalog supplies a successor.
    pub fn alab_follows_saved_model(&self) -> bool {
        codex_alab_core::active()
            && (self.model.is_none()
                || self.model.as_ref() == self.alab_saved_model_can_follow.as_ref())
    }

    pub fn alab_catalog_successor(
        &self,
        models: &[ModelPreset],
        current: &str,
        effort: Option<&ReasoningEffort>,
    ) -> Option<usize> {
        if self.alab_uses_sol_default() || self.alab_follows_explicit_api_catalog() {
            return newest_eligible_successor(models, current, effort);
        }
        // These native adapters publish their own catalog of versioned OpenAI models.
        // A replacement catalog owns its migrations even when it uses a built-in provider.
        if codex_alab_core::active()
            && self.model_catalog.is_none()
            && self.model_provider.model_catalog_url.is_none()
        {
            let namespace = match self.model_provider_id.as_str() {
                GCP_PROVIDER_ID => Some(""),
                AMAZON_BEDROCK_PROVIDER_ID if current.starts_with("openai.gpt-") => Some("openai."),
                AMAZON_BEDROCK_RUNTIME_PROVIDER_ID => current
                    .split_once("gpt-")
                    .map(|(prefix, _)| prefix)
                    .filter(|prefix| prefix.ends_with(".openai.")),
                _ => None,
            };
            if let Some(namespace) = namespace {
                return newest_eligible_routed_successor(models, current, namespace, effort);
            }
        }
        declared_successor(models, current, effort)
    }

    pub fn alab_no_default_model_message(&self) -> &'static str {
        if self.alab_uses_sol_default() {
            "No released Sol model supporting the requested reasoning effort is available in this catalog. Set an explicit model with --model or check your account's model catalog."
        } else {
            "No models are available. Set `model` explicitly or check your model catalog configuration."
        }
    }

    /// Clients honor the serving backend's advertised default, including on remote
    /// connections. A missing family default must not turn the first picker row into one.
    pub fn alab_advertised_default(models: &[ModelPreset]) -> Option<&ModelPreset> {
        let advertised = models.iter().find(|model| model.is_default);
        if codex_alab_core::active() {
            advertised
        } else {
            advertised.or(models.first())
        }
    }

    /// A fresh thread needs an advertised default; resume and fork get their model
    /// from the saved thread. Hosted backends can supply their model at activation.
    pub fn alab_provisional_startup_model(
        &self,
        models: &[ModelPreset],
        fresh: bool,
        remote: bool,
    ) -> Result<bool, &'static str> {
        if fresh
            && self.model.is_none()
            && !models.is_empty()
            && Self::alab_advertised_default(models).is_none()
        {
            if remote {
                return Ok(true);
            }
            return Err(
                "The server did not advertise a default model. Set an explicit model with --model.",
            );
        }
        Ok(false)
    }

    pub fn alab_mark_family_default(&self, models: &mut [ModelPreset]) {
        if self.alab_uses_sol_default() {
            let effort = self.model_reasoning_effort.as_ref();
            let selected = match self.model.as_deref() {
                Some(current) if self.alab_follows_saved_model() => self
                    .alab_catalog_successor(models, current, effort)
                    .or_else(|| models.iter().position(|model| model.model == current)),
                Some(pinned) => models
                    .iter()
                    .position(|model| model.model == pinned && model.show_in_picker),
                None => newest_eligible_sol(models, effort),
            };
            for (index, preset) in models.iter_mut().enumerate() {
                preset.is_default = selected == Some(index);
            }
        }
    }

    pub(crate) async fn alab_models_for_successor(
        &self,
        manager: &dyn ModelsManager,
        strategy: RefreshStrategy,
    ) -> Vec<ModelPreset> {
        let catalog = manager
            .model_catalog_with_origins(strategy, self.http_client_factory())
            .await;
        // Other providers can inherit the bundled first-party catalog as a display
        // fallback. Those entries cannot declare migrations on that provider's behalf.
        let first_party = self.alab_uses_sol_default();
        manager.build_available_models(
            catalog
                .into_iter()
                .filter(|entry| {
                    first_party
                        || matches!(
                            entry.origin,
                            ModelOrigin::Provider | ModelOrigin::ExplicitConfig
                        )
                })
                .map(|entry| Arc::unwrap_or_clone(entry.model))
                .collect(),
        )
    }

    /// ThreadModelCatalog has selected this exact provider and auth owner. Reuse
    /// the region admitted on startup for the inference endpoint and SigV4.
    pub(crate) fn alab_thread_model_provider(
        &self,
        manager: &dyn ModelsManager,
    ) -> codex_model_provider_info::ModelProviderInfo {
        let mut info = self.model_provider.clone();
        if self.model_provider_id == AMAZON_BEDROCK_PROVIDER_ID
            && info.base_url.is_none()
            && let Some(region) = manager.alab_confirmed_mantle_region()
            && let Some(aws) = &mut info.aws
            && aws.region.is_none()
        {
            aws.region = Some(region);
        }
        info
    }

    pub async fn alab_resolve_catalog_model(
        &self,
        manager: &dyn ModelsManager,
        allow_provider_model_fallback: bool,
        strategy: RefreshStrategy,
    ) -> String {
        if matches!(
            strategy,
            RefreshStrategy::Online | RefreshStrategy::OnlineIfUncached
        ) {
            manager.alab_prepare_mantle_thread().await;
        }
        // The server can outlive a model release. Confirm both a new root's
        // family default and a resumed root's followed preference at startup.
        let strategy =
            if strategy == RefreshStrategy::OnlineIfUncached && self.alab_loads_startup_catalog() {
                RefreshStrategy::Online
            } else {
                strategy
            };
        if self.alab_follows_saved_model()
            && let Some(current) = self.model.as_deref()
        {
            let models = self.alab_models_for_successor(manager, strategy).await;
            if let Some(index) =
                self.alab_catalog_successor(&models, current, self.model_reasoning_effort.as_ref())
            {
                return models[index].model.clone();
            }
            // Keep the manager's existing policy for a selected model that no longer
            // exists. The read above already performed this call's requested refresh.
            return manager
                .get_default_model(
                    &self.model,
                    allow_provider_model_fallback,
                    RefreshStrategy::InMemory,
                    self.http_client_factory(),
                )
                .await;
        }
        if self.model.is_none() && self.alab_follows_explicit_api_catalog() {
            let models = self.alab_models_for_successor(manager, strategy).await;
            if !models.is_empty() || manager.refreshed_model_catalog().is_some() {
                return newest_eligible_sol(&models, self.model_reasoning_effort.as_ref())
                    .map(|index| models[index].model.clone())
                    .unwrap_or_default();
            }
        }
        if self.model.is_none() && self.alab_uses_sol_default() {
            let models = manager
                .list_models(strategy, self.http_client_factory())
                .await;
            return newest_eligible_sol(&models, self.model_reasoning_effort.as_ref())
                .map(|index| models[index].model.clone())
                .unwrap_or_default();
        }
        manager
            .get_default_model(
                &self.model,
                allow_provider_model_fallback,
                strategy,
                self.http_client_factory(),
            )
            .await
    }
}

#[cfg(test)]
#[path = "family_config_tests.rs"]
mod tests;
