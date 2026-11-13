// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use codex_alab_core::quota::HeadlessExec;
use codex_core::StartThreadOptions;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::config::ExecConfiguration;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::run_test_with_large_stack;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use std::time::Duration;

#[test]
fn upgrade_forest_bootstrap_has_one_release_after_the_last_native_admission() -> anyhow::Result<()>
{
    run_test_with_large_stack("forest-bootstrap", || restore(false, false, 0, 0))
}
#[test]
fn upgrade_forest_refusal_stops_every_root_without_model_work() -> anyhow::Result<()> {
    run_test_with_large_stack("forest-bootstrap-refused", || restore(true, false, 0, 0))
}
#[test]
fn upgrade_forest_activation_survives_a_cancelled_transport_waiter() -> anyhow::Result<()> {
    run_test_with_large_stack("forest-bootstrap-cancelled", || restore(false, true, 0, 0))
}

#[test]
fn upgrade_forest_restores_goal_accounting_before_native_continuation() -> anyhow::Result<()> {
    run_test_with_large_stack("forest-goal-bootstrap", || restore(false, false, 1, 0))
}
#[test]
fn upgrade_forest_goal_row_mismatch_refuses_every_model_request() -> anyhow::Result<()> {
    run_test_with_large_stack("forest-goal-mismatch", || restore(false, false, 2, 0))
}

#[test]
fn upgrade_forest_final_native_activation_refusal_keeps_all_model_work_stopped()
-> anyhow::Result<()> {
    run_test_with_large_stack("forest-native-activation-refused", || {
        restore(false, false, 3, 0)
    })
}

#[test]
fn upgrade_forest_reloads_each_root_before_starting_restored_work() -> anyhow::Result<()> {
    run_test_with_large_stack("forest-policy-reload", || restore(false, false, 0, 1))
}
#[test]
fn upgrade_forest_invalid_secondary_policy_starts_no_root() -> anyhow::Result<()> {
    run_test_with_large_stack("forest-invalid-secondary-policy", || {
        restore(false, false, 0, 2)
    })
}

