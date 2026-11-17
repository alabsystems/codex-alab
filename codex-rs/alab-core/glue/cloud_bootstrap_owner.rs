// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The actual original cloud-service constructor, before its loader starts.
//! Both the ordinary storage path and owned Source preparation use this same
//! constructor. Keeping its real manager alive creates no SDK/Native grant.
use super::*;

pub struct PreparedAuth {
    manager: Arc<AuthManager>,
    service: CloudConfigBundleService<ChatGptSource<BackendBundleClient>>,
}
impl PreparedAuth {
    pub async fn prepare(
        auth_config: AuthConfig,
        enable_codex_api_key_env: bool,
    ) -> std::io::Result<Self> {
        let manager =
            AuthManager::shared_from_auth_config(auth_config.clone(), enable_codex_api_key_env)
                .await?;
        let service = CloudConfigBundleService::new(
            manager.clone(),
            Arc::new(BackendBundleClient::new(
                auth_config
                    .chatgpt_base_url
                    .unwrap_or_else(|| "https://chatgpt.com/backend-api/".to_string()),
                auth_config.auth_route_config.http_client_factory().clone(),
            )),
            auth_config.codex_home,
            CLOUD_CONFIG_BUNDLE_TIMEOUT,
        );
        Ok(Self { manager, service })
    }
    pub fn manager(&self) -> &Arc<AuthManager> {
        &self.manager
    }
    pub fn into_owned_loader(self) -> OwnedLoader {
        let (loader, refresh) = managed_config_bundle_loader_for_service(self.service);
        OwnedLoader {
            loader,
            refresh,
            completed: None,
        }
    }
    pub(super) fn into_service(
        self,
    ) -> CloudConfigBundleService<ChatGptSource<BackendBundleClient>> {
        self.service
    }
}

/// Actual original loader and its one existing refresh task. Ordinary storage
/// callers keep their existing loader lifetime; this partial owner additionally
/// retains a joinable handle for explicit Source phase/runtime shutdown.
pub struct OwnedLoader {
    loader: CloudConfigBundleLoader,
    refresh: JoinHandle<()>,
    completed: Option<Arc<std::io::Result<()>>>,
}
impl OwnedLoader {
    pub(crate) fn from_parts(loader: CloudConfigBundleLoader, refresh: JoinHandle<()>) -> Self {
        Self {
            loader,
            refresh,
            completed: None,
        }
    }
    pub fn loader(&self) -> &CloudConfigBundleLoader {
        &self.loader
    }
    pub async fn shutdown(&mut self) -> Arc<std::io::Result<()>> {
        if let Some(result) = &self.completed {
            return result.clone();
        }
        self.refresh.abort();
        // Borrow the actual handle: cancelling this observation keeps its wait
        // custody in this owner; no replacement task or credential read starts.
        let result = match (&mut self.refresh).await {
            Ok(()) => Ok(()),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(_) => Err(std::io::Error::other(
                "bootstrap cloud refresh outcome unobserved",
            )),
        };
        let result = Arc::new(result);
        self.completed = Some(result.clone());
        result
    }
}
impl Drop for OwnedLoader {
    fn drop(&mut self) {
        self.refresh.abort();
    }
}
