// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_models_manager::ModelCatalogOriginator;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use pretty_assertions::assert_eq;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use core_test_support::alab_test_process as process;

#[derive(Debug)]
struct TabEndpoint {
    catalog: Mutex<Vec<ModelInfo>>,
    fetches: AtomicUsize,
    work: Option<Arc<TabEndpoint>>,
}

impl ModelsEndpointClient for TabEndpoint {
    fn for_originator(
        &self,
        originator: ModelCatalogOriginator,
    ) -> Option<Arc<dyn ModelsEndpointClient>> {
        if originator != ModelCatalogOriginator::Work {
            return None;
        }
        Some(self.work.as_ref()?.clone())
    }

    fn identity(&self) -> Option<String> {
        Some(IDENTITY.into())
    }
    fn has_command_auth(&self) -> bool {
        false
    }
    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(std::future::ready(true))
    }
    fn list_models<'a>(
        &'a self,
        _: &'a str,
        _: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<ModelsEndpointResponse>> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        let models = self.catalog.lock().unwrap().clone();
        Box::pin(std::future::ready(Ok(ModelsEndpointResponse {
            etag: models.last().map(|model| model.slug.clone()),
            models,
            identity: IDENTITY.into(),
        })))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    Follow,
    Pinned,
    FollowWithOlderProvider,
}

#[derive(Clone, Copy, PartialEq)]
enum Confirmation {
    ModelList,
    NextTurn,
    WorkNextTurn,
    IdleAfterPolicyRecovery,
}