async fn restore(
    refuse: bool,
    cancel_waiter: bool,
    goal_case: u8,
    reload: u8,
) -> anyhow::Result<()> {
    let mock = responses::start_mock_server().await;
    let source_requests =
        responses::mount_sse_once(&mock, responses::sse_completed("source-completed")).await;
    let fixture = test_codex()
        .with_model("gpt-5.6-sol")
        .with_config(move |config| {
            config.model_provider.requires_openai_auth = false;
            // Make the loopback provider an explicit captured preference rather
            // than a mutation of the built-in provider's runtime definition.
            config.model_provider_id = "forest-loopback".into();
            config.model_provider.include_internal_metadata = false;
            config.model_providers.insert(
                config.model_provider_id.clone(),
                config.model_provider.clone(),
            );
            if goal_case != 0 {
                config
                    .features
                    .enable(codex_features::Feature::Goals)
                    .unwrap();
            }
        })
        .build_with_auto_env(&mock)
        .await?;
    let primary = fixture.codex.clone();
    let primary_id = primary.exec_thread_id();
    primary
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "completed work before publication".into(),
            text_elements: Vec::new(),
        }]))
        .await?;
    tokio::time::timeout(
        Duration::from_secs(10),
        wait_for_event(&primary, |event| matches!(event, EventMsg::TurnComplete(_))),
    )
    .await?;
    let mut memory = fixture.config.clone();
    memory.ephemeral = true;
    if reload != 0 {
        memory.cwd = fixture.config.cwd.join("secondary-project");
        memory.workspace_roots = vec![memory.cwd.clone()];
        memory
            .permissions
            .set_workspace_roots(memory.workspace_roots.clone());
        std::fs::create_dir_all(memory.cwd.as_path())?;
        codex_core::config::set_project_trust_level(
            fixture.config.codex_home.as_path(),
            memory.cwd.as_path(),
            codex_protocol::config_types::TrustLevel::Trusted,
        )?;
    }
    let secondary_cwd = memory.cwd.clone();
    let secondary = fixture
        .thread_manager
        .start_thread(StartThreadOptions::new(memory.clone()))
        .await?
        .thread;
    let secondary_id = secondary.exec_thread_id();
    let mut child_options = StartThreadOptions::new(memory.clone());
    child_options.session_source = Some(SessionSource::Internal(
        InternalSessionSource::MemoryConsolidation,
    ));
    let child = fixture
        .thread_manager
        .spawn_internal_session(secondary_id, child_options)
        .await?
        .thread;
    let child_id = child.exec_thread_id();
    for member in [&primary, &secondary, &child] {
        assert!(
            member
                .thread_extension_data()
                .get::<HeadlessExec>()
                .is_none()
        );
    }
    let mail = codex_protocol::protocol::InterAgentCommunication::new(
        codex_protocol::AgentPath::root(),
        codex_protocol::AgentPath::root(),
        Vec::new(),
        "work waiting for complete forest activation".into(),
        false,
    );
    primary
        .submit(codex_protocol::protocol::Op::InterAgentCommunication {
            communication: mail,
            start_options: Default::default(),
        })
        .await?;
    let invocation = ExecConfiguration::capture(&fixture.config, &ConfigOverrides::default())?;
    let mut prepared = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match primary.prepare_upgrade_forest(&invocation).await {
                Ok(prepared) => break Ok::<_, anyhow::Error>(prepared),
                // TurnComplete precedes final source task cleanup. No new source
                // work is submitted while this bounded fixture wait runs.
                Err(error)
                    if error == "native admission is pending"
                        || error == "idle input sealing found an active or suspended turn" =>
                {
                    tokio::task::yield_now().await
                }
                Err(error) => break Err(anyhow::anyhow!(error)),
            }
        }
    })
    .await??;
    let expected = prepared
        .threads()
        .iter()
        .map(|member| member.exec_thread_id())
        .collect::<Vec<_>>();
    assert!(prepared.commit_if(|| true));
    let snapshot = prepared
        .into_committed()
        .map_err(anyhow::Error::msg)?
        .capture()
        .await
        .map_err(anyhow::Error::msg)?;
    // Exercise a retained auto-start input at the destination without racing a
    // source actor into a new turn. This is queue data, not a native grant.
    let mut encoded = serde_json::to_value(snapshot.as_ref())?;
    let mut changed = 0;
    for item in encoded["inputs"]["input"]["items"].as_array_mut().unwrap() {
        if item["kind"] == "Mail" {
            let mut communication: codex_protocol::protocol::InterAgentCommunication =
                serde_json::from_value(item["communication"].clone())?;
            communication.trigger_turn = true;
            item["communication"] = serde_json::to_value(communication)?;
            changed += 1;
        }
    }
    assert_eq!(changed, 1);
    let snapshot = Arc::new(serde_json::from_value::<UpgradeForestSnapshot>(encoded)?);
    // Native destination test data: source Core had no goal extension. Seed
    // its actual database after source shutdown, and use a real monotonic stamp
    // with a known carried interval. This is not full SDK source-capture proof.
    let db = fixture.codex.state_db().unwrap();
    let goal_state = if goal_case != 0 {
        let mut saved = db
            .thread_goals()
            .replace_thread_goal(
                primary_id,
                "carry accounting before any restored turn",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await?;
        saved.tokens_used = 93;
        saved.time_used_seconds = 11;
        db.thread_goals()
            .replace_thread_goal_snapshot(&saved)
            .await?;
        let baseline = codex_alab_core::goal_clock::Instant::now()
            .checked_sub(Duration::new(5, 123_456_789))
            .unwrap();
        let stamp =
            codex_alab_core::goal_clock::checkpoint(&baseline).map_err(anyhow::Error::msg)?;
        let mut expected_goal = saved;
        if goal_case == 2 {
            expected_goal.tokens_used += 1;
        }
        Some(Arc::new(
            codex_alab_autopilot::upgrade_goals::GoalForestState {
                forest: snapshot.clone(),
                goals: expected
                    .iter()
                    .map(|&id| codex_alab_autopilot::upgrade_goals::FinalGoalState {
                        thread: id,
                        goal: (id == primary_id).then(|| expected_goal.clone()),
                        clock: (id == primary_id).then_some(stamp),
                    })
                    .collect(),
            },
        ))
    } else {
        None
    };
    let destination = if goal_case == 1 {
        responses::mount_sse_sequence(
            &mock,
            vec![
                responses::sse(vec![
                    responses::ev_response_created("pause-restored-goal"),
                    responses::ev_function_call(
                        "pause-goal",
                        codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                        r#"{"status":"paused"}"#,
                    ),
                    responses::ev_completed("pause-restored-goal"),
                ]),
                responses::sse_completed("after-paused-goal"),
            ],
        )
        .await
    } else {
        responses::mount_sse_once(&mock, responses::sse_completed("after-forest-release")).await
    };
    let mut client = start_client(fixture.config.clone()).await?;
    let sender = client.sender();
    let cancel = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let pump_cancel = cancel.clone();
    let (events_tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let pump = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = pump_cancel.cancelled() => break,
                event = client.next_event() => match event { Some(event) => { let _ = events_tx.send(event); }, None => break },
            }
        }
        client.shutdown().await
    });
    let admissions = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let calls = admissions.clone();
    let (reached, reached_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let held = Arc::new(tokio::sync::Mutex::new(Some((reached, release_rx))));
    let policies = vec![
        (
            primary_id,
            FreshRootStartup::interactive(fixture.config.clone()),
        ),
        (secondary_id, FreshRootStartup::interactive(memory)),
    ];
    let admit = move |thread, turn| -> AdmissionFuture {
        let calls = calls.clone();
        let held = held.clone();
        Box::pin(async move {
            calls.lock().await.push((thread, turn));
            if thread == secondary_id {
                let (reached, wait) = held
                    .lock()
                    .await
                    .take()
                    .ok_or("second root admitted twice")?;
                reached.send(()).map_err(|_| "admission observer left")?;
                wait.await.map_err(|_| "admission gate left")?;
                if refuse {
                    return Err("native final root refused".into());
                }
            }
            Ok(())
        })
    };
    let finalized = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let final_called = finalized.clone();
    let final_db = db.clone();
    let activate = move || -> AdmissionFuture {
        let called = final_called.clone();
        let db = final_db.clone();
        Box::pin(async move {
            assert!(
                !called.swap(true, std::sync::atomic::Ordering::SeqCst),
                "final activation is owned once"
            );
            if goal_case != 0 {
                let row = db
                    .thread_goals()
                    .get_thread_goal(primary_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or("restored goal is absent")?;
                assert_eq!((row.tokens_used, row.time_used_seconds), (93, 11));
            }
            if goal_case == 3 {
                return Err("native final activation refused".into());
            }
            Ok(())
        })
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    if reload == 2 {
        std::fs::create_dir_all(secondary_cwd.join(".codex").as_path())?;
        std::fs::write(
            secondary_cwd.join(".codex/config.toml").as_path(),
            "[invalid project policy",
        )?;
    }
    let request = if reload != 0 {
        ForestBootstrapRequest::new_reloading(
            primary_id,
            &expected,
            (*snapshot).clone(),
            deadline,
            admit,
            activate,
        )
    } else {
        match &goal_state {
            Some(goals) => ForestBootstrapRequest::new_with_goals(
                primary_id,
                &expected,
                goals.clone(),
                policies,
                deadline,
                admit,
                activate,
            ),
            None => ForestBootstrapRequest::new(
                primary_id,
                &expected,
                (*snapshot).clone(),
                policies,
                deadline,
                admit,
                activate,
            ),
        }
    }
    .map_err(anyhow::Error::msg)?;
    let bootstrap = sender.bootstrap_upgrade_forest(request.clone()).await;
    if reload == 2 {
        let error = bootstrap
            .err()
            .expect("invalid secondary policy must refuse bootstrap");
        assert!(
            error.to_string().contains("fresh application policy"),
            "actual bootstrap error: {error}"
        );
        assert!(admissions.lock().await.is_empty());
        assert!(!finalized.load(std::sync::atomic::Ordering::SeqCst));
        assert!(destination.requests().is_empty());
        let loaded = sender
            .request(codex_app_server_protocol::ClientRequest::ThreadLoadedList {
                request_id: codex_app_server_protocol::RequestId::Integer(912),
                params: codex_app_server_protocol::ThreadLoadedListParams {
                    cursor: None,
                    limit: None,
                },
            })
            .await?
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        let loaded: codex_app_server_protocol::ThreadLoadedListResponse =
            serde_json::from_value(loaded)?;
        assert!(
            loaded.data.is_empty(),
            "all policies load before any Core starts"
        );
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(10), pump).await???;
        return Ok(());
    }
    let configured = bootstrap?;
    if reload != 0 {
        let (_, (_, secondary)) = configured
            .roots
            .iter()
            .find(|(id, _)| *id == secondary_id)
            .unwrap();
        assert_eq!(secondary.cwd, secondary_cwd);
        assert!(
            secondary.rollout_path.is_none(),
            "private roots must remain ephemeral"
        );
    }
    let (secondary_context, _) = configured
        .root_context(secondary_id)
        .await
        .map_err(anyhow::Error::msg)?;
    assert_eq!(secondary_context.cwd(), &secondary_cwd);
    assert_eq!(secondary_context.model_provider_id, "forest-loopback");
    assert!(secondary_context.ephemeral);
    assert!(configured.root_context(ThreadId::new()).await.is_err());
    assert_eq!(configured.primary, primary_id);
    assert!(
        configured
            .roots
            .iter()
            .all(|(_, (_, root))| root.model == "gpt-5.6-sol"
                && root.model_provider_id == "forest-loopback"),
        "bootstrap reports actual restored preferences rather than default startup settings"
    );
    assert_eq!(configured.roots.len(), 2);
    assert_eq!(
        sender.bootstrap_upgrade_forest(request).await?.roots.len(),
        2
    );
    assert!(admissions.lock().await.is_empty());
    assert!(!finalized.load(std::sync::atomic::Ordering::SeqCst));
    assert!(sender.activate_upgrade_forest(secondary_id).await.is_err());
    assert!(sender.activate_idle_exec_tree(primary_id).await.is_err());
    let mut activation = Some(tokio::spawn({
        let sender = sender.clone();
        async move { sender.activate_upgrade_forest(primary_id).await }
    }));
    tokio::time::timeout(Duration::from_secs(10), reached_rx)
        .await
        .map_err(|_| anyhow::anyhow!("last admission was not reached"))??;
    assert_eq!(
        *admissions.lock().await,
        vec![(primary_id, None), (secondary_id, None)]
    );
    assert_eq!(source_requests.requests().len(), 1);
    assert!(
        destination.requests().is_empty(),
        "first root sampled before the final root's native admission"
    );
    if goal_case != 0 {
        let row = db
            .thread_goals()
            .get_thread_goal(primary_id)
            .await?
            .unwrap();
        assert_eq!(
            (row.tokens_used, row.time_used_seconds),
            (93, 11),
            "startup must not account or continue before the final admission"
        );
    }
    if cancel_waiter {
        let waiter = activation.take().unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
    }
    release.send(()).unwrap();
    if !cancel_waiter {
        let activated =
            tokio::time::timeout(Duration::from_secs(10), activation.take().unwrap()).await??;
        if refuse || matches!(goal_case, 2 | 3) {
            assert!(activated.unwrap_err().to_string().contains(if refuse {
                "native final root refused"
            } else if goal_case == 3 {
                "native final activation refused"
            } else {
                "persisted goal differs"
            }));
        } else {
            activated?;
        }
    }
    if refuse || matches!(goal_case, 2 | 3) {
        assert!(sender.activate_upgrade_forest(primary_id).await.is_err());
        assert!(destination.requests().is_empty());
    } else {
        sender.activate_upgrade_forest(primary_id).await?;
        let mut observed = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = events.recv().await {
                observed.push(format!("{event:?}"));
                if let crate::in_process::InProcessServerEvent::ServerNotification(notification) =
                    event
                    && let codex_app_server_protocol::ServerNotification::TurnCompleted(done) =
                        *notification
                    && done.thread_id == primary_id.to_string()
                {
                    return Ok::<_, anyhow::Error>(());
                }
            }
            anyhow::bail!("destination event stream closed before the restored turn completed")
        })
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "no completed destination turn; model requests={}; events={observed:?}",
                destination.requests().len()
            )
        })??;
        assert_eq!(
            destination.requests().len(),
            if goal_case == 1 { 2 } else { 1 }
        );
        if goal_case == 1 {
            let row = db
                .thread_goals()
                .get_thread_goal(primary_id)
                .await?
                .unwrap();
            assert_eq!(row.status, codex_state::ThreadGoalStatus::Paused);
            assert_eq!(row.tokens_used, 93);
            assert!(
                row.time_used_seconds >= 16,
                "carried native clock was reset at startup"
            );
        }
    }
    assert_eq!(
        finalized.load(std::sync::atomic::Ordering::SeqCst),
        !refuse && goal_case != 2
    );
    assert_eq!(
        *admissions.lock().await,
        if refuse {
            vec![(primary_id, None), (secondary_id, None)]
        } else {
            vec![(primary_id, None), (secondary_id, None), (child_id, None)]
        }
    );
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(10), pump).await???;
    Ok(())
}

