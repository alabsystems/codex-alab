// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! codex-alab's model catalog cache sits in `models_cache.alab.json`, beside the
//! `models_cache.json` `codex-old` (OpenAI's own build, sharing `~/.codex`) keeps
//! (`ALAB(models-cache)`). The switch is on in this test binary, as in the codex binary;
//! `inactive.rs` holds the run without it.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_models_manager::manager::ModelsEndpointClient;
use codex_models_manager::manager::ModelsEndpointFuture;
use codex_models_manager::manager::ModelsEndpointResponse;
use codex_models_manager::manager::ModelsManager;
use codex_models_manager::manager::OpenAiModelsManager;
use codex_models_manager::manager::RefreshStrategy;
use pretty_assertions::assert_eq;

const IDENTITY: &str = "alab-test-provider";
/// What codex-old 0.164.0 leaves in its cache: a catalog only its own version trusts.
const CODEX_OLDS: &str = r#"{"fetched_at":"2026-10-23T14:00:00Z","etag":null,"client_version":"0.164.0","identity":"alab-test-provider","models":[]}"#;

/// The models endpoint, counting the fetches codex-alab makes.
#[derive(Debug, Default)]
struct Catalog {
    fetches: AtomicUsize,
}

impl ModelsEndpointClient for Catalog {
    fn identity(&self) -> Option<String> {
        Some(IDENTITY.to_string())
    }

    fn has_command_auth(&self) -> bool {
        false
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(std::future::ready(true))
    }

    fn list_models<'a>(
        &'a self,
        _client_version: &'a str,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, codex_protocol::error::Result<ModelsEndpointResponse>> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::ready(Ok(ModelsEndpointResponse {
            models: Vec::new(),
            etag: None,
            identity: IDENTITY.to_string(),
        })))
    }
}

/// One launch: a fresh manager on `home` lists the catalog as startup does.
async fn launch(home: &std::path::Path, catalog: &Arc<Catalog>) {
    list(OpenAiModelsManager::new(
        home.to_path_buf(),
        catalog.clone(),
        None,
    ))
    .await;
}

/// The catalog as startup lists it.
async fn list(manager: OpenAiModelsManager) {
    manager
        .raw_model_catalog(
            RefreshStrategy::OnlineIfUncached,
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        )
        .await;
}

/// codex-alab, then codex-old, then codex-alab again: the second codex-alab launch reads its own
/// cache instead of fetching, and codex-old's cache is never rewritten. NEGATIVE CONTROL: without
/// the hook the first launch writes its catalog over codex-old's `models_cache.json`, so the file
/// assertions fail.
#[tokio::test]
async fn a_launch_after_codex_old_reads_its_own_cache() {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let old = dir.path().join("models_cache.json");
    fs::write(&old, CODEX_OLDS).unwrap_or_else(|err| panic!("{err}"));
    let catalog = Arc::new(Catalog::default());

    launch(dir.path(), &catalog).await;
    assert_eq!(catalog.fetches.load(Ordering::SeqCst), 1);
    assert!(dir.path().join("models_cache.alab.json").is_file());
    assert_eq!(fs::read_to_string(&old).ok().as_deref(), Some(CODEX_OLDS));

    // codex-old runs and rewrites its own cache.
    fs::write(&old, CODEX_OLDS).unwrap_or_else(|err| panic!("{err}"));
    launch(dir.path(), &catalog).await;
    assert_eq!(catalog.fetches.load(Ordering::SeqCst), 1);
    assert_eq!(fs::read_to_string(&old).ok().as_deref(), Some(CODEX_OLDS));
}

/// A catalog downloaded ten minutes ago still serves a launch: codex-alab trusts it as long as
/// its hourly poll's (`ALAB(model-poll)`), where upstream's five minutes had nearly every launch
/// download it again. NEGATIVE CONTROL: with upstream's TTL the launch fetches. A provider's
/// own catalog keeps upstream's five minutes.
#[tokio::test]
async fn a_launch_reads_a_catalog_downloaded_ten_minutes_ago() {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let ten_minutes_ago = chrono::Utc::now() - chrono::Duration::minutes(10);
    let cached = serde_json::json!({
        "fetched_at": ten_minutes_ago.to_rfc3339(),
        "etag": null,
        "client_version": codex_models_manager::client_version_to_whole(),
        "identity": IDENTITY,
        "models": [],
    });
    fs::write(
        dir.path().join("models_cache.alab.json"),
        cached.to_string(),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    let catalog = Arc::new(Catalog::default());

    launch(dir.path(), &catalog).await;
    assert_eq!(catalog.fetches.load(Ordering::SeqCst), 0);

    // A provider's own catalog (`model_catalog_url`) gets no models etag, so nothing would
    // refetch it when it changed: upstream's five minutes hold for it. NEGATIVE CONTROL: without
    // the provider hook in `try_load_cache` this launch reads the cache too.
    let provider = OpenAiModelsManager::new(dir.path().to_path_buf(), catalog.clone(), None)
        .with_provider_catalog();
    list(provider).await;
    assert_eq!(catalog.fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn vanilla_rejects_an_old_cache_from_a_manager_created_while_enhanced() {
    const CHILD: &str = "ALAB_TEST_LIVE_CACHE_MODE";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "vanilla_rejects_an_old_cache_from_a_manager_created_while_enhanced",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    codex_alab_core::activate();
    let home = tempfile::tempdir().unwrap();
    let cached = serde_json::json!({
        "fetched_at": (chrono::Utc::now() - chrono::Duration::minutes(10)).to_rfc3339(),
        "etag": null,
        "client_version": codex_models_manager::client_version_to_whole(),
        "identity": IDENTITY,
        "models": [],
    });
    fs::write(
        home.path().join("models_cache.alab.json"),
        cached.to_string(),
    )
    .unwrap();
    let catalog = Arc::new(Catalog::default());
    let manager = OpenAiModelsManager::new(home.path().to_path_buf(), catalog.clone(), None);
    codex_alab_core::set_enhanced(false);
    list(manager).await;
    assert_eq!(catalog.fetches.load(Ordering::SeqCst), 1);
}
