// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::super::AmazonBedrockModelProvider;
use super::super::BedrockEndpoint;
use super::super::auth;
use super::super::is_amazon_bedrock_gov_cloud_region;
use super::super::is_supported_amazon_bedrock_region;
use super::super::static_gov_model_catalog;
use super::super::static_model_catalog;
use codex_http_client::HttpClientFactory;
use codex_http_client::NetworkPolicy;
use codex_login::AuthChangeState;
use codex_login::AuthManager;
use codex_models_manager::CatalogModel;
use codex_models_manager::ModelOrigin;
use codex_models_manager::ModelsManagerConfig;
use codex_models_manager::manager::ModelsManager;
use codex_models_manager::manager::ModelsManagerFuture;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::config_types::CollaborationModeMask;
use codex_protocol::openai_models::ModelInfo;
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::sync::TryLockError;

impl AmazonBedrockModelProvider {
    pub(in super::super) fn alab_regional_mantle_manager(&self) -> Option<SharedModelsManager> {
        (codex_alab_core::active()
            && self.endpoint == BedrockEndpoint::Mantle
            && self.info.base_url.is_none())
        .then(|| {
            Arc::new(MantleRegionalCatalog {
                provider: self.clone(),
                commercial: StaticModelsManager::new(
                    None,
                    static_model_catalog(),
                    ModelOrigin::Provider,
                ),
                gov: StaticModelsManager::new(
                    None,
                    static_gov_model_catalog(),
                    ModelOrigin::Provider,
                ),
                observed: RwLock::default(),
                admitting: Semaphore::new(1),
            }) as SharedModelsManager
        })
    }
}

/// The native route can change with the credential owner. Authoritative rows
/// and cached metadata observations are bound to the same source as requests.
#[derive(Debug)]
struct MantleRegionalCatalog {
    provider: AmazonBedrockModelProvider,
    commercial: StaticModelsManager,
    gov: StaticModelsManager,
    observed: RwLock<Option<(RegionSource, String)>>,
    admitting: Semaphore,
}

#[derive(Debug, PartialEq, Eq)]
struct RegionSource {
    auth: auth::BedrockAuthSource,
    revision: Option<AuthChangeState>,
    policy: NetworkPolicy,
}

impl MantleRegionalCatalog {
    fn source(&self) -> RegionSource {
        let p = &self.provider;
        RegionSource {
            auth: p.auth_source(),
            revision: p
                .auth_manager
                .as_ref()
                .map(|owner| *owner.auth_change_state_receiver().borrow()),
            policy: p
                .http_client_factory
                .network_policy()
                .clone()
                .for_current_account(),
        }
    }

    fn observed_region(&self, source: &RegionSource) -> Option<String> {
        self.observed
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|(owner, _)| owner == source)
            .map(|(_, region)| region.clone())
    }

    fn region(&self) -> Option<String> {
        let source = self.source();
        let region = self
            .provider
            .alab_local_mantle_region()
            .or_else(|| self.observed_region(&source));
        region.filter(|r| is_supported_amazon_bedrock_region(r) && source == self.source())
    }

    fn for_region(&self, region: Option<&str>) -> &StaticModelsManager {
        if region.is_some_and(is_amazon_bedrock_gov_cloud_region) {
            &self.gov
        } else {
            &self.commercial
        }
    }

    fn catalog(&self) -> &StaticModelsManager {
        self.for_region(self.region().as_deref())
    }
}

impl ModelsManager for MantleRegionalCatalog {
    fn alab_prepare_mantle_thread(&self) -> ModelsManagerFuture<'_, ()> {
        Box::pin(async {
            let Ok(_admission) = self.admitting.acquire().await else {
                return;
            };
            let source = self.source();
            if self.provider.alab_local_mantle_region().is_some()
                || self.observed_region(&source).is_some()
            {
                return;
            }
            let p = &self.provider;
            let region = auth::resolve_region(
                source.auth,
                p.managed_auth().as_ref(),
                &p.aws,
                BedrockEndpoint::Mantle,
                &p.http_client_factory
                    .clone()
                    .with_network_policy(source.policy.clone()),
            )
            .await;
            if let Ok(region) = region
                && is_supported_amazon_bedrock_region(&region)
                && source == self.source()
                && p.alab_local_mantle_region().is_none()
            {
                *self
                    .observed
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((source, region));
            }
        })
    }

    fn alab_confirmed_mantle_region(&self) -> Option<String> {
        let source = self.source();
        if self.provider.alab_local_mantle_region().is_some() {
            return None;
        }
        self.observed_region(&source)
            .filter(|_| source == self.source())
    }

    fn ensure_initialized(&self, factory: HttpClientFactory) -> ModelsManagerFuture<'_, ()> {
        self.catalog().ensure_initialized(factory)
    }
    fn model_catalog_with_origins(
        &self,
        strategy: RefreshStrategy,
        factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'_, Vec<CatalogModel>> {
        Box::pin(async move {
            let confirmed = self.region();
            let catalog = self.for_region(confirmed.as_deref());
            let mut rows = catalog
                .model_catalog_with_origins(strategy, factory.clone())
                .await;
            if confirmed.is_none() {
                // Commercial-only display rows cannot authorize a saved-model
                // migration before this provider's region has been confirmed.
                let common = self.gov.model_catalog_with_origins(strategy, factory).await;
                for row in &mut rows {
                    if !common.iter().any(|allowed| allowed.slug == row.slug) {
                        row.origin = ModelOrigin::Bundled;
                    }
                }
            }
            rows
        })
    }
    fn get_default_model<'a>(
        &'a self,
        model: &'a Option<String>,
        fallback: bool,
        strategy: RefreshStrategy,
        factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'a, String> {
        let catalog = match self.region() {
            Some(region) => self.for_region(Some(&region)),
            None => &self.gov,
        };
        catalog.get_default_model(model, fallback, strategy, factory)
    }
    fn get_remote_models(&self) -> ModelsManagerFuture<'_, Vec<ModelInfo>> {
        self.catalog().get_remote_models()
    }
    fn try_get_remote_models(&self) -> Result<Vec<ModelInfo>, TryLockError> {
        self.catalog().try_get_remote_models()
    }
    fn auth_manager(&self) -> Option<&AuthManager> {
        None
    }
    fn list_collaboration_modes(&self) -> Vec<CollaborationModeMask> {
        self.catalog().list_collaboration_modes()
    }
    fn get_model_info<'a>(
        &'a self,
        model: &'a str,
        config: &'a ModelsManagerConfig,
    ) -> ModelsManagerFuture<'a, CatalogModel> {
        self.catalog().get_model_info(model, config)
    }
    fn refresh_if_new_etag(
        &self,
        etag: String,
        factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'_, ()> {
        self.catalog().refresh_if_new_etag(etag, factory)
    }
}