async fn start_client(config: Config) -> anyhow::Result<crate::in_process::InProcessClientHandle> {
    use crate::in_process::*;
    let state_db = codex_rollout::state_db::try_init(&config).await?;
    assert!(
        config
            .model_provider
            .base_url
            .as_deref()
            .is_some_and(
                |url| url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")
            ),
        "forest fixtures must use the loopback model server"
    );
    let cli_overrides = vec![
        (
            "model_provider".into(),
            toml::Value::String(config.model_provider_id.clone()),
        ),
        (
            format!("model_providers.{}", config.model_provider_id),
            toml::Value::try_from(&config.model_provider)?,
        ),
    ];
    Ok(start(InProcessStartArgs {
        arg0_paths: Default::default(),
        config: Arc::new(config),
        cli_overrides,
        loader_overrides: codex_config::LoaderOverrides::without_managed_config_for_tests(),
        strict_config: false,
        cloud_config_bundle: Default::default(),
        embedded_network_policy: Default::default(),
        thread_config_loader: Arc::new(codex_config::NoopThreadConfigLoader),
        feedback: codex_feedback::CodexFeedback::new(),
        log_db: None,
        state_db: Some(state_db),
        environment_manager: Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        config_warnings: Vec::new(),
        session_source: SessionSource::Cli,
        enable_codex_api_key_env: false,
        initialize: codex_app_server_protocol::InitializeParams {
            client_info: codex_app_server_protocol::ClientInfo {
                name: "forest-upgrade-test".into(),
                title: None,
                version: "test".into(),
            },
            capabilities: None,
        },
        channel_capacity: 32,
    })
    .await?)
}
