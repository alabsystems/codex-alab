// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_models_manager::ModelOrigin;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelInfoUpgrade;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;

use crate::alab_test_process as process;

#[tokio::test]
async fn provider_switches_replace_packaged_catalogs_and_preserve_explicit_catalogs() {
    if !process::child(concat!(
        module_path!(),
        "::provider_switches_replace_packaged_catalogs_and_preserve_explicit_catalogs"
    )) {
        return;
    }
    codex_alab_core::activate();
    let mut config = crate::config::test_config().await;
    for provider in [
        codex_alab_kimi::GLOBAL_PROVIDER_ID,
        codex_alab_kimi::CHINA_PROVIDER_ID,
    ] {
        config.alab_select_provider_catalog(provider).unwrap();
        assert!(
            config
                .model_catalog
                .as_ref()
                .unwrap()
                .models
                .iter()
                .any(|m| m.slug == "k3-256k")
        );
        config
            .alab_select_provider_catalog(OPENAI_PROVIDER_ID)
            .unwrap();
        assert!(config.model_catalog.is_none());
    }
    let explicit = codex_models_manager::bundled_models_response().unwrap();
    config.model_catalog = Some(explicit.clone());
    for provider in [codex_alab_kimi::GLOBAL_PROVIDER_ID, OPENAI_PROVIDER_ID] {
        config.alab_select_provider_catalog(provider).unwrap();
        assert_eq!(config.model_catalog.as_ref(), Some(&explicit));
    }
}

#[tokio::test]
async fn built_in_cloud_successors_keep_routing_and_respect_replacement_catalogs() {
    if !process::child(concat!(
        module_path!(),
        "::built_in_cloud_successors_keep_routing_and_respect_replacement_catalogs"
    )) {
        return;
    }
    codex_alab_core::activate();
    let mut config = crate::config::test_config().await;
    let providers = codex_model_provider_info::built_in_model_providers(None);
    let template = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .find(|row| {
            row.slug.ends_with("-sol")
                && row.visibility == codex_protocol::openai_models::ModelVisibility::List
                && row
                    .supported_reasoning_levels
                    .iter()
                    .any(|level| level.effort == ReasoningEffort::High)
        })
        .unwrap();
    let rows: Vec<ModelPreset> = [
        "gpt-6.2-sol",
        "openai.gpt-6.2-sol",
        "global.openai.gpt-8-sol",
        "us.openai.gpt-6.2-sol",
        "us.openai.gpt-9-astra",
        "us.openai.gpt-10-sol-preview",
    ]
    .into_iter()
    .map(|slug| {
        let mut row = template.clone();
        row.slug = slug.to_owned();
        ModelPreset::from(row)
    })
    .collect();
    for (id, current, expected) in [
        (GCP_PROVIDER_ID, "gpt-6.1-sol", "gpt-6.2-sol"),
        (
            AMAZON_BEDROCK_PROVIDER_ID,
            "openai.gpt-6.1-sol",
            "openai.gpt-6.2-sol",
        ),
        (
            AMAZON_BEDROCK_RUNTIME_PROVIDER_ID,
            "us.openai.gpt-6.1-sol",
            "us.openai.gpt-6.2-sol",
        ),
    ] {
        config.model_provider_id = id.to_owned();
        config.model_provider = providers[id].clone();
        let select = |config: &Config| {
            config
                .alab_catalog_successor(&rows, current, Some(&ReasoningEffort::High))
                .map(|index| rows[index].model.as_str())
        };
        assert_eq!(select(&config), Some(expected));
        config.model_catalog = Some(ModelsResponse {
            models: vec![template.clone()],
        });
        assert_eq!(select(&config), None);
        config.model_catalog = None;
        config.model_provider_id = "custom-cloud".into();
        assert_eq!(select(&config), None);
    }
    let mut previous = template.clone();
    previous.slug = "previous-deployment".into();
    previous.upgrade = Some(ModelInfoUpgrade {
        model: "successor-deployment".into(),
        migration_markdown: String::new(),
        retirement_at: None,
    });
    let mut successor = template;
    successor.slug = "successor-deployment".into();
    successor.upgrade = None;
    let aliases = [ModelPreset::from(previous), ModelPreset::from(successor)];
    config.model_provider_id = AMAZON_BEDROCK_PROVIDER_ID.to_owned();
    config.model_provider = providers[AMAZON_BEDROCK_PROVIDER_ID].clone();
    assert_eq!(
        config.alab_catalog_successor(&aliases, "previous-deployment", None),
        Some(1)
    );
}

