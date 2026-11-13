// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(model-poll) glue tests, compiled inside codex-app-server as
//! `models_refresh_worker::alab_tests` (one `#[path]` line at the end of
//! `app-server/src/models_refresh_worker.rs`): the poll's two arms. codex's
//! `spawn_with_interval` passes `codex_alab_core::active()` as `reads_fresh_cache`; this test
//! binary never activates (every upstream test in it runs as upstream's), so each arm is driven
//! here by that argument.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_core::config::ConfigBuilder;
use codex_http_client::HttpClientFactory;
use codex_models_manager::cache::ModelsCache;
use codex_models_manager::cache::ModelsCacheEntry;
use codex_models_manager::cache::ModelsCacheError;
use codex_models_manager::cache::ModelsCacheFuture;
use codex_models_manager::manager::ModelsEndpointClient;
use codex_models_manager::manager::ModelsEndpointFuture;
use codex_models_manager::manager::ModelsEndpointResponse;
use codex_models_manager::manager::OpenAiModelsManager;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::error::Result as CoreResult;
use pretty_assertions::assert_eq;
use tokio::sync::Notify;

use super::*;

const IDENTITY: &str = "alab-launch-poll";

/// What the cache and the endpoint were asked, in order; `called` wakes on each call.
#[derive(Debug, Default)]
struct Calls {
    log: Mutex<Vec<&'static str>>,
    called: Notify,
}

impl Calls {
    fn push(&self, call: &'static str) {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(call);
        self.called.notify_one();
    }

    fn log(&self) -> Vec<&'static str> {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// A cache that always holds a catalog downloaded just now.
#[derive(Debug)]
struct FreshCache(Arc<Calls>);

impl ModelsCache for FreshCache {
    fn load<'a>(
        &'a self,
        client_version: &'a str,
    ) -> ModelsCacheFuture<'a, Result<Option<ModelsCacheEntry>, ModelsCacheError>> {
        self.0.push("load");
        Box::pin(std::future::ready(Ok(Some(ModelsCacheEntry {
            fetched_at: Utc::now(),
            etag: None,
            client_version: Some(client_version.to_string()),
            identity: Some(IDENTITY.to_string()),
            models: Vec::new(),
        }))))
    }

    fn store<'a>(
        &'a self,
        _entry: &'a ModelsCacheEntry,
    ) -> ModelsCacheFuture<'a, Result<(), ModelsCacheError>> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn refresh_ttl<'a>(
        &'a self,
        _client_version: &'a str,
        _identity: &'a str,
        _etag: &'a str,
    ) -> ModelsCacheFuture<'a, Result<(), ModelsCacheError>> {
        Box::pin(std::future::ready(Ok(())))
    }
}

#[derive(Debug)]
struct Endpoint(Arc<Calls>, Option<Arc<Calls>>);

impl ModelsEndpointClient for Endpoint {
    fn for_originator(
        &self,
        _originator: codex_models_manager::ModelCatalogOriginator,
    ) -> Option<Arc<dyn ModelsEndpointClient>> {
        self.1.as_ref().map(|calls| {
            Arc::new(Endpoint(Arc::clone(calls), None)) as Arc<dyn ModelsEndpointClient>
        })
    }
    fn identity(&self) -> Option<String> {
        Some(IDENTITY.to_string())
    }

    fn has_command_auth(&self) -> bool {
        true
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(std::future::ready(false))
    }

    fn list_models<'a>(
        &'a self,
        _client_version: &'a str,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<ModelsEndpointResponse>> {
        self.0.push("fetch");
        Box::pin(std::future::ready(Ok(ModelsEndpointResponse {
            models: Vec::new(),
            etag: None,
            identity: IDENTITY.to_string(),
        })))
    }
}

/// The first three calls the worker made, polling every 10 ms over a cache that is always fresh.
async fn fixture() -> (tempfile::TempDir, Arc<ModelCatalog>, Arc<Calls>) {
    fixture_with_work(None).await
}

