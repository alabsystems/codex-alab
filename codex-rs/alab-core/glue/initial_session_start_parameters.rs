// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original thread/start parameter preparation shared with prospective CREATE
//! validation. This function never calls Core start or persists project trust.
//! Its ConfigManager/ThreadManager are the actual retained processor resources.
use super::*;
#[cfg(unix)]
#[path = "initial_session_fallback.rs"]
pub(super) mod fallback;

pub(crate) struct StartParameters {
    pub(super) config: Option<HashMap<String, serde_json::Value>>,
    pub(super) typesafe_overrides: ConfigOverrides,
    pub(super) dynamic_tools: Option<Vec<DynamicToolSpec>>,
    pub(super) selected_capability_roots: Option<Vec<SelectedCapabilityRoot>>,
    pub(super) experimental_raw_events: bool,
    pub(super) history_mode: Option<codex_app_server_protocol::ThreadHistoryMode>,
    pub(super) session_start_source: Option<codex_app_server_protocol::ThreadStartSource>,
    pub(super) thread_source: Option<codex_app_server_protocol::ThreadSource>,
    pub(super) project_id: Option<String>,
    pub(super) daybreak_enabled: Option<bool>,
    pub(super) environment_requests: Option<Vec<TurnEnvironmentRequest>>,
    pub(super) service_name: Option<String>,
    pub(super) allow_provider_model_fallback: bool,
}
/// Actual Core preparation result; never a historical start or SDK witness.
#[cfg(unix)]
pub(crate) struct ValidatedProspectiveStart {
    pub(crate) requested: ThreadStartParams,
    pub(crate) _effective: ThreadStartParams,
    pub(crate) config: Config,
    pub(crate) _prepared: StartParameters,
}
impl ThreadRequestProcessor {
    pub(super) async fn alab_prepare_start_parameters(
        &self,
        params: ThreadStartParams,
    ) -> Result<StartParameters, JSONRPCErrorError> {
        let ThreadStartParams {
            model,
            model_provider,
            allow_provider_model_fallback,
            service_tier,
            cwd,
            runtime_workspace_roots,
            approval_policy,
            approvals_reviewer,
            sandbox,
            permissions,
            config,
            service_name,
            base_instructions,
            developer_instructions,
            dynamic_tools,
            selected_capability_roots,
            mock_experimental_field: _mock_experimental_field,
            experimental_raw_events,
            personality,
            multi_agent_mode: _multi_agent_mode,
            ephemeral,
            history_mode,
            session_start_source,
            thread_source,
            project_id,
            daybreak_enabled,
            environments,
        } = params;
        if matches!(
            history_mode,
            Some(codex_app_server_protocol::ThreadHistoryMode::Paginated)
        ) && !self.thread_store.supports_paginated_history_lists()
        {
            return Err(invalid_request(
                "paginated threads require thread/turns/list and thread/items/list support",
            ));
        }
        if sandbox.is_some() && permissions.is_some() {
            return Err(invalid_request(
                "`permissions` cannot be combined with `sandbox`",
            ));
        }
        if let Some(project_id) = project_id.as_ref() {
            if project_id.is_empty() {
                return Err(invalid_request("projectId must not be empty"));
            }
            let project = self
                .thread_store
                .read_project(project_id.clone())
                .await
                .map_err(|error| match error {
                    ThreadStoreError::Unsupported { operation } => {
                        unsupported_thread_store_operation(operation)
                    }
                    error => internal_error(format!("failed to read project: {error}")),
                })?;
            if project.is_none() {
                return Err(invalid_request(format!("project not found: {project_id}")));
            }
        }
        let runtime_workspace_roots = runtime_workspace_roots.map(resolve_runtime_workspace_roots);
        let environment_requests =
            resolve_turn_environment_requests(self.thread_manager.as_ref(), environments)?;
        let mut typesafe_overrides = self.build_thread_config_overrides(
            model,
            model_provider,
            service_tier,
            cwd,
            runtime_workspace_roots,
            approval_policy,
            approvals_reviewer,
            sandbox,
            permissions,
            base_instructions,
            developer_instructions,
            personality,
        );
        typesafe_overrides.ephemeral = ephemeral;
        Ok(StartParameters {
            config,
            typesafe_overrides,
            dynamic_tools,
            selected_capability_roots,
            experimental_raw_events,
            history_mode,
            session_start_source,
            thread_source,
            project_id,
            daybreak_enabled,
            environment_requests,
            service_name,
            allow_provider_model_fallback,
        })
    }

    #[cfg(unix)]
    pub(crate) async fn alab_validate_prospective_start(
        &self,
        requested: ThreadStartParams,
    ) -> Result<ValidatedProspectiveStart, JSONRPCErrorError> {
        let mut effective = requested.clone();
        loop {
            let prepared =
                match Box::pin(self.alab_prepare_start_parameters(effective.clone())).await {
                    Ok(prepared) => prepared,
                    Err(error) if fallback::start_history(&effective, &error) => {
                        // This original preparation returned before ANY Core call.
                        effective.history_mode = None;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
            let config = self
                .config_manager
                .load_with_overrides(prepared.config.clone(), prepared.typesafe_overrides.clone())
                .await
                .map_err(|error| config_load_error(&error))?;
            validate_loaded_start(&config, prepared.daybreak_enabled)?;
            if let Some(tools) = &prepared.dynamic_tools {
                validate_dynamic_tools(tools).map_err(invalid_request)?;
            }
            // The normal start task owns the later project-trust write and
            // actual Core start. Prospective validation performs neither.
            return Ok(ValidatedProspectiveStart {
                requested,
                _effective: effective,
                config,
                _prepared: prepared,
            });
        }
    }
}
pub(super) fn validate_loaded_start(
    config: &Config,
    daybreak_enabled: Option<bool>,
) -> Result<(), JSONRPCErrorError> {
    if config.ephemeral && daybreak_enabled.is_some() {
        Err(invalid_request(
            "daybreakEnabled is not supported for ephemeral threads",
        ))
    } else {
        Ok(())
    }
}
