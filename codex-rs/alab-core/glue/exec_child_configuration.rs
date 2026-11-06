// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Child startup deltas over the original root invocation, reloaded under current policy.
use super::*;
use codex_features::FeatureToml;
use codex_features::FeaturesToml;
use codex_features::TokenBudgetConfigToml;
use codex_protocol::models::ActivePermissionProfile;
use codex_protocol::models::BaseInstructionsProvenance;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preferences {
    model: Option<String>,
    review_model: Option<String>,
    model_context_window: Option<i64>,
    model_auto_compact_token_limit: Option<i64>,
    reasoning: Option<ReasoningEffort>,
    summary: Option<ReasoningSummary>,
    verbosity: Option<Verbosity>,
    base: Option<String>,
    base_provenance: Option<BaseInstructionsProvenance>,
    developer: Option<String>,
    compact_prompt: Option<String>,
    personality: Option<Personality>,
    provider_id: String,
    provider: Option<ModelProviderInfo>,
    features: FeaturesToml,
    include_skill_instructions: bool,
    active_profile: Option<ActivePermissionProfile>,
    profile_roots: Vec<String>,
}

fn invalid(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
}

impl ExecConfiguration {
    /// Retain a child's resolved role and runtime choices while inheriting
    /// invocation-only inputs (such as executable paths) from this root.
    /// This encodes configuration data, never a grant to resume the child.
    pub fn capture_child(&self, child: &Config) -> std::io::Result<Self> {
        if self.version != 2 || self.home != child.codex_home {
            return Err(invalid("child configuration changed the invocation home"));
        }
        let mut config = child.clone();
        // A new session re-applies startup activation. Preserve the configured
        // token-budget preferences instead of baking model activation into them.
        config.prepare_token_budget_for_startup().map_err(invalid)?;
        let mut overrides = self.overrides.clone();
        overrides.model.clone_from(&config.model);
        overrides.review_model.clone_from(&config.review_model);
        overrides.cwd = Some(config.cwd.to_path_buf());
        overrides.approval_policy = Some(config.permissions.approval_policy.value());
        overrides.approvals_reviewer = Some(config.approvals_reviewer);
        // The resolved profile already contains the child's restrictions. Do
        // not replay a parent's additive roots or competing profile selection.
        overrides.sandbox_mode = None;
        overrides.default_permissions = None;
        overrides.persisted_permission_profile_id = None;
        overrides.persisted_permission_profile = None;
        overrides.permission_profile = Some(config.permissions.permission_profile().clone());
        overrides.additional_writable_roots.clear();
        overrides.workspace_roots = Some(config.workspace_roots.clone());
        overrides.model_provider = Some(config.model_provider_id.clone());
        overrides.service_tier = Some(config.service_tier.clone());
        overrides
            .base_instructions
            .clone_from(&config.base_instructions);
        overrides
            .developer_instructions
            .clone_from(&config.developer_instructions);
        overrides.personality = config.personality;
        overrides.compact_prompt.clone_from(&config.compact_prompt);
        overrides.show_raw_agent_reasoning = Some(config.show_raw_agent_reasoning);
        overrides.ephemeral = Some(config.ephemeral);
        let saved = Self {
            version: 2,
            home: config.codex_home.clone(),
            layers: config.config_layer_stack.capture_exec_layers()?,
            overrides,
            child: Some(Preferences::capture(&config)?),
        };
        // Run the complete wire path while the source is still active. A
        // serializer refusal must not first appear after stopping its task.
        serde_json::from_value(serde_json::to_value(saved).map_err(invalid)?).map_err(invalid)
    }
}