#[tokio::test]
async fn family_startup_uses_catalog_and_preserves_pins_and_server_authority() {
    if !process::child(concat!(
        module_path!(),
        "::family_startup_uses_catalog_and_preserves_pins_and_server_authority"
    )) {
        return;
    }
    let mut config = crate::config::test_config().await;
    config.model = None;
    config.model_reasoning_effort = Some(ReasoningEffort::Ultra);
    let template = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .find(|m| m.slug.ends_with("-sol"))
        .expect("a bundled Sol SKU");
    let rows: Vec<ModelInfo> = ["gpt-9-astra", "gpt-6.9-sol", "gpt-6.10-sol"]
        .iter()
        .enumerate()
        .map(|(position, slug)| {
            let mut row = template.clone();
            row.slug = (*slug).to_owned();
            row.priority = position as i32;
            row.supported_in_api = true;
            row
        })
        .collect();
    let catalog = ModelsResponse {
        models: rows.clone(),
    };
    let manager = StaticModelsManager::new(None, catalog.clone(), ModelOrigin::ExplicitConfig);
    let resolve = async |config: &Config| {
        config
            .alab_resolve_catalog_model(&manager, false, RefreshStrategy::Offline)
            .await
    };
    assert_eq!(resolve(&config).await, "gpt-9-astra");

    codex_alab_core::activate();
    let source = codex_protocol::protocol::SessionSource::Cli;
    let fresh = codex_history::InitialHistory::New;
    assert_eq!(
        config.alab_start_mode_instructions(&source, None, &fresh),
        None
    );
    config.alab_fresh_tui_preset = Config::alab_is_tui_start(&source, Some("codex-tui"));
    let frontend = Config::alab_fresh_tui_mode("gpt-6.1-sol", Some(ReasoningEffort::Ultra))
        .expect("enhanced TUI preset");
    assert!(frontend.settings.developer_instructions.is_some());
    assert_eq!(
        config.alab_start_mode_instructions(&source, None, &fresh),
        frontend.settings.developer_instructions
    );
    assert_eq!(
        config.alab_start_mode_instructions(&source, Some(codex_protocol::ThreadId::new()), &fresh),
        None
    );

    assert_eq!(resolve(&config).await, "gpt-6.10-sol");
    let mut presets = manager.build_available_models(rows);
    // A connected remote server can own a legitimate default in any family.
    assert_eq!(
        Config::alab_advertised_default(&presets).unwrap().model,
        "gpt-9-astra"
    );
    config.alab_mark_family_default(&mut presets);
    assert_eq!(
        Config::alab_advertised_default(&presets).unwrap().model,
        "gpt-6.10-sol"
    );
    let mut off_family = vec![presets[0].clone()];
    config.alab_mark_family_default(&mut off_family);
    assert!(Config::alab_advertised_default(&off_family).is_none());
    assert!(
        config
            .alab_provisional_startup_model(&off_family, true, false)
            .is_err()
    );
    assert!(
        config
            .alab_provisional_startup_model(&off_family, false, false)
            .is_ok()
    );
    assert!(
        config
            .alab_provisional_startup_model(&[], true, true)
            .is_ok()
    );

    assert_eq!(
        config.alab_provisional_startup_model(&off_family, true, true),
        Ok(true)
    );
    assert_eq!(
        config.alab_provisional_startup_model(&presets, true, true),
        Ok(false)
    );

    config.model = Some("gpt-6.9-sol".into());
    assert_eq!(resolve(&config).await, "gpt-6.9-sol");
    config.alab_saved_model_can_follow = config.model.clone();
    assert_eq!(resolve(&config).await, "gpt-6.10-sol");
    config.alab_saved_model_can_follow = None;
    config.model = Some("gpt-9-astra".into());
    assert!(
        config
            .alab_provisional_startup_model(&off_family, true, false)
            .is_ok()
    );
    assert_eq!(resolve(&config).await, "gpt-9-astra");
    let mut pinned_catalog = presets.clone();
    config.alab_mark_family_default(&mut pinned_catalog);
    let pinned = pinned_catalog
        .iter()
        .position(|row| row.is_default)
        .expect("visible pinned model");
    assert_eq!(pinned_catalog[pinned].model, resolve(&config).await);
    pinned_catalog[pinned].show_in_picker = false;
    config.alab_mark_family_default(&mut pinned_catalog);
    assert!(Config::alab_advertised_default(&pinned_catalog).is_none());
    pinned_catalog.remove(pinned);
    config.alab_mark_family_default(&mut pinned_catalog);
    assert!(Config::alab_advertised_default(&pinned_catalog).is_none());

    config.model = None;
    config.model_catalog = Some(catalog);
    assert_eq!(resolve(&config).await, "gpt-9-astra");
    config.model_catalog = None;
    config.model_provider_id = "other".into();
    assert!(Config::alab_advertised_default(&off_family).is_none());
    assert_eq!(resolve(&config).await, "gpt-9-astra");

    // A custom provider only authorizes a migration using its own metadata.
    config.model = Some("deployment-blue".into());
    config.alab_saved_model_can_follow = config.model.clone();
    let mut source = template.clone();
    source.slug = "deployment-blue".into();
    source.supported_in_api = true;
    source.upgrade = Some(ModelInfoUpgrade {
        model: "deployment-green".into(),
        migration_markdown: String::new(),
        retirement_at: None,
    });
    let mut target = source.clone();
    target.slug = "deployment-green".into();
    target.upgrade = None;
    let provider_catalog = ModelsResponse {
        models: vec![source, target],
    };
    for (origin, expected) in [
        (ModelOrigin::Bundled, "deployment-blue"),
        (ModelOrigin::Provider, "deployment-green"),
        (ModelOrigin::ExplicitConfig, "deployment-green"),
    ] {
        let manager = StaticModelsManager::new(None, provider_catalog.clone(), origin);
        let offline = config
            .alab_models_for_successor(&manager, RefreshStrategy::Offline)
            .await;
        let successor = config.alab_catalog_successor(
            &offline,
            "deployment-blue",
            Some(&ReasoningEffort::Ultra),
        );
        assert_eq!(
            successor.map(|index| offline[index].model.as_str()),
            (expected == "deployment-green").then_some("deployment-green")
        );
        assert_eq!(
            config
                .alab_resolve_catalog_model(&manager, false, RefreshStrategy::Offline)
                .await,
            expected
        );
    }
}

#[path = "v2_agent_catalog_tests.rs"]
mod v2_agent_catalog_tests;
