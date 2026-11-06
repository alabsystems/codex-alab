// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Capture configuration inputs, never persisted runtime credentials or authority.
use super::*;

#[path = "exec_child_configuration.rs"]
mod child;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecConfiguration {
    version: u32,
    home: AbsolutePathBuf,
    layers: serde_json::Value,
    overrides: SavedOverrides,
    child: Option<child::Preferences>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedOverrides {
    model: Option<String>,
    review_model: Option<String>,
    cwd: Option<PathBuf>,
    approval_policy: Option<AskForApproval>,
    approvals_reviewer: Option<ApprovalsReviewer>,
    sandbox_mode: Option<SandboxMode>,
    permission_profile: Option<PermissionProfile>,
    default_permissions: Option<String>,
    persisted_permission_profile_id: Option<String>,
    persisted_permission_profile: Option<PermissionProfile>,
    model_provider: Option<String>,
    #[serde(with = "service_tier")]
    service_tier: Option<Option<String>>,
    codex_self_exe: Option<PathBuf>,
    codex_linux_sandbox_exe: Option<PathBuf>,
    main_execve_wrapper_exe: Option<PathBuf>,
    base_instructions: Option<String>,
    developer_instructions: Option<String>,
    personality: Option<Personality>,
    compact_prompt: Option<String>,
    show_raw_agent_reasoning: Option<bool>,
    tools_web_search_request: Option<bool>,
    ephemeral: Option<bool>,
    bypass_hook_trust: Option<bool>,
    additional_writable_roots: Vec<PathBuf>,
    workspace_roots: Option<Vec<AbsolutePathBuf>>,
}
impl From<ConfigOverrides> for SavedOverrides {
    fn from(value: ConfigOverrides) -> Self {
        let ConfigOverrides {
            model,
            review_model,
            cwd,
            approval_policy,
            approvals_reviewer,
            sandbox_mode,
            permission_profile,
            default_permissions,
            persisted_permission_profile_id,
            persisted_permission_profile,
            model_provider,
            service_tier,
            codex_self_exe,
            codex_linux_sandbox_exe,
            main_execve_wrapper_exe,
            base_instructions,
            developer_instructions,
            personality,
            compact_prompt,
            show_raw_agent_reasoning,
            tools_web_search_request,
            ephemeral,
            bypass_hook_trust,
            additional_writable_roots,
            workspace_roots,
        } = value;
        Self {
            model,
            review_model,
            cwd,
            approval_policy,
            approvals_reviewer,
            sandbox_mode,
            permission_profile,
            default_permissions,
            persisted_permission_profile_id,
            persisted_permission_profile,
            model_provider,
            service_tier,
            codex_self_exe,
            codex_linux_sandbox_exe,
            main_execve_wrapper_exe,
            base_instructions,
            developer_instructions,
            personality,
            compact_prompt,
            show_raw_agent_reasoning,
            tools_web_search_request,
            ephemeral,
            bypass_hook_trust,
            additional_writable_roots,
            workspace_roots,
        }
    }
}
impl From<SavedOverrides> for ConfigOverrides {
    fn from(value: SavedOverrides) -> Self {
        let SavedOverrides {
            model,
            review_model,
            cwd,
            approval_policy,
            approvals_reviewer,
            sandbox_mode,
            permission_profile,
            default_permissions,
            persisted_permission_profile_id,
            persisted_permission_profile,
            model_provider,
            service_tier,
            codex_self_exe,
            codex_linux_sandbox_exe,
            main_execve_wrapper_exe,
            base_instructions,
            developer_instructions,
            personality,
            compact_prompt,
            show_raw_agent_reasoning,
            tools_web_search_request,
            ephemeral,
            bypass_hook_trust,
            additional_writable_roots,
            workspace_roots,
        } = value;
        Self {
            model,
            review_model,
            cwd,
            approval_policy,
            approvals_reviewer,
            sandbox_mode,
            permission_profile,
            default_permissions,
            persisted_permission_profile_id,
            persisted_permission_profile,
            model_provider,
            service_tier,
            codex_self_exe,
            codex_linux_sandbox_exe,
            main_execve_wrapper_exe,
            base_instructions,
            developer_instructions,
            personality,
            compact_prompt,
            show_raw_agent_reasoning,
            tools_web_search_request,
            ephemeral,
            bypass_hook_trust,
            additional_writable_roots,
            workspace_roots,
        }
    }
}

impl ExecConfiguration {
    pub fn capture(config: &Config, overrides: &ConfigOverrides) -> std::io::Result<Self> {
        Ok(Self {
            version: 2,
            home: config.codex_home.clone(),
            layers: config.config_layer_stack.capture_exec_layers()?,
            overrides: overrides.clone().into(),
            child: None,
        })
    }

