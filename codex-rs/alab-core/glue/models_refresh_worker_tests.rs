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
    let calls = Arc::new(Calls::default());
    let endpoint = Arc::new(Endpoint(Arc::clone(&calls), work.clone()));
    fixture_with_endpoint(calls, endpoint, work.is_some()).await
}

async fn fixture_with_endpoint(
    calls: Arc<Calls>,
    endpoint: Arc<dyn ModelsEndpointClient>,
    retain_work: bool,
) -> (tempfile::TempDir, Arc<ModelCatalog>, Arc<Calls>) {
    let codex_home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch home: {err}"));
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
        endpoint,
        /*auth_manager*/ None,
    ));
    if retain_work {
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

#[derive(Debug, Default)]
struct PreflightGate {
    entered: std::sync::atomic::AtomicUsize,
    dropped: std::sync::atomic::AtomicUsize,
    started: Notify,
    release: CancellationToken,
}

struct PreflightObserver(Arc<PreflightGate>);

impl Drop for PreflightObserver {
    fn drop(&mut self) {
        self.0
            .dropped
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct PendingPreflightEndpoint {
    gate: Arc<PreflightGate>,
    calls: Arc<Calls>,
}

impl ModelsEndpointClient for PendingPreflightEndpoint {
    fn identity(&self) -> Option<String> {
        Some(IDENTITY.to_owned())
    }

    fn has_command_auth(&self) -> bool {
        true
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            let _observer = PreflightObserver(Arc::clone(&gate));
            gate.entered
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            gate.started.notify_one();
            gate.release.cancelled().await;
            false
        })
    }

    fn list_models<'a>(
        &'a self,
        _client_version: &'a str,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<ModelsEndpointResponse>> {
        self.calls.push("fetch");
        Box::pin(std::future::ready(Ok(ModelsEndpointResponse {
            models: Vec::new(),
            etag: None,
            identity: IDENTITY.to_owned(),
        })))
    }
}

#[derive(Clone, Copy, Debug)]
enum StopWorker {
    Shutdown,
    Drop,
}

#[derive(Clone, Copy, Debug)]
enum StopPoint {
    BeforeStart,
    DuringPreflight,
}

#[derive(Debug)]
struct ShutdownObservation {
    enhanced: bool,
    stop: StopWorker,
    point: StopPoint,
    reached_stop_point: bool,
    entered: usize,
    dropped_while_held: usize,
    gate_held: bool,
    catalog_released: bool,
    task_finished: bool,
    calls_while_held: Vec<&'static str>,
    clean_join: bool,
    forced_cleanup: bool,
    final_observer_drops: usize,
}

const OBSERVE_SHUTDOWN: Duration = Duration::from_secs(2);
const CLEANUP_SHUTDOWN: Duration = Duration::from_secs(5);

