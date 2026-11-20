// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::AgentInput;
use crate::AgentMessage;
use crate::AgentTarget;
use crate::MessageDeliveryMode;
use crate::SendRequest;
use crate::SpawnAgentOptions;
use crate::SpawnRequest;
use crate::ThreadEvictionOutcome;
use crate::ThreadManager;
use crate::config::ConfigBuilder;
use crate::config::ConfigOverrides;
use crate::thread_manager::StartThreadOptions;
use crate::tools::handlers::multi_agents_common::thread_spawn_source;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_models_manager::manager::ModelCatalogSettings;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::SessionSource;
use core_test_support::responses;
use std::sync::Arc;
use std::time::Duration;

const MODEL: &str = "kimi-for-coding-highspeed";
const CHILD: &str = "detached child";
const FOLLOWUP: &str = "original worker followup";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn evicted_v2_child_uses_packaged_catalog_on_first_followup() -> anyhow::Result<()> {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::evicted_v2_child_uses_packaged_catalog_on_first_followup"
    )) {
        codex_alab_core::activate();
        tokio::time::timeout(Duration::from_secs(35), check_followup("kimi", false)).await??;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn evicted_v2_child_uses_administrator_catalog_on_first_followup() -> anyhow::Result<()> {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::evicted_v2_child_uses_administrator_catalog_on_first_followup"
    )) {
        codex_alab_core::activate();
        tokio::time::timeout(Duration::from_secs(35), check_followup("kimi", true)).await??;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn evicted_v2_child_discards_the_senders_packaged_catalog_on_first_followup()
-> anyhow::Result<()> {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::evicted_v2_child_discards_the_senders_packaged_catalog_on_first_followup"
    )) {
        codex_alab_core::activate();
        tokio::time::timeout(Duration::from_secs(35), check_followup("alternate", false)).await??;
    }
    Ok(())
}

