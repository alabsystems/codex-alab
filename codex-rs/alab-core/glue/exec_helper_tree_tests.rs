// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual model-spawned helpers cross the production native tree recovery API.
use super::*;
use base64::Engine as _;
use codex_features::Feature;
use codex_login::CodexAuth;
use codex_protocol::openai_models::ModelAccessPrograms;
use codex_protocol::turn_input::CyberAccessProgram;
use serde_json::json;

#[test]
fn native_helper_tree_retains_an_implicit_choice_under_a_fresh_owner() -> anyhow::Result<()> {
    run_test_with_large_stack("native-helper-tree-same", || helper_tree(false, false))
}

#[test]
fn native_helper_tree_refuses_another_accounts_implicit_choice() -> anyhow::Result<()> {
    run_test_with_large_stack("native-helper-tree-other", || helper_tree(true, false))
}

#[test]
fn native_helper_tree_preserves_an_explicit_override_across_accounts() -> anyhow::Result<()> {
    run_test_with_large_stack("native-helper-tree-explicit", || helper_tree(true, true))
}

fn helper_auth(account: &str) -> CodexAuth {
    let claims = json!({"https://api.openai.com/auth": {
        "chatgpt_user_id": "helper-tree-user", "chatgpt_account_id": account
    }});
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
    CodexAuth::from_external_chatgpt_tokens(&format!("header.{payload}.signature"), account, None)
        .unwrap()
}

fn configure_helper(config: &mut Config) {
    for feature in [
        Feature::Collab,
        Feature::MultiAgentV2,
        Feature::DaybreakProgramPropagation,
    ] {
        config.features.enable(feature).unwrap();
    }
    config
        .features
        .disable(Feature::ResponsesWebsockets)
        .unwrap();
    config
        .features
        .disable(Feature::ResponsesWebsocketsV2)
        .unwrap();
    config.model_provider.supports_websockets = false;
}

fn request_json(body: &[u8]) -> serde_json::Value {
    let bytes = codex_alab_core::test_support::recorded_request_body(body, 8 * 1024 * 1024)
        .expect("decode actual recorded request");
    serde_json::from_slice(&bytes).expect("recorded request is JSON")
}

fn contains_helper_input(body: &serde_json::Value) -> bool {
    body["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            matches!(item["type"].as_str(), Some("message" | "agent_message"))
                && item.to_string().contains("native-helper-original-input")
        })
    })
}

