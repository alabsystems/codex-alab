// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Both original managed-cloud routes, without creating an unused AuthManager.
//! Compile as child of bundle_loader; actual source constructors stay shared.
use super::*;

pub struct ManagedBootstrapOwner {
    manager: Option<Arc<AuthManager>>,
    cloud: alab_bootstrap::OwnedLoader,
}
impl ManagedBootstrapOwner {
    pub async fn prepare(
        auth: AuthConfig,
        requirements: &ConfigRequirementsToml,
        enable_codex_api_key_env: bool,
    ) -> std::io::Result<Self> {
        if let Some(config) = requirements.config_server.as_ref() {
            let factory = config_server_http_client_factory(config, &auth.auth_route_config);
            let (loader, refresh) =
                crate::config_server::alab_owned::parts(config.clone(), auth.codex_home, factory)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
            return Ok(Self {
                manager: None,
                cloud: alab_bootstrap::OwnedLoader::from_parts(loader, refresh),
            });
        }
        let prepared =
            alab_bootstrap::PreparedAuth::prepare(auth, enable_codex_api_key_env).await?;
        let manager = Some(prepared.manager().clone());
        Ok(Self {
            manager,
            cloud: prepared.into_owned_loader(),
        })
    }
    pub fn manager(&self) -> Option<&Arc<AuthManager>> {
        self.manager.as_ref()
    }
    pub fn loader(&self) -> &CloudConfigBundleLoader {
        self.cloud.loader()
    }
    pub async fn shutdown(&mut self) -> Arc<std::io::Result<()>> {
        self.cloud.shutdown().await
    }
}