async fn shared_cache_check(
    warm_launch: bool,
    offline_first: bool,
    selection: Selection,
    confirmation: Confirmation,
) {
    let following = selection != Selection::Pinned;
    let promoted = selection == Selection::Follow;
    codex_alab_core::activate();
    let home = tempfile::tempdir().unwrap();
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_owned())
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .harness_overrides(codex_core::config::ConfigOverrides {
            cwd: Some(home.path().to_owned()),
            ..Default::default()
        })
        .build()
        .await
        .unwrap();
    let old = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .find(|model| model.slug == "gpt-6.1-sol")
        .unwrap();
    let advertised_wire_effort = old.multi_agent_reasoning_effort.clone().unwrap();
    let mut new = old.clone();
    new.slug = "gpt-6.2-sol".into();
    let server = wiremock::MockServer::start().await;
    config.model_provider.base_url = Some(format!("{}/backend-api/codex", server.uri()));
    config.model_provider.supports_websockets = false;
    let auth = codex_login::AuthManager::from_auth_for_testing(
        codex_login::CodexAuth::create_dummy_chatgpt_auth_for_testing(),
    );
    let work_turn = confirmation == Confirmation::WorkNextTurn;
    let service = ModelCatalogOriginator::Work.as_str();
    let tab = || {
        let endpoint = |work| {
            Arc::new(TabEndpoint {
                catalog: Mutex::new(vec![old.clone()]),
                fetches: AtomicUsize::new(0),
                work,
            })
        };
        let work = work_turn.then(|| endpoint(None));
        let default = endpoint(work.clone());
        let base: SharedModelsManager = Arc::new(OpenAiModelsManager::new(
            home.path().to_owned(),
            default.clone(),
            Some(auth.clone()),
        ));
        let manager = if work_turn {
            base.for_originator(service).expect("real Work catalog")
        } else {
            base.clone()
        };
        (work.unwrap_or(default), base, manager)
    };
    let (publisher_endpoint, publisher_base, publisher) = tab();
    let (reader_endpoint, reader_base, reader) = tab();
    let http = config.http_client_factory();
    let default_cache = home.path().join(codex_alab_core::home::MODELS_CACHE_FILE);
    let untouched = if work_turn {
        publisher_base
            .raw_model_catalog(RefreshStrategy::Online, http.clone())
            .await;
        Some(std::fs::read(&default_cache).unwrap())
    } else {
        None
    };
    publisher
        .raw_model_catalog(RefreshStrategy::Online, http.clone())
        .await;
    let initial_strategy = if warm_launch {
        RefreshStrategy::OnlineIfUncached
    } else {
        RefreshStrategy::Online
    };
    reader
        .raw_model_catalog(initial_strategy, http.clone())
        .await;
    let initial_requests = usize::from(!warm_launch);
    assert_eq!(
        reader_endpoint.fetches.load(Ordering::SeqCst),
        initial_requests
    );
    config.model = Some(old.slug.clone());
    config.alab_saved_model_can_follow = following.then(|| old.slug.clone());
    config.model_reasoning_effort = Some(ReasoningEffort::Ultra);
    let threads = super::super::tests::test_thread_manager(&config, Some(reader_base), auth);
    let mut options = codex_core::StartThreadOptions::new(config.clone());
    options.metrics_service_name = work_turn.then(|| service.to_string());
    let thread = threads.start_thread(options).await.unwrap().thread;
    assert_eq!(thread.config_snapshot().await.model, old.slug);
    let requirements = home.path().join("requirements.toml");
    let mut loader = LoaderOverrides::without_managed_config_for_tests();
    loader.system_requirements_path = Some(requirements.clone());
    let config_manager = crate::config_manager::ConfigManager::new_for_tests(
        home.path().to_owned(),
        Vec::new(),
        loader,
        CloudConfigBundleLoader::default(),
    );
    let catalog = ModelCatalog::new(
        config_manager,
        Some(config.cwd.to_path_buf()),
        Arc::new(config),
        threads,
    );
    let mut updates = reader.model_catalog_updates().unwrap();
    let before = reader_endpoint.fetches.load(Ordering::SeqCst);
    if work_turn {
        let work_cache = default_cache.with_extension(format!("{service}.json"));
        assert_eq!(
            thread.alab_peer_model_cache_file(),
            Some(work_cache.as_path())
        );
        // Prime the turn's revision before another process publishes. An initial
        // observation of the unchanged default file cannot satisfy the next turn.
        check_next_request(&thread, &server, &old.slug, &advertised_wire_effort).await;
    } else {
        catalog
            .list_models(RefreshStrategy::OnlineIfUncached)
            .await
            .unwrap();
    }
    assert_eq!(
        reader_endpoint.fetches.load(Ordering::SeqCst),
        before,
        "unchanged warm catalog"
    );
    assert!(!updates.has_changed().unwrap());
    *publisher_endpoint.catalog.lock().unwrap() = vec![old.clone(), new.clone()];
    if selection != Selection::FollowWithOlderProvider {
        *reader_endpoint.catalog.lock().unwrap() = vec![old.clone(), new.clone()];
    }
    publisher
        .raw_model_catalog(RefreshStrategy::Online, http.clone())
        .await;
    if offline_first {
        let disk = reader
            .raw_model_catalog(RefreshStrategy::Offline, http)
            .await;
        assert!(disk.models.iter().any(|model| model.slug == new.slug));
        assert!(
            !updates.has_changed().unwrap(),
            "reading another tab's file is not a provider observation"
        );
    }
    let expected_model = if promoted { &new.slug } else { &old.slug };
    match confirmation {
        Confirmation::ModelList => {
            catalog
                .list_models(RefreshStrategy::OnlineIfUncached)
                .await
                .unwrap();
        }
        Confirmation::NextTurn | Confirmation::WorkNextTurn => {
            check_next_request(&thread, &server, expected_model, &advertised_wire_effort).await;
        }
        Confirmation::IdleAfterPolicyRecovery => {
            let mut revisions = std::collections::HashMap::new();
            std::fs::write(&requirements, "model_provider = 'other'").unwrap();
            catalog.alab_confirm_peer_catalogs(&mut revisions).await;
            assert_eq!(reader_endpoint.fetches.load(Ordering::SeqCst), before);
            assert!(!updates.has_changed().unwrap());

            std::fs::remove_file(&requirements).unwrap();
            catalog.alab_confirm_peer_catalogs(&mut revisions).await;
            assert_eq!(reader_endpoint.fetches.load(Ordering::SeqCst), before);
            // Only the policy changed. The peer never rewrites the catalog.
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(31)).await;
            tokio::time::resume();
            catalog.alab_confirm_peer_catalogs(&mut revisions).await;
        }
    }
    assert_eq!(
        reader_endpoint.fetches.load(Ordering::SeqCst),
        before + usize::from(following),
        "a live follower must confirm the candidate using its own provider"
    );
    if following {
        assert!(updates.has_changed().unwrap());
        updates.borrow_and_update();
        assert_eq!(
            reader
                .refreshed_model_catalog()
                .unwrap()
                .iter()
                .any(|model| model.slug == new.slug),
            promoted,
            "only B's own provider response is authoritative"
        );
    } else {
        assert!(!updates.has_changed().unwrap());
    }
    if promoted
        && matches!(
            confirmation,
            Confirmation::ModelList | Confirmation::IdleAfterPolicyRecovery
        )
    {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let EventMsg::ThreadSettingsApplied(event) =
                    thread.next_event().await.unwrap().msg
                    && event.thread_settings.model == new.slug
                {
                    break;
                }
            }
        })
        .await
        .expect("following thread did not receive its promoted settings");
    }
    let applied = thread.config_snapshot().await;
    assert_eq!(&applied.model, expected_model);
    assert_eq!(applied.reasoning_effort, Some(ReasoningEffort::Ultra));
    if !work_turn {
        catalog
            .list_models(RefreshStrategy::OnlineIfUncached)
            .await
            .unwrap();
    }
    assert_eq!(
        reader_endpoint.fetches.load(Ordering::SeqCst),
        before + usize::from(following)
    );
    assert!(
        !updates.has_changed().unwrap(),
        "unchanged catalog must not wake the thread twice"
    );
    check_next_request(&thread, &server, expected_model, &advertised_wire_effort).await;
    assert_eq!(
        reader_endpoint.fetches.load(Ordering::SeqCst),
        before + usize::from(following)
    );
    if let Some(untouched) = untouched {
        assert_eq!(std::fs::read(default_cache).unwrap(), untouched);
    }
    thread.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn idle_model_following_recovers_when_managed_provider_requirements_allow_it_again() {
    if !process::child(concat!(
        module_path!(),
        "::idle_model_following_recovers_when_managed_provider_requirements_allow_it_again"
    )) {
        return;
    }
    shared_cache_check(
        true,
        false,
        Selection::Follow,
        Confirmation::IdleAfterPolicyRecovery,
    )
    .await;
}

