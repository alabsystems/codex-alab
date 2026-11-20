// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Apply aterm's narrow application policy through the same account catalog and
//! config manager as config/read and config/batchWrite, even with no open TUI.
use crate::config_manager::ConfigManager;
use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigLayerSource;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::WriteStatus;
use codex_arg0::Arg0DispatchPaths;
use codex_config::NoopThreadConfigLoader;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::sol_family::newest_eligible_sol;
use codex_protocol::openai_models::ReasoningEffort;

pub enum ResultState {
    Applied(String),
    Overridden,
    Unsupported,
}

pub async fn apply(
    home: &std::path::Path,
    previously_managed: bool,
    paths: Arg0DispatchPaths,
) -> anyhow::Result<ResultState> {
    let manager = ConfigManager::new(
        home.to_path_buf(),
        Vec::new(),
        Default::default(),
        true,
        Default::default(),
        paths,
        super::alab_no_external_thread_config_loader(NoopThreadConfigLoader),
    );
    let config = manager.load_managed_startup_config(None).await?.config?;
    if !config.alab_uses_sol_default() {
        return Ok(ResultState::Unsupported);
    }
    let auth = codex_login::AuthManager::shared_from_config(&config, false).await?;
    let runtime = if config
        .features
        .enabled(codex_features::Feature::RuntimeAgentInstances)
    {
        let installation = codex_core::resolve_installation_id(&config.codex_home).await?;
        codex_core::build_runtime_auth_manager(&config, std::sync::Arc::clone(&auth), &installation)
    } else {
        None
    };
    let models = codex_core::build_models_manager(&config, auth, runtime)
        .list_models(
            RefreshStrategy::OnlineIfUncached,
            config.http_client_factory(),
        )
        .await;
    let Some(index) = newest_eligible_sol(&models, Some(&ReasoningEffort::Ultra)) else {
        return Ok(ResultState::Unsupported);
    };
    let model = models[index].model.clone();
    let read = || {
        manager.read(ConfigReadParams {
            include_layers: true,
            cwd: None,
        })
    };
    let before = read().await?;
    // Leave explicit pins intact. Only an unpinned primary account follows released Sol.
    if before.config.model.is_some() {
        return Ok(ResultState::Overridden);
    }
    if before.config.model_reasoning_effort != Some(ReasoningEffort::Ultra) {
        if previously_managed {
            return Ok(ResultState::Overridden);
        }
        let user = before.layers.as_deref().and_then(|layers| {
            layers
                .iter()
                .find(|layer| matches!(layer.name, ConfigLayerSource::User { profile: None, .. }))
        });
        let Some(user) = user else {
            return Ok(ResultState::Overridden);
        };
        let write = manager
            .batch_write(ConfigBatchWriteParams {
                edits: vec![ConfigEdit {
                    key_path: "model_reasoning_effort".into(),
                    value: "ultra".into(),
                    merge_strategy: MergeStrategy::Replace,
                }],
                expected_version: Some(user.version.clone()),
                file_path: None,
                reload_user_config: false,
            })
            .await?;
        if write.status != WriteStatus::Ok {
            return Ok(ResultState::Overridden);
        }
    }
    let after = read().await?;
    Ok(
        if after.config.model.is_none()
            && after.config.model_reasoning_effort == Some(ReasoningEffort::Ultra)
        {
            ResultState::Applied(model)
        } else {
            ResultState::Overridden
        },
    )
}