async fn check_followup(worker_provider: &str, explicit_catalog: bool) -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let sender_endpoint = responses::start_mock_server().await;
    let child_endpoint = responses::start_mock_server().await;
    let sender_provider = if worker_provider == "kimi" {
        "alternate"
    } else {
        "kimi"
    };
    let mut config_toml = format!(
        r#"model_provider = "{sender_provider}"
model = "k3-256k"
model_reasoning_effort = "high"
features.multi_agent_v2 = true
[model_providers.alternate]
name = "Alternate provider"
wire_api = "responses"
[model_providers.kimi]
name = "Kimi Code"
wire_api = "responses"
"#
    );
    if explicit_catalog {
        let mut catalog = codex_alab_kimi::model_catalog(true, "kimi", None)?.expect("Kimi models");
        let levels = catalog.models[0].supported_reasoning_levels.clone();
        let fast = catalog
            .models
            .iter_mut()
            .find(|row| row.slug == MODEL)
            .unwrap();
        fast.supported_reasoning_levels = levels;
        fast.default_reasoning_level = Some(ReasoningEffort::High);
        std::fs::write(
            home.path().join("admin.json"),
            serde_json::to_vec(&catalog)?,
        )?;
        config_toml.insert_str(0, "model_catalog_json = \"admin.json\"\n");
    }
    std::fs::write(home.path().join("config.toml"), config_toml)?;
    let mut sender = ConfigBuilder::without_managed_config_for_tests()
        .codex_home(home.path().to_owned())
        .harness_overrides(ConfigOverrides {
            cwd: Some(home.path().to_owned()),
            ..Default::default()
        })
        .build()
        .await?;
    // The caller and child endpoints and credentials are independently observable.
    for (id, endpoint, token) in [
        (sender_provider, &sender_endpoint, "sender-fixture"),
        (worker_provider, &child_endpoint, "worker-fixture"),
    ] {
        let provider = sender.model_providers.get_mut(id).unwrap();
        provider.base_url = Some(format!("{}/v1", endpoint.uri()));
        provider.env_key = None;
        provider.experimental_bearer_token = Some(token.into());
        provider.supports_websockets = false;
    }
    sender.model_provider = sender.model_providers[sender_provider].clone();
    sender.alab_select_provider_catalog(sender_provider)?;
    assert_eq!(
        sender.alab_packaged_kimi_catalog,
        sender_provider == "kimi" && !explicit_catalog
    );
    let mut child_config = sender.clone();
    child_config.model_provider_id = worker_provider.to_owned();
    child_config.model_provider = child_config.model_providers[worker_provider].clone();
    child_config.model = Some(MODEL.into());
    child_config.alab_select_provider_catalog(worker_provider)?;
    let packaged = worker_provider == "kimi" && !explicit_catalog;
    assert_eq!(child_config.alab_packaged_kimi_catalog, packaged);
    let auth = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("local-fixture"));
    let db = crate::init_state_db(&sender).await;
    let manager = ThreadManager::new(
        &sender,
        crate::build_startup_models(
            &sender,
            auth,
            uuid::Uuid::new_v4().to_string(),
            Arc::new(ModelCatalogSettings::disk(sender.codex_home.to_path_buf())),
        ),
        Default::default(),
        SessionSource::Exec,
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        codex_extension_api::empty_extension_registry(),
        Arc::new(crate::test_support::EmptyUserInstructionsProvider),
        None,
        crate::passthrough_image_store(),
        crate::ThreadStoreBuilder::from_config(&sender, db.clone()).build(),
        crate::thread_manager::local_agent_graph_store_from_state_db(db.as_ref()),
        None,
        None,
    );
    let root = manager
        .start_thread(StartThreadOptions::new(sender.clone()))
        .await?;
    let root_id = root.thread_id;
    let control = &root.thread.session.services.agent_control;
    let input = |text: &str| AgentInput::Message {
        message: AgentMessage::Plaintext(text.to_owned()),
        mode: MessageDeliveryMode::TriggerTurn,
    };
    let reply = |label: &str| {
        responses::sse(vec![
            responses::ev_response_created(label),
            responses::ev_assistant_message(label, label),
            responses::ev_completed(label),
        ])
    };
    let initial = responses::mount_sse_once(&child_endpoint, reply("worker finished")).await;
    let (worker, _) = control
        .spawn(SpawnRequest {
            caller: root_id,
            config: child_config,
            source: thread_spawn_source(
                root_id,
                root.thread.session_source(),
                1,
                None,
                Some("worker".into()),
            )
            .unwrap(),
            input: input(CHILD),
            options: SpawnAgentOptions {
                parent_thread_id: Some(root_id),
                ..Default::default()
            },
        })
        .await?;
    let first = manager.get_thread(worker.thread_id).await?;
    let mut status = first.subscribe_status();
    status
        .wait_for(|status| matches!(status, AgentStatus::Completed(_)))
        .await?;
    let first_request = initial.single_request();
    assert!(
        first_request
            .input()
            .iter()
            .any(|item| item.to_string().contains(CHILD))
    );
    let expected = (!packaged).then_some("high");
    assert_eq!(
        first_request
            .body_json()
            .pointer("/reasoning/effort")
            .and_then(serde_json::Value::as_str),
        expected
    );
    assert_eq!(
        manager.try_evict_v2_thread(Arc::clone(&first)).await?,
        ThreadEvictionOutcome::Evicted
    );
    assert!(manager.get_thread(worker.thread_id).await.is_err());

    let resumed = responses::mount_sse_once(&child_endpoint, reply("followup finished")).await;
    let receipt = control
        .send(SendRequest {
            caller: root_id,
            target: AgentTarget::Id(worker.thread_id),
            resume_config: sender,
            input: input(FOLLOWUP),
            start_options: Default::default(),
        })
        .await?;
    assert_eq!(receipt.thread_id, worker.thread_id);
    let reloaded = manager.get_thread(worker.thread_id).await?;
    assert!(!Arc::ptr_eq(&first, &reloaded));
    let mut status = reloaded.subscribe_status();
    status
        .wait_for(|status| matches!(status, AgentStatus::Completed(_)))
        .await?;
    let request = resumed.single_request();
    let body = request.body_json();
    assert_eq!(
        request.header("thread-id"),
        Some(worker.thread_id.to_string())
    );
    assert_eq!(
        request.header("authorization").as_deref(),
        Some("Bearer worker-fixture")
    );
    assert_eq!(body["model"], MODEL);
    assert!(
        request
            .input()
            .iter()
            .any(|item| item.to_string().contains(FOLLOWUP))
    );
    assert_eq!(
        body.pointer("/reasoning/effort")
            .and_then(serde_json::Value::as_str),
        expected,
        "explicit catalog: {explicit_catalog}"
    );
    assert_eq!(reloaded.config().await.alab_packaged_kimi_catalog, packaged);
    assert!(
        sender_endpoint
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| {
                r.headers
                    .get("thread-id")
                    .and_then(|header| header.to_str().ok())
                    != Some(worker.thread_id.to_string().as_str())
            })
    );
    reloaded.shutdown_and_wait().await?;
    root.thread.shutdown_and_wait().await?;
    Ok(())
}
