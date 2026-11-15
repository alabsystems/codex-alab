// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::MultiAgentVersion;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

fn catalog() -> ModelsResponse {
    crate::model_catalog(true, crate::GLOBAL_PROVIDER_ID, None)
        .expect("embedded catalog")
        .expect("Kimi catalog")
}

#[test]
fn catalog_covers_every_documented_model_with_current_codex_capabilities() {
    let models: BTreeMap<_, _> = catalog()
        .models
        .into_iter()
        .map(|model| (model.slug.clone(), model))
        .collect();
    let expected = [
        ("k3-256k", 262_144),
        ("k3.1-pro", 1_048_576),
        ("k3.1-pro-highspeed", 1_048_576),
        ("k3", 1_048_576),
        ("kimi-for-coding", 1_048_576),
        ("kimi-for-coding-highspeed", 262_144),
    ];
    assert_eq!(models.len(), expected.len());
    for (slug, context_window) in expected {
        let model = models.get(slug).unwrap_or_else(|| panic!("missing {slug}"));
        assert_eq!(model.context_window, Some(context_window), "{slug}");
        assert_eq!(model.max_context_window, Some(context_window), "{slug}");
        assert_eq!(
            model.input_modalities,
            [InputModality::Text, InputModality::Image],
            "{slug}"
        );
        assert_eq!(model.shell_type, ConfigShellToolType::UnifiedExec, "{slug}");
        assert_eq!(
            model.apply_patch_tool_type,
            Some(ApplyPatchToolType::Freeform),
            "{slug}"
        );
        assert_eq!(
            model.multi_agent_version,
            Some(MultiAgentVersion::V2),
            "{slug}"
        );
        assert_eq!(
            model.default_reasoning_summary,
            ReasoningSummary::None,
            "{slug}"
        );
        assert!(model.supports_reasoning_summary_parameter, "{slug}");
        assert_eq!(
            model
                .model_messages
                .as_ref()
                .and_then(|messages| messages.instructions_template.as_deref()),
            Some(codex_models_manager::model_info::BASE_INSTRUCTIONS),
            "{slug}"
        );
        assert!(!model.use_responses_lite, "{slug}");
    }

    for slug in [
        "k3-256k",
        "k3.1-pro",
        "k3.1-pro-highspeed",
        "k3",
        "kimi-for-coding",
    ] {
        let model = &models[slug];
        let efforts: Vec<_> = model
            .supported_reasoning_levels
            .iter()
            .map(|preset| preset.effort.clone())
            .collect();
        assert_eq!(
            efforts,
            [
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max
            ],
            "{slug}"
        );
        let default_effort = if slug == "kimi-for-coding" {
            ReasoningEffort::Max
        } else {
            ReasoningEffort::High
        };
        assert_eq!(
            model.default_reasoning_level,
            Some(default_effort),
            "{slug}"
        );
    }

    let highspeed = &models["kimi-for-coding-highspeed"];
    assert_eq!(highspeed.default_reasoning_level, None);
    assert!(highspeed.supported_reasoning_levels.is_empty());
}

#[test]
fn explicit_catalog_and_vanilla_behavior_take_precedence() {
    let configured = ModelsResponse::default();
    for active in [false, true] {
        for provider in [
            crate::GLOBAL_PROVIDER_ID,
            crate::CHINA_PROVIDER_ID,
            "openai",
        ] {
            assert_eq!(
                crate::model_catalog(active, provider, Some(configured.clone()))
                    .expect("configured catalog"),
                Some(configured.clone())
            );
        }
    }
    assert_eq!(
        crate::model_catalog(false, crate::GLOBAL_PROVIDER_ID, None).expect("vanilla"),
        None
    );
    assert_eq!(
        crate::model_catalog(true, "openai", None).expect("other provider"),
        None
    );
    assert_eq!(
        crate::model_catalog(true, crate::CHINA_PROVIDER_ID, None)
            .expect("China catalog")
            .map(|catalog| catalog.models.len()),
        Some(6)
    );
}
