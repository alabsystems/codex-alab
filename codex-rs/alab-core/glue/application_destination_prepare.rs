// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual B requirements/configuration load. Never clones A's cloud loader.
use super::*;
impl Plan {
    pub(super) async fn prepare_configuration(
        &self,
        context: codex_login::auth::AlabDestinationResourceContext,
        thread: codex_core::ThreadConfigSnapshot,
    ) -> io::Result<Arc<dyn api::PreparedConfigOwner>> {
        self.bootstrap.validate(&self.owner)?;
        let before = context.current_binding().await?;
        // Serialize the actual shared configuration work without retaining a
        // mutable backend-state borrow through its network/configuration awaits.
        let _preparation = self
            .preparation
            .acquire()
            .await
            .map_err(|_| unavailable())?;
        let next = {
            let mut destination = self.destination.lock().map_err(|_| unavailable())?;
            if let Some(current) = destination.as_ref() {
                if !Arc::ptr_eq(&current.source, context.manager()) {
                    return Err(unavailable());
                }
            } else {
                // Exact B owns this loader; preserve C bootstrap and endpoint bounds.
                let bootstrap = self.bootstrap.local_config();
                let endpoint = codex_backend_client::Client::new(
                    bootstrap.chatgpt_base_url.clone(),
                    self.owner.routes.http_client_factory().clone(),
                )
                .config_bundle_url();
                let factory = self
                    .owner
                    .routes
                    .http_client_factory_for_bootstrap(endpoint.parse::<url::Url>());
                let loader = codex_cloud_config::alab_destination_config_loader(
                    context.manager().clone(),
                    bootstrap.chatgpt_base_url.clone(),
                    self.owner.manager.codex_home.clone(),
                    factory,
                );
                *self
                    .owner
                    .manager
                    .cloud_config_bundle
                    .write()
                    .map_err(|_| unavailable())? = loader;
                *destination = Some(Arc::new(Backend {
                    source: context.manager().clone(),
                    manager: self.owner.manager.clone(),
                    routes: self.owner.routes.clone(),
                }));
            }
            destination.as_ref().cloned().ok_or_else(unavailable)?
        };
        // Keep initialization serialized across participants sharing this actual CM.
        let policy = self
            .owner
            .manager
            .refresh_application_network_policy()
            .await?;
        self.bootstrap.validate(&self.owner)?;
        let overrides = ConfigOverrides {
            model: Some(thread.model.clone()),
            model_provider: Some(thread.model_provider_id.clone()),
            service_tier: Some(thread.service_tier.clone()),
            approval_policy: Some(thread.approval_policy),
            approvals_reviewer: Some(thread.approvals_reviewer),
            permission_profile: Some(thread.permission_profile.clone()),
            workspace_roots: Some(thread.workspace_roots.clone()),
            personality: thread.personality,
            ephemeral: Some(thread.ephemeral),
            ..Default::default()
        };
        let mut config = ConfigBuilder::default()
            .codex_home(self.owner.manager.codex_home.clone())
            .cli_overrides(self.owner.manager.current_cli_overrides())
            .loader_overrides(self.owner.manager.loader_overrides.clone())
            .strict_config(self.owner.manager.strict_config)
            .harness_overrides(overrides)
            .fallback_cwd(Some(thread.cwd().as_path().to_path_buf()))
            .cloud_config_bundle(policy.cloud_config.clone())
            .thread_config_loader(self.owner.manager.thread_config_loader.clone())
            .build()
            .await?;
        self.bootstrap.validate(&self.owner)?;
        self.owner.manager.check_application_policy_load(&policy)?;
        if config
            .config_layer_stack
            .requirements_toml()
            .config_server
            .is_some()
        {
            return Err(unavailable());
        }
        config.model_reasoning_effort = thread.reasoning_effort;
        config.model_reasoning_summary = thread.reasoning_summary;
        self.owner
            .manager
            .apply_runtime_feature_enablement(&mut config);
        self.owner.manager.apply_arg0_paths(&mut config);
        let routes = self
            .owner
            .routes
            .alab_rebase_destination_options(&config.auth_route_config())?;
        config.application_network_policy = routes.http_client_factory().network_policy().clone();
        config.application_auth_route_config = Some(routes);
        config.config_layer_stack = config
            .config_layer_stack
            .with_mcp_ema_cancellation(self.owner.manager.capture_mcp_ema_cancellation());
        if context.current_binding().await? != before {
            return Err(unavailable());
        }
        let prepared = Prepared {
            owner: self.owner.clone(),
            bootstrap: self.bootstrap.clone(),
            next,
            config: Arc::new(config),
            policy,
        };
        api::PreparedConfigOwner::validate(&prepared)?;
        Ok(Arc::new(prepared))
    }
}