async fn helper_tree(other_account: bool, explicit: bool) -> anyhow::Result<()> {
    let (_root_release, root_gate) = oneshot::channel();
    let (_child_release, child_gate) = oneshot::channel();
    let mut chunks = vec![
        vec![StreamingSseChunk { gate: None, body: responses::sse(vec![
            responses::ev_function_call_with_namespace("spawn-native-helper", "collaboration", "spawn_agent",
                &json!({"task_name":"worker", "message":"native-helper-original-input", "fork_turns":"none"}).to_string()),
            responses::ev_completed("source-spawn"),
        ]) }],
        vec![StreamingSseChunk { gate: Some(root_gate), body: responses::sse_completed("source-open-one") }],
        vec![StreamingSseChunk { gate: Some(child_gate), body: responses::sse_completed("source-open-two") }],
    ];
    for index in 0..8 {
        chunks.push(vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![
                responses::ev_assistant_message(&format!("restored-message-{index}"), "Done."),
                responses::ev_completed(&format!("restored-{index}")),
            ]),
        }]);
    }
    let (source, _) = start_streaming_sse_server(chunks).await;
    let mock = responses::start_mock_server().await;
    let endpoint = format!("{}/v1", source.uri());
    let fixture = test_codex()
        .with_pre_build_hook(move |home| {
            // Native descendant reload reads the loader's layers, not an in-memory
            // provider mutation. Keep the built-in identity and its real input path.
            let mut catalog = codex_models_manager::bundled_models_response().unwrap();
            let model = catalog
                .models
                .iter_mut()
                .find(|model| model.slug == "gpt-5.5")
                .expect("fixture model exists in the bundled catalog");
            model.available_access_programs = Some(ModelAccessPrograms {
                cyber: vec![
                    CyberAccessProgram::Standard,
                    CyberAccessProgram::DaybreakBlue,
                ],
            });
            model.use_responses_lite = false;
            let catalog_path = home.join("helper-models.json");
            std::fs::write(&catalog_path, serde_json::to_vec(&catalog).unwrap()).unwrap();
            std::fs::write(
                home.join("config.toml"),
                format!(
                    "openai_base_url = {}\nmodel_catalog_json = {}\n",
                    serde_json::to_string(&endpoint).unwrap(),
                    serde_json::to_string(&catalog_path).unwrap()
                ),
            )
            .unwrap();
        })
        .with_auth(helper_auth("source-account"))
        .with_model("gpt-5.5")
        .with_config(configure_helper)
        .build_with_auto_env(&mock)
        .await?;
    let mut options = fixture.start_thread_options().await;
    options.config.model_provider.base_url = Some(format!("{}/v1", source.uri()));
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let config = options.config.clone();
    let environments = options.environments.clone();
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default())?;
    let root = fixture.thread_manager.start_thread(options).await?.thread;
    root.ensure_rollout_materialized().await;
    root.update_thread_metadata(
        codex_thread_store::ThreadMetadataPatch {
            daybreak_enabled: Some(true),
            ..Default::default()
        },
        false,
    )
    .await?;
    let mut created = fixture.thread_manager.subscribe_thread_created();
    let mut request = TurnInputRequest::user_input(vec![UserInput::Text {
        text: "native root request".into(),
        text_elements: Vec::new(),
    }]);
    if explicit {
        request.start.cyber_access_program = Some(CyberAccessProgram::DaybreakBlue);
    }
    let TurnInputSubmission::Started {
        turn_id: root_turn, ..
    } = root.start_or_steer_turn(request).await?
    else {
        anyhow::bail!("root input did not start")
    };
    let child_id = tokio::time::timeout(Duration::from_secs(15), created.recv()).await??;
    let child = fixture.thread_manager.get_thread(child_id).await?;
    tokio::time::timeout(Duration::from_secs(15), source.wait_for_request_count(3)).await?;
    let requests = source.requests().await;
    let original = requests
        .iter()
        .map(|body| request_json(body))
        .find(contains_helper_input)
        .expect("actual native helper requested its accepted input");
    assert_eq!(
        original["access_programs"]["cyber"],
        json!(CyberAccessProgram::DaybreakBlue)
    );
    root.update_thread_metadata(
        codex_thread_store::ThreadMetadataPatch {
            daybreak_enabled: Some(false),
            ..Default::default()
        },
        false,
    )
    .await?;
    let captured = super::super::super::super::State::default()
        .suspend(
            &fixture.thread_manager,
            &crate::turn_admission::TurnAdmission::default(),
            root.exec_thread_id(),
            &root_turn,
            &invocation,
        )
        .await
        .map_err(anyhow::Error::msg)?;
    let bytes = serde_json::to_vec(captured.tree())?;
    let snapshot: ExecTreeSnapshot = serde_json::from_slice(&bytes)?;
    snapshot
        .validate(root.exec_thread_id(), Some(&root_turn))
        .map_err(anyhow::Error::msg)?;
    let wire: serde_json::Value = serde_json::from_slice(&bytes)?;
    let child_wire = wire["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["thread"] == child_id.to_string())
        .unwrap();
    assert_eq!(child_wire["helper"]["role"], "helper");
    assert_eq!(child_wire["recovery"]["helper_choice"].is_null(), explicit);
    let child_turn = snapshot
        .threads()
        .iter()
        .find(|node| node.thread() == child_id)
        .unwrap()
        .recovery()
        .unwrap()
        .turn()
        .to_owned();
    let replacement = test_codex()
        .with_auth(helper_auth(if other_account {
            "destination-account"
        } else {
            "source-account"
        }))
        .with_model("gpt-5.5")
        .with_home(Arc::clone(&fixture.home))
        .with_thread_store(Arc::clone(&fixture.thread_store))
        .with_config(configure_helper)
        .build_with_auto_env(&mock)
        .await?;
    let stored = fixture
        .thread_store
        .load_latest_model_context(LoadThreadHistoryParams {
            thread_id: root.exec_thread_id(),
            include_archived: false,
        })
        .await?;
    let mut options = replacement.start_thread_options().await;
    options.config = config;
    options.environments = environments;
    options.initial_history = InitialHistory::Resumed(ResumedHistory {
        conversation_id: root.exec_thread_id(),
        history: Arc::new(stored.items),
        history_revision: stored.revision,
        rollout_path: None,
    });
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    snapshot.threads()[0].stage_helper_restore(&mut options.thread_extension_init);
    let mut inputs = snapshot
        .inputs()
        .clone()
        .prepare(root.exec_thread_id(), &root_turn)
        .map_err(anyhow::Error::msg)?;
    let options = inputs
        .prepare_start(options, root.exec_thread_id(), &root_turn)
        .map_err(anyhow::Error::msg)?;
    let restored_root = replacement
        .thread_manager
        .start_thread(options)
        .await?
        .thread;
    let recovery = ExecRecoveryTree::prepare(
        Arc::clone(&restored_root),
        snapshot,
        inputs,
        tokio::time::Instant::now() + Duration::from_secs(30),
        |_, _| Box::pin(async { Ok(()) }),
    )
    .await
    .map_err(anyhow::Error::msg)?;
    let restored_child = replacement.thread_manager.get_thread(child_id).await?;
    assert_eq!(
        source.requests().await.len(),
        3,
        "prepared tree has no request authority"
    );
    recovery.activate().await.map_err(anyhow::Error::msg)?;
    let refused = other_account && !explicit;
    let completed = tokio::time::timeout(
        Duration::from_secs(15),
        wait_for_event(&restored_child, |event| {
            matches!(event, EventMsg::Error(_) | EventMsg::TurnComplete(_))
        }),
    )
    .await?;
    if refused {
        assert!(
            matches!(&completed, EventMsg::Error(error) if error.message.contains("account")),
            "implicit foreign account must refuse: {completed:?}"
        );
    } else {
        assert!(
            matches!(&completed, EventMsg::TurnComplete(done) if done.turn_id == child_turn && done.error.is_none()),
            "restored original helper turn must complete successfully: {completed:?}"
        );
    }
    let requests = source.requests().await;
    let helper_requests = requests[3..]
        .iter()
        .map(|body| request_json(body))
        .filter(contains_helper_input)
        .collect::<Vec<_>>();
    assert_eq!(
        helper_requests.len(),
        usize::from(!refused),
        "old input is requested once or refused before delivery"
    );
    if let Some(request) = helper_requests.first() {
        assert_eq!(
            request["access_programs"]["cyber"],
            json!(CyberAccessProgram::DaybreakBlue)
        );
    }
    let history = replacement
        .thread_store
        .load_latest_turn_history(LoadThreadHistoryParams {
            thread_id: child_id,
            include_archived: false,
        })
        .await?;
    assert!(
        serde_json::to_string(&history.items)?.contains("native-helper-original-input"),
        "account refusal cannot erase accepted input"
    );
    restored_root.shutdown_and_wait().await?;
    root.shutdown_and_wait().await?;
    child.shutdown_and_wait().await?;
    source.shutdown().await;
    Ok(())
}