// This completed placeholder lets the test retain the actual JoinHandle across
// real worker Drop. It starts no worker logic and is finished before real spawn.
async fn completed_task_slot() -> JoinHandle<()> {
    let slot = tokio::spawn(async {});
    tokio::time::timeout(CLEANUP_SHUTDOWN, async {
        while !slot.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("empty task slot completes before the model worker starts");
    slot
}

async fn observe_shutdown(
    enhanced: bool,
    stop: StopWorker,
    point: StopPoint,
) -> ShutdownObservation {
    let gate = Arc::new(PreflightGate::default());
    let calls = Arc::new(Calls::default());
    let endpoint = Arc::new(PendingPreflightEndpoint {
        gate: Arc::clone(&gate),
        calls: Arc::clone(&calls),
    });
    let (_home, catalog, calls) = fixture_with_endpoint(calls, endpoint, false).await;
    let weak_catalog = Arc::downgrade(&catalog);
    let (mode, receiver) = tokio::sync::watch::channel(enhanced);
    let slot = completed_task_slot().await;
    let mut worker = Some(alab_live::spawn_with_mode(
        &catalog,
        receiver,
        Duration::from_secs(3600),
        Duration::from_secs(3600),
    ));
    // These private fields belong to the actual worker under test. Retaining its
    // JoinHandle observes a panic and permits cleanup; no task logic is copied.
    let mut task = std::mem::replace(&mut worker.as_mut().unwrap()._task, slot);
    let observed_task = task.abort_handle(); // Observe only; never abort as proof.
    let reached_stop_point = match point {
        StopPoint::BeforeStart => true,
        StopPoint::DuringPreflight => tokio::time::timeout(CLEANUP_SHUTDOWN, async {
            while gate.entered.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                gate.started.notified().await;
            }
        })
        .await
        .is_ok(),
    };
    match stop {
        StopWorker::Shutdown => worker.as_ref().unwrap().shutdown(),
        StopWorker::Drop => drop(worker.take()),
    }
    drop(catalog);

    // JoinHandle waiting is cancel-safe. Snapshot all expected RED properties
    // while the endpoint gate remains closed, without panicking before cleanup.
    let early_join = tokio::time::timeout(OBSERVE_SHUTDOWN, &mut task).await.ok();
    let mut observation = ShutdownObservation {
        enhanced,
        stop,
        point,
        reached_stop_point,
        entered: gate.entered.load(std::sync::atomic::Ordering::SeqCst),
        dropped_while_held: gate.dropped.load(std::sync::atomic::Ordering::SeqCst),
        gate_held: !gate.release.is_cancelled(),
        catalog_released: weak_catalog.upgrade().is_none(),
        task_finished: observed_task.is_finished(),
        calls_while_held: calls.log(),
        clean_join: false,
        forced_cleanup: false,
        final_observer_drops: 0,
    };

    // Old source is still awaiting preflight. Release it and join before any
    // expected failure, so the regression cannot leave that worker running.
    gate.release.cancel();
    observation.clean_join = match early_join {
        Some(result) => result.is_ok(),
        None => match tokio::time::timeout(CLEANUP_SHUTDOWN, &mut task).await {
            Ok(result) => result.is_ok(),
            Err(_) => {
                // Abnormal custody fallback only, after the immutable snapshot.
                // Forced cleanup can never turn the contract green.
                observation.forced_cleanup = true;
                task.abort();
                tokio::time::timeout(CLEANUP_SHUTDOWN, &mut task)
                    .await
                    .expect("model worker cleanup abort/reap deadline")
                    .expect_err("forced cleanup cancels the model worker");
                false
            }
        },
    };
    observation.final_observer_drops = gate.dropped.load(std::sync::atomic::Ordering::SeqCst);
    drop(worker);
    drop(mode);
    observation
}

fn assert_shutdown_observation(observation: &ShutdownObservation) {
    let case = (observation.enhanced, observation.stop, observation.point);
    assert!(observation.clean_join, "case {case:?}: {observation:?}");
    assert!(!observation.forced_cleanup, "{observation:?}");
    assert!(observation.reached_stop_point, "{observation:?}");
    assert!(observation.gate_held, "{observation:?}");
    assert!(observation.calls_while_held.is_empty(), "{observation:?}");
    let expected_observers = match observation.point {
        StopPoint::BeforeStart => 0,
        StopPoint::DuringPreflight => 1,
    };
    assert_eq!(observation.entered, expected_observers, "{observation:?}");
    assert_eq!(
        observation.final_observer_drops, expected_observers,
        "cleanup leaves no preflight observer: {observation:?}"
    );
    // Expected old-source RED follows actual preflight and clean task custody.
    assert_eq!(
        observation.dropped_while_held, expected_observers,
        "shutdown drops its observer without waiting for source readiness: {observation:?}"
    );
    assert!(observation.catalog_released, "{observation:?}");
    assert!(observation.task_finished, "{observation:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn live_worker_shutdown_releases_pending_preflight_in_both_modes() {
    let mut observations = Vec::new();
    for enhanced in [true, false] {
        observations
            .push(observe_shutdown(enhanced, StopWorker::Shutdown, StopPoint::BeforeStart).await);
        observations.push(
            observe_shutdown(enhanced, StopWorker::Shutdown, StopPoint::DuringPreflight).await,
        );
    }
    for observation in observations {
        assert_shutdown_observation(&observation);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn live_worker_drop_releases_pending_preflight_in_both_modes() {
    let mut observations = Vec::new();
    for enhanced in [true, false] {
        observations
            .push(observe_shutdown(enhanced, StopWorker::Drop, StopPoint::DuringPreflight).await);
    }
    for observation in observations {
        assert_shutdown_observation(&observation);
    }
}

#[path = "models_cross_tab_tests.rs"]
mod cross_tab;
