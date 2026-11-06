// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(autopilot) glue tests, compiled inside codex-app-server as
//! `request_processors::thread_goal_processor::alab_goal_person_stop_tests` (one `#[path]` line
//! at the end of `app-server/src/request_processors/thread_goal_processor.rs`): the one
//! production feed of `codex_alab_core::goal_stops`, the hook in `thread_goal_set_inner` that
//! notes a person's goal set as it passes (review of the forever-persist lane, F3: every test of
//! item c called `goal_stops::person_set` itself, so a mutant of the hook survived them all).
//! The request goes through the in-process app-server, the transport codex's own TUI uses, to the
//! real request processor; the hook writes this process's shared state, which the test reads.

use std::sync::Arc;

use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadGoalMutationOrigin;
use codex_app_server_protocol::ThreadGoalSetParams;
use codex_app_server_protocol::ThreadGoalSetResponse;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_core::config::ConfigBuilder;
use codex_exec_server::EnvironmentManager;
use codex_feedback::CodexFeedback;
use codex_protocol::protocol::SessionSource;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use toml::Value as TomlValue;

use crate::in_process::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
use crate::in_process::InProcessClientHandle;
use crate::in_process::InProcessStartArgs;

/// A fixture that needs no shell, plugin sync or provider transport (the in-process tests'
/// own), with goals on.
fn overrides() -> Vec<(String, TomlValue)> {
    vec![
        (
            "features.shell_snapshot".to_string(),
            TomlValue::Boolean(false),
        ),
        ("features.plugins".to_string(), TomlValue::Boolean(false)),
        ("features.goals".to_string(), TomlValue::Boolean(true)),
        (
            "model_provider".to_string(),
            TomlValue::String("in_process_test".to_string()),
        ),
        (
            "model_providers.in_process_test".to_string(),
            TomlValue::Table(toml::toml! {
                name = "In-process test"
                base_url = "http://127.0.0.1:0/v1"
                supports_websockets = false
                requires_openai_auth = false
            }),
        ),
    ]
}

/// The in-process app-server over the scratch home `codex_home`.
async fn client(codex_home: &TempDir) -> InProcessClientHandle {
    let config = Arc::new(
        ConfigBuilder::default()
            .codex_home(codex_home.path().to_path_buf())
            .cli_overrides(overrides())
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .build()
            .await
            .expect("the fixture's config"),
    );
    let state_db = codex_rollout::state_db::try_init(config.as_ref())
        .await
        .expect("the fixture's state db");
    crate::in_process::start(InProcessStartArgs {
        arg0_paths: Arg0DispatchPaths::default(),
        config,
        cli_overrides: overrides(),
        loader_overrides: LoaderOverrides::without_managed_config_for_tests(),
        strict_config: false,
        cloud_config_bundle: CloudConfigBundleLoader::default(),
        embedded_network_policy: Default::default(),
        thread_config_loader: Arc::new(codex_config::NoopThreadConfigLoader),
        feedback: CodexFeedback::new(),
        log_db: None,
        state_db: Some(state_db),
        environment_manager: Arc::new(EnvironmentManager::default_for_tests()),
        config_warnings: Vec::new(),
        session_source: SessionSource::Cli,
        enable_codex_api_key_env: false,
        initialize: InitializeParams {
            client_info: ClientInfo {
                name: "codex-alab-goal-stop-test".to_string(),
                title: None,
                version: "0.0.0".to_string(),
            },
            capabilities: None,
        },
        channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    })
    .await
    .expect("the in-process app-server")
}

/// `thread/goal/set` request `id` on thread `thread_id` through `client`: the goal's status
/// after it.
async fn goal_set(
    client: &InProcessClientHandle,
    id: i64,
    thread_id: &str,
    origin: Option<ThreadGoalMutationOrigin>,
    objective: Option<&str>,
    status: Option<ThreadGoalStatus>,
) -> ThreadGoalStatus {
    let response = client
        .request(ClientRequest::ThreadGoalSet {
            request_id: RequestId::Integer(id),
            params: ThreadGoalSetParams {
                thread_id: thread_id.to_string(),
                origin,
                objective: objective.map(str::to_owned),
                status,
                token_budget: None,
            },
        })
        .await
        .expect("request transport")
        .unwrap_or_else(|err| panic!("thread/goal/set {origin:?} {status:?}: {err:?}"));
    serde_json::from_value::<ThreadGoalSetResponse>(response)
        .expect("thread/goal/set response")
        .goal
        .status
}

/// A PERSON'S GOAL SET IS NOTED AS IT PASSES THE APP-SERVER (review of the forever-persist lane,
/// F3): a `thread/goal/set` with a `user` origin that leaves the goal blocked or paused counts a
/// person's stop of that thread's goal, which the autopilot reads before it binds or resumes a
/// wall's goal; so does a person's edit of the objective of a goal it leaves paused (the stop is
/// read from the status the set leaves). NEGATIVE CONTROLS: the same stops made automatically,
/// or with no origin (missing provenance is no person's), count none. A person's set that leaves
/// the goal active is not made here (an active goal may start a continuation turn against the
/// fixture's unreachable provider): the hook reads a stop from the resulting status, and an
/// inverted reading fails each person's case above.
#[tokio::test]
async fn a_persons_goal_set_is_noted_as_it_passes_the_app_server() {
    let codex_home = TempDir::new().expect("scratch codex home");
    let client = client(&codex_home).await;
    let started = client
        .request(ClientRequest::ThreadStart {
            request_id: RequestId::Integer(1),
            params: ThreadStartParams::default(),
        })
        .await
        .expect("request transport")
        .expect("thread/start");
    let thread_id = serde_json::from_value::<ThreadStartResponse>(started)
        .expect("thread/start response")
        .thread
        .id;
    let stops = || codex_alab_core::goal_stops::person_stops(&thread_id);
    let (user, automatic) = (
        Some(ThreadGoalMutationOrigin::User),
        Some(ThreadGoalMutationOrigin::Automatic),
    );
    let blocked = Some(ThreadGoalStatus::Blocked);
    let paused = Some(ThreadGoalStatus::Paused);
    assert_eq!(stops(), 0);
    // NEGATIVE CONTROLS: an automatic set, and one with no origin, count none.
    let objective = Some("finish the original task");
    assert_eq!(
        goal_set(&client, 2, &thread_id, automatic, objective, blocked).await,
        ThreadGoalStatus::Blocked
    );
    assert_eq!(stops(), 0, "an automatic set is no person's");
    assert_eq!(
        goal_set(&client, 3, &thread_id, None, None, paused).await,
        ThreadGoalStatus::Paused
    );
    assert_eq!(stops(), 0, "a set with no origin is no person's");
    // A person's block and pause each count one.
    assert_eq!(
        goal_set(&client, 4, &thread_id, user, None, blocked).await,
        ThreadGoalStatus::Blocked
    );
    assert_eq!(stops(), 1, "a person's block");
    assert_eq!(
        goal_set(&client, 5, &thread_id, user, None, paused).await,
        ThreadGoalStatus::Paused
    );
    assert_eq!(stops(), 2, "a person's pause");
    // A person's edit of the objective of a goal that stays paused: one more.
    let edited = Some("finish the edited task");
    assert_eq!(
        goal_set(&client, 6, &thread_id, user, edited, None).await,
        ThreadGoalStatus::Paused
    );
    assert_eq!(stops(), 3, "a person's edit of a stopped goal");
    client.shutdown().await.expect("in-process shutdown");
}