async fn check_next_request(
    thread: &codex_core::CodexThread,
    server: &wiremock::MockServer,
    expected_model: &str,
    wire_effort: &ReasoningEffort,
) {
    use core_test_support::responses;
    let request = responses::mount_sse_once(
        server,
        responses::sse(vec![
            responses::ev_response_created("after-catalog-poll"),
            responses::ev_assistant_message("answer", "Done."),
            responses::ev_completed("after-catalog-poll"),
        ]),
    )
    .await;
    thread
        .start_or_steer_turn(codex_protocol::turn_input::TurnInputRequest::user_input(
            vec![codex_protocol::user_input::UserInput::Text {
                text: "Report the current model.".into(),
                text_elements: Vec::new(),
            }],
        ))
        .await
        .unwrap();
    core_test_support::wait_for_event_with_timeout(
        thread,
        |event| matches!(event, EventMsg::TurnComplete(_)),
        Duration::from_secs(15),
    )
    .await;
    let body = request.single_request().body_json();
    assert_eq!(body["model"], expected_model);
    assert_eq!(body["reasoning"]["effort"], wire_effort.as_str());
}

#[tokio::test]
async fn following_thread_confirms_a_peer_catalog_after_a_warm_launch() {
    if process::child(concat!(
        module_path!(),
        "::following_thread_confirms_a_peer_catalog_after_a_warm_launch"
    )) {
        shared_cache_check(true, true, Selection::Follow, Confirmation::ModelList).await;
    }
}

#[tokio::test]
async fn following_thread_with_an_older_provider_observation_confirms_a_peer_catalog() {
    if process::child(concat!(
        module_path!(),
        "::following_thread_with_an_older_provider_observation_confirms_a_peer_catalog"
    )) {
        shared_cache_check(false, false, Selection::Follow, Confirmation::ModelList).await;
    }
}

#[tokio::test]
async fn exact_pin_keeps_a_peer_catalog_warm_without_confirming_a_successor() {
    if process::child(concat!(
        module_path!(),
        "::exact_pin_keeps_a_peer_catalog_warm_without_confirming_a_successor"
    )) {
        shared_cache_check(true, true, Selection::Pinned, Confirmation::ModelList).await;
    }
}

#[tokio::test]
async fn peer_successor_requires_the_following_threads_own_provider_to_confirm_it() {
    if process::child(concat!(
        module_path!(),
        "::peer_successor_requires_the_following_threads_own_provider_to_confirm_it"
    )) {
        shared_cache_check(
            true,
            true,
            Selection::FollowWithOlderProvider,
            Confirmation::ModelList,
        )
        .await;
    }
}

#[tokio::test]
async fn next_turn_confirms_peer_models_with_its_own_provider_and_preserves_pins() {
    if !process::child(concat!(
        module_path!(),
        "::next_turn_confirms_peer_models_with_its_own_provider_and_preserves_pins"
    )) {
        return;
    }
    for selection in [
        Selection::Follow,
        Selection::FollowWithOlderProvider,
        Selection::Pinned,
    ] {
        shared_cache_check(true, false, selection, Confirmation::NextTurn).await;
    }
}

#[tokio::test]
async fn work_next_turn_confirms_only_its_scoped_peer_release_and_preserves_pins() {
    if !process::child(concat!(
        module_path!(),
        "::work_next_turn_confirms_only_its_scoped_peer_release_and_preserves_pins"
    )) {
        return;
    }
    for selection in [
        Selection::Follow,
        Selection::FollowWithOlderProvider,
        Selection::Pinned,
    ] {
        shared_cache_check(true, false, selection, Confirmation::WorkNextTurn).await;
    }
}