    pub async fn restore(&self, current: &Config) -> std::io::Result<Config> {
        if self.version != 2 || self.home != current.codex_home {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "configuration snapshot version or home mismatch",
            ));
        }
        let layers = ConfigLayerStack::restore_exec_layers(
            self.layers.clone(),
            &current.config_layer_stack,
        )?;
        let mut cfg = config_toml_from_layers(&layers).await?;
        if let Some(child) = &self.child {
            child.apply_inputs(&mut cfg);
        }
        let mut restored = Config::load_config_with_layer_stack(
            LOCAL_FS.as_ref(),
            cfg,
            self.overrides.clone().into(),
            current.codex_home.clone(),
            layers,
        )
        .await?;
        if let Some(child) = &self.child {
            child.restore_metadata(&mut restored, self.overrides.permission_profile.as_ref())?;
        }
        restored.application_network_policy = current.application_network_policy.clone();
        restored.application_auth_route_config = current.application_auth_route_config.clone();
        Ok(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fixture(
        home: &AbsolutePathBuf,
        model: &str,
        overrides: ConfigOverrides,
        restrict_approval: bool,
    ) -> std::io::Result<Config> {
        let requirements = if restrict_approval {
            codex_config::ConfigRequirements {
                approval_policy: ConstrainedWithSource::new(
                    Constrained::allow_only(AskForApproval::Never),
                    Some(codex_config::RequirementSource::Unknown),
                ),
                ..Default::default()
            }
        } else {
            Default::default()
        };
        let layers = ConfigLayerStack::new(
            vec![codex_config::ConfigLayerEntry::new(
                codex_config::ConfigLayerSource::User {
                    file: AbsolutePathBuf::from_absolute_path(home.join("config.toml"))?,
                    profile: None,
                },
                toml::Value::try_from(ConfigToml {
                    model: Some(model.into()),
                    ..Default::default()
                })
                .unwrap(),
            )],
            requirements,
            Default::default(),
        )?;
        let cfg = config_toml_from_layers(&layers).await?;
        Config::load_config_with_layer_stack(
            LOCAL_FS.as_ref(),
            cfg,
            overrides,
            home.clone(),
            layers,
        )
        .await
    }

    #[tokio::test]
    async fn restores_original_layers_and_overrides_under_current_requirements()
    -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let home = AbsolutePathBuf::from_absolute_path(directory.path())?;
        let overrides = ConfigOverrides {
            cwd: Some(directory.path().to_owned()),
            developer_instructions: Some("retain the original instructions".into()),
            ephemeral: Some(true),
            ..Default::default()
        };
        let original = fixture(&home, "original-model", overrides.clone(), false).await?;
        let snapshot = ExecConfiguration::capture(&original, &overrides)?;
        let encoded = serde_json::to_vec(&snapshot)?;
        let snapshot: ExecConfiguration = serde_json::from_slice(&encoded)?;
        let current = fixture(
            &home,
            "changed-model",
            ConfigOverrides {
                cwd: Some(directory.path().to_owned()),
                ..Default::default()
            },
            true,
        )
        .await?;
        let restored = snapshot.restore(&current).await?;
        assert_eq!(restored.model.as_deref(), Some("original-model"));
        assert_eq!(
            restored.developer_instructions,
            original.developer_instructions
        );
        assert!(restored.ephemeral);
        assert_eq!(
            restored.permissions.approval_policy.value(),
            AskForApproval::Never
        );
        assert!(
            restored
                .permissions
                .approval_policy
                .can_set(&AskForApproval::OnRequest)
                .is_err()
        );
        let mut wrong_home = current.clone();
        wrong_home.codex_home =
            AbsolutePathBuf::from_absolute_path(directory.path().join("other"))?;
        assert!(snapshot.restore(&wrong_home).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn child_runtime_settings_round_trip_without_replaying_parent_overrides()
    -> std::io::Result<()> {
        use codex_features::Feature;
        use codex_protocol::models::ActivePermissionProfile;
        use codex_protocol::models::BaseInstructionsProvenance;
        use codex_protocol::openai_models::ReasoningEffort;
        let directory = tempfile::tempdir()?;
        let home = AbsolutePathBuf::from_absolute_path(directory.path())?;
        let overrides = ConfigOverrides {
            model: Some("parent-model".into()),
            cwd: Some(directory.path().to_owned()),
            developer_instructions: Some("parent-only instructions".into()),
            base_instructions: Some("parent base".into()),
            compact_prompt: Some("parent compaction".into()),
            service_tier: Some(Some("fast".into())),
            additional_writable_roots: vec![directory.path().join("parent-output")],
            codex_linux_sandbox_exe: Some(directory.path().join("original-sandbox")),
            ..Default::default()
        };
        let original = fixture(&home, "layer-model", overrides.clone(), false).await?;
        let invocation = ExecConfiguration::capture(&original, &overrides)?;
        let mut child = original.clone();
        child.model = Some("child-model".into());
        child.model_provider_id = "child-provider".into();
        child.model_provider.name = "child provider".into();
        child.model_provider.include_internal_metadata = false;
        child.model_provider.base_url = Some("https://child-provider.invalid/v1".into());
        child
            .permissions
            .approval_policy
            .set(AskForApproval::Never)
            .map_err(std::io::Error::other)?;
        child.developer_instructions = None;
        child.base_instructions = Some("child model instructions".into());
        child.base_instructions_provenance = Some(BaseInstructionsProvenance::Model {
            model: "child-model".into(),
        });
        child.compact_prompt = None;
        child.service_tier = None;
        child.cwd = home.join("child");
        child.workspace_roots = vec![child.cwd.clone()];
        child
            .permissions
            .set_workspace_roots(child.workspace_roots.clone());
        child.model_context_window = Some(54321);
        child.model_auto_compact_token_limit = Some(43210);
        child.model_reasoning_effort = Some(ReasoningEffort::High);
        child.include_skill_instructions = false;
        child
            .features
            .disable(Feature::ShellTool)
            .map_err(std::io::Error::other)?;
        child
            .features
            .enable(Feature::TokenBudget)
            .map_err(std::io::Error::other)?;
        child.token_budget = Some(TokenBudgetConfig {
            use_history_notes_extension: false,
            reminder_threshold_tokens: Some(1234),
            reminder_message_template: "remaining: {n_remaining}".into(),
            guidance_message: Some("keep working".into()),
            auto_compact_fallback_prompt: Some("save the child state".into()),
            auto_compact_fallback_buffer_tokens: Some(100),
        });
        child
            .permissions
            .set_permission_profile_from_session_snapshot(PermissionProfileSnapshot::active(
                PermissionProfile::read_only(),
                ActivePermissionProfile::read_only(),
            ))
            .map_err(std::io::Error::other)?;
        let saved = invocation.capture_child(&child)?;
        let saved: ExecConfiguration = serde_json::from_slice(&serde_json::to_vec(&saved)?)?;
        let current = fixture(
            &home,
            "new-default",
            ConfigOverrides {
                cwd: Some(directory.path().to_owned()),
                ..Default::default()
            },
            true,
        )
        .await?;
        let restored = saved.restore(&current).await?;
        assert_eq!(restored.model, child.model);
        assert_eq!(restored.model_provider_id, child.model_provider_id);
        assert_eq!(restored.model_provider, child.model_provider);
        assert_eq!(restored.cwd, child.cwd);
        assert_eq!(restored.workspace_roots, child.workspace_roots);
        assert_eq!(restored.base_instructions, child.base_instructions);
        assert_eq!(
            restored.base_instructions_provenance,
            child.base_instructions_provenance
        );
        assert_eq!(restored.developer_instructions, None);
        assert_eq!(restored.compact_prompt, None);
        // Core represents an explicit clear canonically as "default", so
        // the parent's fast tier cannot be inherited again at startup.
        assert_eq!(
            restored.service_tier.as_deref(),
            Some(codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE)
        );
        assert_eq!(restored.model_context_window, child.model_context_window);
        assert_eq!(
            restored.model_auto_compact_token_limit,
            child.model_auto_compact_token_limit
        );
        assert_eq!(
            restored.model_reasoning_effort,
            child.model_reasoning_effort
        );
        assert_eq!(restored.token_budget, child.token_budget);
        assert!(!restored.features.enabled(Feature::ShellTool));
        assert!(!restored.include_skill_instructions);
        assert_eq!(
            restored.permissions.permission_profile(),
            &PermissionProfile::read_only()
        );
        assert_eq!(
            restored.permissions.active_permission_profile(),
            child.permissions.active_permission_profile()
        );
        assert_eq!(
            restored.permissions.approval_policy.value(),
            AskForApproval::Never
        );
        assert_eq!(
            restored.codex_linux_sandbox_exe,
            original.codex_linux_sandbox_exe
        );
        assert_eq!(
            restored.application_network_policy,
            current.application_network_policy
        );
        assert_eq!(original.model.as_deref(), Some("parent-model"));
        Ok(())
    }

    #[tokio::test]
    async fn child_explicit_clears_override_saved_layers_and_prompt_files() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let home = AbsolutePathBuf::from_absolute_path(directory.path())?;
        let overrides = ConfigOverrides {
            cwd: Some(directory.path().to_owned()),
            ..Default::default()
        };
        let mut child = fixture(&home, "original", overrides.clone(), false).await?;
        let invocation = ExecConfiguration::capture(&child, &overrides)?;
        // A captured resolved child must not re-read this now-missing prompt.
        let mut layers = child
            .config_layer_stack
            .all_layers_low_to_high()
            .cloned()
            .collect::<Vec<_>>();
        layers.push(codex_config::ConfigLayerEntry::new(
            codex_config::ConfigLayerSource::SessionFlags,
            toml::toml! { model_instructions_file = "/nonexistent/exec-child-prompt"
            developer_instructions = "stale layer instructions"
            model_context_window = 22222 }
            .into(),
        ));
        child.config_layer_stack =
            ConfigLayerStack::new(layers, Default::default(), Default::default())?;
        child.base_instructions = None;
        child.base_instructions_provenance = None;
        child.developer_instructions = None;
        child.model_context_window = None;
        let restored = invocation.capture_child(&child)?.restore(&child).await?;
        assert_eq!(restored.base_instructions, None);
        assert_eq!(restored.developer_instructions, None);
        assert_eq!(restored.model_context_window, None);
        Ok(())
    }

    #[tokio::test]
    async fn child_preferences_cannot_override_current_managed_features() -> std::io::Result<()> {
        use codex_features::Feature;
        let directory = tempfile::tempdir()?;
        let home = AbsolutePathBuf::from_absolute_path(directory.path())?;
        let overrides = ConfigOverrides {
            cwd: Some(directory.path().to_owned()),
            ..Default::default()
        };
        let mut child = fixture(&home, "child", overrides.clone(), false).await?;
        child
            .features
            .enable(Feature::ShellTool)
            .map_err(std::io::Error::other)?;
        let saved = ExecConfiguration::capture(&child, &overrides)?.capture_child(&child)?;
        let mut current = child.clone();
        let mut requirements = current.config_layer_stack.requirements().clone();
        requirements.feature_requirements = Some(codex_config::Sourced {
            value: codex_config::FeatureRequirementsToml {
                entries: std::collections::BTreeMap::from([("shell_tool".into(), false)]),
            },
            source: codex_config::RequirementSource::Unknown,
        });
        current.config_layer_stack = ConfigLayerStack::new(
            current
                .config_layer_stack
                .all_layers_low_to_high()
                .cloned()
                .collect(),
            requirements,
            Default::default(),
        )?;
        current.application_network_policy =
            codex_http_client::NetworkPolicyController::default().policy();
        let restored = saved.restore(&current).await?;
        assert!(!restored.features.enabled(Feature::ShellTool));
        assert!(child.features.enabled(Feature::ShellTool));
        assert_eq!(
            restored.application_network_policy,
            current.application_network_policy
        );
        assert_ne!(
            restored.application_network_policy,
            child.application_network_policy
        );
        Ok(())
    }

    #[tokio::test]
    async fn child_restore_applies_current_approval_policy() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let home = AbsolutePathBuf::from_absolute_path(directory.path())?;
        let overrides = ConfigOverrides {
            cwd: Some(directory.path().to_owned()),
            approval_policy: Some(AskForApproval::OnRequest),
            ..Default::default()
        };
        let child = fixture(&home, "child", overrides.clone(), false).await?;
        let saved = ExecConfiguration::capture(&child, &overrides)?.capture_child(&child)?;
        let current = fixture(
            &home,
            "current",
            ConfigOverrides {
                cwd: Some(directory.path().to_owned()),
                ..Default::default()
            },
            true,
        )
        .await?;
        let restored = saved.restore(&current).await?;
        assert_eq!(
            restored.permissions.approval_policy.value(),
            AskForApproval::Never
        );
        assert!(
            restored
                .permissions
                .approval_policy
                .can_set(&AskForApproval::OnRequest)
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn child_capture_refuses_nonportable_custom_provider() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let home = AbsolutePathBuf::from_absolute_path(directory.path())?;
        let overrides = ConfigOverrides {
            cwd: Some(directory.path().to_owned()),
            ..Default::default()
        };
        let mut child = fixture(&home, "child", overrides.clone(), false).await?;
        let invocation = ExecConfiguration::capture(&child, &overrides)?;
        child.model_provider_id = "custom-with-runtime-only-metadata".into();
        child.model_provider.include_internal_metadata = true;
        assert!(invocation.capture_child(&child).is_err());
        Ok(())
    }

    #[test]
    fn override_snapshot_preserves_explicit_empty_tier_and_runtime_inputs() {
        let original = ConfigOverrides {
            model: Some("original-model".into()),
            service_tier: Some(None),
            cwd: Some(PathBuf::from("/original/workspace")),
            developer_instructions: Some("original instructions".into()),
            additional_writable_roots: vec![PathBuf::from("/original/output")],
            ephemeral: Some(true),
            ..Default::default()
        };
        let saved = SavedOverrides::from(original);
        let encoded = serde_json::to_vec(&saved).unwrap();
        let decoded: SavedOverrides = serde_json::from_slice(&encoded).unwrap();
        let restored: ConfigOverrides = decoded.into();
        assert_eq!(restored.model.as_deref(), Some("original-model"));
        assert_eq!(restored.service_tier, Some(None));
        assert_eq!(restored.cwd, Some(PathBuf::from("/original/workspace")));
        assert_eq!(
            restored.developer_instructions.as_deref(),
            Some("original instructions")
        );
        assert_eq!(
            restored.additional_writable_roots,
            vec![PathBuf::from("/original/output")]
        );
        assert_eq!(restored.ephemeral, Some(true));
    }
}

// JSON null alone cannot distinguish an absent override from an explicit clear.
mod service_tier {
    use serde::Deserialize;
    use serde::Serialize;
    pub fn serialize<S: serde::Serializer>(
        value: &Option<Option<String>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.iter().collect::<Vec<_>>().serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Option<String>>, D::Error> {
        let mut values = Vec::<Option<String>>::deserialize(deserializer)?;
        if values.len() > 1 {
            return Err(serde::de::Error::custom("multiple service tier overrides"));
        }
        Ok(values.pop())
    }
}
