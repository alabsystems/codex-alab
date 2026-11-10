// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Local-only configuration for a destination before credential construction.
//! This does not load account requirements or publish an effective B policy.
use super::*;
use codex_login::AuthConfig;
use std::io;

pub(crate) struct DestinationBootstrap {
    config: Arc<Config>,
    auth: AuthConfig,
    inputs: local_inputs::Snapshot,
}
impl DestinationBootstrap {
    pub(super) fn with_current(
        &self,
        owner: &DestinationConfigOwner,
        publish: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        self.inputs.with_current(&owner.manager, publish)
    }

    pub(crate) fn auth_config(&self) -> &AuthConfig {
        &self.auth
    }

    /// Local inputs only, not B requirements or a complete resource bundle.
    pub(crate) fn local_config(&self) -> &Arc<Config> {
        &self.config
    }

    pub(crate) fn validate(&self, owner: &DestinationConfigOwner) -> io::Result<()> {
        self.inputs.validate(&owner.manager)?;
        if !self.auth.auth_route_config.alab_has_configuration_owners(
            &owner.manager.network_policy.policy(),
            &owner.manager.local_network_policy.policy(),
        )? || self.auth.auth_route_config != owner.routes
        {
            return Err(io::Error::other("destination bootstrap routes changed"));
        }
        Ok(())
    }
}