async fn fixture_with_work(
    work: Option<Arc<Calls>>,
) -> (tempfile::TempDir, Arc<ModelCatalog>, Arc<Calls>) {
    let codex_home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch home: {err}"));
    let calls = Arc::new(Calls::default());
    let config_manager = crate::config_manager::ConfigManager::new_for_tests(
        codex_home.path().to_path_buf(),
        Vec::new(),
        LoaderOverrides::without_managed_config_for_tests(),
        CloudConfigBundleLoader::default(),
    );
    let config = Arc::new(
        ConfigBuilder::default()
            .codex_home(codex_home.path().to_path_buf())
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .build()
            .await
            .unwrap_or_else(|err| panic!("config: {err}")),
    );
    let models_manager: SharedModelsManager = Arc::new(OpenAiModelsManager::new_with_cache(
        Arc::new(FreshCache(Arc::clone(&calls))),
        Arc::new(Endpoint(Arc::clone(&calls), work.clone())),
        /*auth_manager*/ None,
    ));
    if work.is_some() {
        models_manager
            .for_originator("codex_work_desktop")
            .expect("retained Work catalog");
    }
    let auth_manager = codex_login::AuthManager::shared_from_config(
        config.as_ref(),
        /*enable_codex_api_key_env*/ false,
    )
    .await
    .unwrap_or_else(|err| panic!("auth manager: {err}"));
    let startup = codex_core::build_startup_models(
        &config,
        auth_manager,
        uuid::Uuid::new_v4().to_string(),
        codex_core::test_support::fixed_model_catalog(models_manager),
    );
    let thread_manager = Arc::new(codex_core::ThreadManager::new(
        &config,
        startup,
        codex_core::CodexAppsToolsCache::default(),
        codex_protocol::protocol::SessionSource::Exec,
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        codex_extension_api::empty_extension_registry(),
        Arc::new(codex_core::test_support::EmptyUserInstructionsProvider),
        /*analytics_events_client*/ None,
        codex_core::passthrough_image_store(),
        codex_core::ThreadStoreBuilder::from_config(&config, /*state_db*/ None).build(),
        /*agent_graph_store*/ None,
        /*attestation_provider*/ None,
        /*external_time_provider*/ None,
    ));
    let catalog = Arc::new(ModelCatalog::new(
        config_manager,
        Some(config.cwd.to_path_buf()),
        config,
        thread_manager,
    ));
    (codex_home, catalog, calls)
}

async fn first_three_polls(reads_fresh_cache: bool) -> Vec<&'static str> {
    let (_home, catalog, calls) = fixture().await;
    let worker = spawn_polling(&catalog, Duration::from_millis(10), reads_fresh_cache);
    // A hang detector: three polls come within 30 ms.
    tokio::time::timeout(Duration::from_secs(60), async {
        while calls.log().len() < 3 {
            calls.called.notified().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("three polls did not come within a minute"));
    drop(worker);
    let mut log = calls.log();
    log.truncate(3);
    log
}

#[tokio::test]
async fn live_mode_changes_wake_the_worker_and_restore_native_polling() {
    let (_home, catalog, calls) = fixture().await;
    let (mode, receiver) = tokio::sync::watch::channel(true);
    let worker = alab_live::spawn_with_mode(
        &catalog,
        receiver,
        Duration::from_secs(3600),
        Duration::from_millis(10),
    );
    async fn wait_for(calls: &Calls, count: usize) {
        tokio::time::timeout(Duration::from_secs(60), async {
            while calls.log().len() < count {
                calls.called.notified().await;
            }
        })
        .await
        .expect("model worker did not respond");
    }
    wait_for(&calls, 1).await;
    assert_eq!(calls.log(), vec!["load"]);
    mode.send(false).unwrap();
    wait_for(&calls, 3).await;
    assert_eq!(&calls.log()[..3], &["load", "fetch", "fetch"]);
    mode.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if calls.log().last() == Some(&"load") {
                break;
            }
            calls.called.notified().await;
        }
    })
    .await
    .expect("enhanced mode did not wake the worker");
    let count = calls.log().len();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(
        calls.log().len(),
        count,
        "enhanced mode keeps its hourly poll"
    );
    drop(worker);
}

/// codex's polls read the fresh cache, whichever tab downloaded it, and download nothing: the
/// tabs share one download an hour, not one each. NEGATIVE CONTROL: with the poll `Online` (the
/// hook at the `list_models` call reverted) every call is a download.
#[tokio::test]
async fn codexs_polls_read_a_fresh_cache() {
    assert_eq!(first_three_polls(true).await, vec!["load", "load", "load"]);
}

/// Upstream's worker (every binary but codex) downloads at every poll, cache or not.
#[tokio::test]
async fn upstreams_polls_download() {
    assert!(!codex_alab_core::active());
    assert_eq!(
        first_three_polls(false).await,
        vec!["fetch", "fetch", "fetch"]
    );
}

/// The root can use another tab's fresh disk catalog while retained originators
/// still need successful provider observations to update their saved settings.
#[tokio::test]
async fn cache_aware_polls_refresh_retained_originators_without_refetching_default() {
    let work = Arc::new(Calls::default());
    let (_home, catalog, root) = fixture_with_work(Some(Arc::clone(&work))).await;
    catalog.list_models(RefreshStrategy::Offline).await.unwrap();
    assert!(
        work.log().is_empty(),
        "offline reads must never refresh originators"
    );
    for _ in 0..2 {
        catalog
            .list_models(RefreshStrategy::OnlineIfUncached)
            .await
            .unwrap();
    }
    assert_eq!(root.log(), vec!["load", "load", "load"]);
    assert_eq!(work.log(), vec!["fetch", "fetch"]);
}