impl Preferences {
    fn capture(config: &Config) -> std::io::Result<Self> {
        let provider = if built_in_model_providers(None).contains_key(&config.model_provider_id) {
            None
        } else {
            let decoded: ModelProviderInfo = serde_json::from_value(
                serde_json::to_value(&config.model_provider).map_err(invalid)?,
            )
            .map_err(invalid)?;
            if decoded != config.model_provider {
                return Err(invalid(
                    "custom provider contains runtime-only settings that cannot be checkpointed",
                ));
            }
            Some(decoded)
        };
        let effective = config.config_layer_stack.effective_config();
        let mut features = effective
            .get("features")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        for spec in codex_features::FEATURES {
            let enabled = config.features.enabled(spec.id);
            match features.get_mut(spec.key) {
                Some(toml::Value::Table(table)) => {
                    table.insert("enabled".into(), toml::Value::Boolean(enabled));
                }
                _ => {
                    features.insert(spec.key.into(), toml::Value::Boolean(enabled));
                }
            }
        }
        let mut features: FeaturesToml =
            toml::Value::Table(features).try_into().map_err(invalid)?;
        if let Some(FeatureToml::Config(guardian)) = &mut features.guardianv2 {
            guardian.thread_context = Some(
                config
                    .features
                    .enabled(codex_features::Feature::GuardianThreadContext),
            );
        }
        features.token_budget = Some(match &config.token_budget {
            Some(budget) => {
                // Exhaustive destructuring detects additions to the source type.
                let TokenBudgetConfig {
                    use_history_notes_extension,
                    reminder_threshold_tokens,
                    reminder_message_template,
                    guidance_message,
                    auto_compact_fallback_prompt,
                    auto_compact_fallback_buffer_tokens,
                } = budget;
                FeatureToml::Config(TokenBudgetConfigToml {
                    enabled: Some(
                        config
                            .features
                            .enabled(codex_features::Feature::TokenBudget),
                    ),
                    use_history_notes_extension: Some(*use_history_notes_extension),
                    reminder_threshold_tokens: *reminder_threshold_tokens,
                    reminder_message_template: Some(reminder_message_template.clone()),
                    guidance_message: guidance_message.clone(),
                    auto_compact_fallback_prompt: auto_compact_fallback_prompt.clone(),
                    auto_compact_fallback_buffer_tokens: *auto_compact_fallback_buffer_tokens,
                })
            }
            None => FeatureToml::Enabled(
                config
                    .features
                    .enabled(codex_features::Feature::TokenBudget),
            ),
        });
        Ok(Self {
            model: config.model.clone(),
            review_model: config.review_model.clone(),
            model_context_window: config.model_context_window,
            model_auto_compact_token_limit: config.model_auto_compact_token_limit,
            reasoning: config.model_reasoning_effort.clone(),
            summary: config.model_reasoning_summary,
            verbosity: config.model_verbosity,
            base: config.base_instructions.clone(),
            base_provenance: config.base_instructions_provenance.clone(),
            developer: config.developer_instructions.clone(),
            compact_prompt: config.compact_prompt.clone(),
            personality: config.personality,
            provider_id: config.model_provider_id.clone(),
            provider,
            features,
            include_skill_instructions: config.include_skill_instructions,
            active_profile: config.permissions.active_permission_profile(),
            profile_roots: config
                .permissions
                .profile_workspace_roots()
                .iter()
                .map(|root| root.as_uri().to_string())
                .collect(),
        })
    }

    pub(super) fn apply_inputs(&self, config: &mut ConfigToml) {
        config.model.clone_from(&self.model);
        config.review_model.clone_from(&self.review_model);
        config.model_context_window = self.model_context_window;
        config.model_auto_compact_token_limit = self.model_auto_compact_token_limit;
        config.model_reasoning_effort.clone_from(&self.reasoning);
        config.model_reasoning_summary = self.summary;
        config.model_verbosity = self.verbosity;
        config.instructions.clone_from(&self.base);
        config.model_instructions_file = None;
        config.developer_instructions.clone_from(&self.developer);
        config.compact_prompt.clone_from(&self.compact_prompt);
        config.experimental_compact_prompt_file = None;
        config.personality = self.personality;
        // Built-ins are owned by the loader. Only custom definitions belong
        // in its override map; inserting OpenAI there is correctly refused.
        if let Some(provider) = &self.provider {
            config
                .model_providers
                .insert(self.provider_id.clone(), provider.clone());
        }
        config.features = Some(self.features.clone());
        config
            .skills
            .get_or_insert_with(Default::default)
            .include_instructions = Some(self.include_skill_instructions);
    }

    pub(super) fn restore_metadata(
        &self,
        config: &mut Config,
        source_profile: Option<&PermissionProfile>,
    ) -> std::io::Result<()> {
        config
            .base_instructions_provenance
            .clone_from(&self.base_provenance);
        if let Some(active) = &self.active_profile
            && source_profile == Some(config.permissions.permission_profile())
        {
            let roots = self
                .profile_roots
                .iter()
                .map(|root| {
                    codex_utils_path_uri::PathUri::parse(root)
                        .map(Into::into)
                        .map_err(invalid)
                })
                .collect::<std::io::Result<Vec<_>>>()?;
            // Use only the profile already validated by the current loader.
            // Restoring a display identity must never replace those permissions.
            let snapshot = PermissionProfileSnapshot::active_with_profile_workspace_roots(
                config.permissions.permission_profile().clone(),
                active.clone(),
                roots,
            );
            config
                .permissions
                .set_permission_profile_from_session_snapshot(snapshot)
                .map_err(invalid)?;
        }
        Ok(())
    }
}