impl DestinationConfigOwner {
    pub(crate) async fn local_bootstrap(
        &mut self,
        cwd: PathBuf,
    ) -> io::Result<DestinationBootstrap> {
        let inputs = self.manager.alab_capture_local_inputs()?;
        inputs.validate(&self.manager)?;
        self.manager.alab_destination_thread_source()?;
        if self
            .manager
            .network_policy_snapshot
            .read()
            .map_err(|_| io::Error::other("destination policy snapshot poisoned"))?
            .is_some()
        {
            return Err(io::Error::other("destination policy is already composed"));
        }
        // Only the actual live local controller is refreshed. This method never
        // invokes the application-policy loader or publishes B's controller.
        let local = self.manager.refresh_local_network_policy().await?;
        inputs.validate(&self.manager)?;
        let cancellation = self.manager.capture_mcp_ema_cancellation();
        let mut config = ConfigBuilder::default()
            .codex_home(self.manager.codex_home.clone())
            .cli_overrides(inputs.cli_overrides.clone())
            .loader_overrides(self.manager.loader_overrides.clone())
            .strict_config(self.manager.strict_config)
            .fallback_cwd(Some(cwd))
            .cloud_config_bundle(CloudConfigBundleLoader::default())
            .thread_config_loader(Arc::clone(&self.manager.thread_config_loader))
            .build()
            .await?;
        inputs.validate(&self.manager)?;
        // No API currently proves that an old loader's config-server worker is
        // independent C authority. Do not copy A's getter or silently omit it.
        if config
            .config_layer_stack
            .requirements_toml()
            .config_server
            .is_some()
        {
            return Err(io::Error::other(
                "independent config-server destination authority is unavailable",
            ));
        }
        let built_application = config
            .config_layer_stack
            .requirements()
            .application
            .as_ref()
            .map(|application| application.value.clone());
        if local.compose(Default::default())? != built_application {
            return Err(io::Error::other(
                "local application requirements changed during bootstrap",
            ));
        }
        // Use the same protected-feature mapping as ordinary ConfigManager
        // loads, with the coherently captured C values rather than a live reread.
        super::super::apply_runtime_feature_enablement(
            &mut config,
            &inputs.runtime_feature_enablement,
        );
        self.manager.apply_arg0_paths(&mut config);
        let options = config.auth_route_config();
        let routes = self.routes.alab_rebase_destination_options(&options)?;
        if !routes.alab_has_configuration_owners(
            &self.manager.network_policy.policy(),
            &self.manager.local_network_policy.policy(),
        )? {
            return Err(io::Error::other("destination configuration owner differs"));
        }
        // Content includes all original application/content endpoint bounds.
        // The controller remains unavailable; this is not an effective publish.
        config.application_network_policy = routes.http_client_factory().network_policy().clone();
        config.application_auth_route_config = Some(routes.clone());
        config.config_layer_stack = config
            .config_layer_stack
            .with_mcp_ema_cancellation(cancellation);
        let auth = config.auth_config();
        auth.validate()?;
        inputs.validate(&self.manager)?;
        self.routes = routes;
        let bootstrap = DestinationBootstrap {
            config: Arc::new(config),
            auth,
            inputs,
        };
        bootstrap.validate(self)?;
        Ok(bootstrap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_http_client::HttpClientFactory;
    use codex_http_client::NetworkPolicyDenied;
    use codex_http_client::OutboundProxyPolicy;

    async fn forbidden_source_cloud()
    -> Result<Option<codex_config::CloudConfigBundle>, codex_config::CloudConfigBundleLoadError>
    {
        panic!("local bootstrap must never invoke A's cloud getter")
    }

    fn fixture(requirements: &str) -> (tempfile::TempDir, ConfigManager, DestinationConfigOwner) {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            "[features]\nrespect_system_proxy=false\n",
        )
        .unwrap();
        let requirements_path = home.path().join("requirements.toml");
        std::fs::write(&requirements_path, requirements).unwrap();
        let overrides = LoaderOverrides {
            managed_config_path: Some(home.path().join("missing-managed.toml")),
            system_config_path: Some(home.path().join("missing-system.toml")),
            system_requirements_path: Some(requirements_path),
            ..LoaderOverrides::without_managed_config_for_tests()
        };
        let source = ConfigManager::new(
            home.path().to_path_buf(),
            Vec::new(),
            overrides,
            /*strict_config*/ true,
            CloudConfigBundleLoader::from_getter(forbidden_source_cloud),
            Arg0DispatchPaths::default(),
            alab_no_external_thread_config_loader(codex_config::NoopThreadConfigLoader),
        );
        let factory = |policy| {
            HttpClientFactory::new(OutboundProxyPolicy::RespectSystemProxy)
                .with_network_policy(policy)
        };
        let routes =
            AuthRouteConfig::from_http_client_factory(factory(source.network_policy.policy()))
                .with_local_bootstrap_factory(factory(source.local_network_policy.policy()));
        let owner = source.alab_begin_registered_destination(&routes).unwrap();
        (home, source, owner)
    }

    #[tokio::test]
    async fn actual_local_bootstrap_changes_options_but_never_publishes_b_and_rejects_input_aba() {
        let (home, source, mut owner) = fixture("");
        source
            .extend_runtime_feature_enablement([("respect_system_proxy".into(), true)])
            .unwrap();
        let bootstrap = owner
            .local_bootstrap(home.path().to_path_buf())
            .await
            .unwrap();
        bootstrap.validate(&owner).unwrap();
        // An explicit local feature is protected by normal ConfigManager mapping.
        assert!(
            !bootstrap
                .local_config()
                .features
                .enabled(codex_features::Feature::RespectSystemProxy)
        );
        assert_eq!(
            bootstrap
                .auth_config()
                .auth_route_config
                .http_client_factory()
                .outbound_proxy_policy(),
            OutboundProxyPolicy::ReqwestDefault
        );
        assert!(
            owner
                .manager
                .network_policy_snapshot
                .read()
                .unwrap()
                .is_none()
        );
        let url: url::Url = "https://auth.example.test/token".parse().unwrap();
        assert!(matches!(
            owner.manager.network_policy.policy().acquire(&url),
            Err(NetworkPolicyDenied::Unavailable)
        ));
        assert!(
            bootstrap
                .auth_config()
                .auth_route_config
                .alab_has_configuration_owners(
                    &owner.manager.network_policy.policy(),
                    &source.local_network_policy.policy(),
                )
                .unwrap()
        );
        source
            .extend_runtime_feature_enablement([("respect_system_proxy".into(), false)])
            .unwrap();
        source
            .extend_runtime_feature_enablement([("respect_system_proxy".into(), true)])
            .unwrap();
        assert!(
            bootstrap.validate(&owner).is_err(),
            "equal-value C input ABA must be refused"
        );
        assert!(matches!(
            owner.manager.network_policy.policy().acquire(&url),
            Err(NetworkPolicyDenied::Unavailable)
        ));
    }

    #[tokio::test]
    async fn actual_invalid_local_managed_policy_fails_without_default_or_b_publication() {
        let (home, source, mut owner) = fixture("[application.network]\nenabled='invalid'\n");
        let url: url::Url = "https://auth.example.test/token".parse().unwrap();
        assert!(source.local_network_policy.publish(
            source.local_network_policy.policy().revision(),
            codex_http_client::DestinationPolicy::Unrestricted
        ));
        let before = owner.manager.network_policy.policy();
        assert!(
            owner
                .local_bootstrap(home.path().to_path_buf())
                .await
                .is_err()
        );
        assert_eq!(before, owner.manager.network_policy.policy());
        assert!(
            owner
                .manager
                .network_policy_snapshot
                .read()
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            owner.manager.network_policy.policy().acquire(&url),
            Err(NetworkPolicyDenied::Unavailable)
        ));
        assert!(matches!(
            source.local_network_policy.policy().acquire(&url),
            Err(NetworkPolicyDenied::Unavailable)
        ));
    }

    #[tokio::test]
    async fn local_config_server_requirements_need_independent_authority_before_b_auth() {
        let (home, _, mut owner) = fixture(
            r#"
[config_server]
base_url = "https://settings.example.test/policy"
[config_server.oauth]
authorization_url = "https://login.example.test/authorize"
token_url = "https://login.example.test/token"
client_id = "config-client"
scopes = ["policy.read"]
"#,
        );
        let error = match owner.local_bootstrap(home.path().to_path_buf()).await {
            Ok(_) => panic!("independent config-server authority must not be inferred"),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            "independent config-server destination authority is unavailable"
        );
        let url: url::Url = "https://settings.example.test/policy".parse().unwrap();
        assert!(matches!(
            owner.manager.network_policy.policy().acquire(&url),
            Err(NetworkPolicyDenied::Unavailable)
        ));
        assert!(
            owner
                .manager
                .network_policy_snapshot
                .read()
                .unwrap()
                .is_none()
        );
    }
}
