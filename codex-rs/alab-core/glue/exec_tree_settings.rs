// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::environments::Requests;
use crate::CodexThreadSettingsOverrides;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::Personality;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::WindowsSandboxLevel;
use codex_protocol::models::ActivePermissionProfile;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::SandboxPolicy;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    environments: Option<Requests>,
    runtime_workspace_roots: Option<Vec<AbsolutePathBuf>>,
    profile_workspace_roots: Option<Vec<String>>,
    approval_policy: Option<AskForApproval>,
    approvals_reviewer: Option<ApprovalsReviewer>,
    sandbox_policy: Option<SandboxPolicy>,
    permission_profile: Option<PermissionProfile>,
    active_permission_profile: Option<ActivePermissionProfile>,
    windows_sandbox_level: Option<WindowsSandboxLevel>,
    model: Option<String>,
    #[serde(with = "optional_override")]
    effort: Option<Option<ReasoningEffort>>,
    summary: Option<ReasoningSummary>,
    #[serde(with = "optional_override")]
    service_tier: Option<Option<String>>,
    collaboration_mode: Option<CollaborationMode>,
    personality: Option<Personality>,
    disabled_plugin_ids: Option<Vec<String>>,
}
impl Settings {
    pub(super) fn capture(source: &CodexThreadSettingsOverrides) -> Result<Self, String> {
        let CodexThreadSettingsOverrides {
            turn_extension_init,
            environments,
            runtime_workspace_roots,
            profile_workspace_roots,
            approval_policy,
            approvals_reviewer,
            sandbox_policy,
            permission_profile,
            active_permission_profile,
            windows_sandbox_level,
            model,
            effort,
            summary,
            service_tier,
            collaboration_mode,
            personality,
            disabled_plugin_ids,
        } = source;
        if turn_extension_init.is_some() {
            return Err("host extension objects cannot enter a tree checkpoint".into());
        }
        Ok(Self {
            environments: environments.as_ref().map(Requests::capture).transpose()?,
            runtime_workspace_roots: runtime_workspace_roots.clone(),
            profile_workspace_roots: profile_workspace_roots
                .as_ref()
                .map(|roots| roots.iter().map(|r| r.as_uri().to_string()).collect()),
            approval_policy: *approval_policy,
            approvals_reviewer: *approvals_reviewer,
            sandbox_policy: sandbox_policy.clone(),
            permission_profile: permission_profile.clone(),
            active_permission_profile: active_permission_profile.clone(),
            windows_sandbox_level: *windows_sandbox_level,
            model: model.clone(),
            effort: effort.clone(),
            summary: *summary,
            service_tier: service_tier.clone(),
            collaboration_mode: collaboration_mode.clone(),
            personality: *personality,
            disabled_plugin_ids: disabled_plugin_ids.clone(),
        })
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        if let Some(environments) = &self.environments {
            environments.validate()?;
        }
        if let Some(roots) = &self.profile_workspace_roots {
            for root in roots {
                PathUri::parse(root).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    pub(super) fn restore(
        &self,
        fresh: &CodexThreadSettingsOverrides,
    ) -> Result<CodexThreadSettingsOverrides, String> {
        self.validate()?;
        let environments = self
            .environments
            .as_ref()
            .map(|saved| {
                saved.restore(
                    fresh
                        .environments
                        .as_ref()
                        .ok_or("tree settings need fresh environment bindings")?,
                )
            })
            .transpose()?;
        Ok(CodexThreadSettingsOverrides {
            // New host objects come from the new owner, never from the wire.
            turn_extension_init: fresh.turn_extension_init.clone(),
            environments,
            runtime_workspace_roots: self.runtime_workspace_roots.clone(),
            profile_workspace_roots: self
                .profile_workspace_roots
                .as_ref()
                .map(|roots| {
                    roots
                        .iter()
                        .map(|root| {
                            PathUri::parse(root)
                                .map(Into::into)
                                .map_err(|e| e.to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            approval_policy: self.approval_policy,
            approvals_reviewer: self.approvals_reviewer,
            sandbox_policy: self.sandbox_policy.clone(),
            permission_profile: self.permission_profile.clone(),
            active_permission_profile: self.active_permission_profile.clone(),
            windows_sandbox_level: self.windows_sandbox_level,
            model: self.model.clone(),
            effort: self.effort.clone(),
            summary: self.summary,
            service_tier: self.service_tier.clone(),
            collaboration_mode: self.collaboration_mode.clone(),
            personality: self.personality,
            disabled_plugin_ids: self.disabled_plugin_ids.clone(),
        })
    }
}
mod optional_override {
    use serde::Deserialize;
    use serde::Serialize;
    pub fn serialize<S: serde::Serializer, T: Serialize>(
        value: &Option<Option<T>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.iter().collect::<Vec<_>>().serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        let mut values = Vec::<Option<T>>::deserialize(deserializer)?;
        if values.len() > 1 {
            return Err(serde::de::Error::custom("multiple tree setting overrides"));
        }
        Ok(values.pop())
    }
}
