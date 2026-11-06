// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_alab_core::quota::HeadlessExec;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::config::ExecConfiguration;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnInputSubmission;
use codex_protocol::user_input::UserInput;
use codex_rollout::InitialHistory;
use codex_rollout::ResumedHistory;
use codex_thread_store::LoadThreadHistoryParams;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::run_test_with_large_stack;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::sync::oneshot;

#[test]
fn exec_recovery_tree_keeps_internal_children_and_survives_a_lost_waiter() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-recovery-tree", || recovery_tree(TestMode::Success))
}

#[test]
fn exec_recovery_tree_refusal_stops_every_member_before_sampling() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-recovery-tree-denied", || {
        recovery_tree(TestMode::Refusal)
    })
}

#[test]
fn exec_recovery_tree_crosses_the_embedded_bootstrap_boundary() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-tree-bootstrap", || recovery_tree(TestMode::Bootstrap))
}

#[test]
fn exec_recovery_tree_deadline_cancels_pending_admission_without_sampling() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-tree-deadline", || recovery_tree(TestMode::Deadline))
}

enum TestMode {
    Success,
    Refusal,
    Bootstrap,
    Deadline,
}

async fn recovery_tree(mode: TestMode) -> anyhow::Result<()> {
    let deny_child = matches!(mode, TestMode::Refusal);
    let bootstrap = matches!(mode, TestMode::Bootstrap);
    let expire = matches!(mode, TestMode::Deadline);
    let (_root_release, root_gate) = oneshot::channel();
    let (_child_release, child_gate) = oneshot::channel();
    let (source, _) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: Some(root_gate),
            body: responses::sse_completed("source-root"),
        }],
        vec![StreamingSseChunk {
            gate: Some(child_gate),
            body: responses::sse_completed("source-child"),
        }],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse_completed("destination-first"),
        }],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse_completed("destination-second"),
        }],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse_completed("destination-third"),
        }],
    ])
    .await;
    let mock = responses::start_mock_server().await;
    let fixture = test_codex()
        .with_model("gpt-5.6-sol")
        .build_with_auto_env(&mock)
        .await?;
    let mut options = fixture.start_thread_options().await;
    options.config.model_provider.base_url = Some(format!("{}/v1", source.uri()));
    options.config.model_provider_id = "tree-fixture".into();
    options.config.model_provider.include_internal_metadata = false;
    if bootstrap {
        options.config.model_provider.requires_openai_auth = false;
    }
    options
        .config
        .model_providers
        .insert("tree-fixture".into(), options.config.model_provider.clone());
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let invocation = ExecConfiguration::capture(&options.config, &ConfigOverrides::default())?;
    let child_config = options.config.clone();
    let root = fixture.thread_manager.start_thread(options).await?.thread;
    let root_id = root.exec_thread_id();
    let environments = root
        .environment_selections()
        .await
        .into_iter()
        .map(codex_protocol::protocol::TurnEnvironmentSelection::into_request)
        .collect::<Vec<_>>();
    let mut children = Vec::new();
    for _ in 0..2 {
        let mut options = fixture.start_thread_options().await;
        options.config = child_config.clone();
        options.session_source = Some(SessionSource::Internal(
            InternalSessionSource::MemoryConsolidation,
        ));
        options.thread_extension_init.insert(HeadlessExec);
        options.turn_extension_init.insert(HeadlessExec);
        children.push(
            fixture
                .thread_manager
                .spawn_internal_session(root_id, options)
                .await?
                .thread,
        );
    }
    let text = |value: &str| {
        TurnInputRequest::user_input(vec![UserInput::Text {
            text: value.into(),
            text_elements: Vec::new(),
        }])
    };
    let TurnInputSubmission::Started {
        turn_id: root_turn, ..
    } = root.start_or_steer_turn(text("tree root input")).await?
    else {
        anyhow::bail!("root not started")
    };
    tokio::time::timeout(Duration::from_secs(10), source.wait_for_request_count(1)).await?;
    let StartIfIdleSubmission::Started {
        turn_id: child_turn,
        ..
    } = children[0]
        .start_turn_if_idle(TurnInputRequest::new(
            codex_protocol::turn_input::TurnInput::InterAgentCommunication(
                codex_protocol::protocol::InterAgentCommunication::new_encrypted(
                    codex_protocol::AgentPath::root(),
                    codex_protocol::AgentPath::root(),
                    Vec::new(),
                    "tree child input".into(),
                    true,
                ),
            ),
        ))
        .await?
    else {
        anyhow::bail!("child not started")
    };
    tokio::time::timeout(Duration::from_secs(10), source.wait_for_request_count(2)).await?;
    let child_evidence = children[0].exec_recovery_history().await?;
    assert!(!child_evidence.items.iter().any(|item| matches!(
        item,
        codex_rollout::RolloutItem::EventMsg(EventMsg::UserMessage(_))
    )));
    assert!(
        child_evidence.items.iter().any(|item| matches!(item,
        codex_rollout::RolloutItem::ResponseItem(envelope)
            if matches!(envelope.item, codex_protocol::models::ResponseItem::AgentMessage { .. })))
    );
    let child_id = children[0].exec_thread_id();
    let idle_id = children[1].exec_thread_id();
    let captured = super::super::super::State::default()
        .suspend(
            &fixture.thread_manager,
            &crate::turn_admission::TurnAdmission::default(),
            root_id,
            &root_turn,
            &invocation,
        )
        .await
        .map_err(anyhow::Error::msg)?;
    let snapshot = captured.tree().clone();
    if bootstrap {
        bootstrap_tree(
            child_config,
            snapshot,
            &source,
            &fixture,
            root_id,
            &root_turn,
            child_id,
            &child_turn,
        )
        .await?;
        source.shutdown().await;
        return Ok(());
    }
    let destination = responses::start_mock_server().await;
    let replacement = test_codex()
        .with_model("gpt-5.6-sol")
        .with_home(Arc::clone(&fixture.home))
        .with_thread_store(Arc::clone(&fixture.thread_store))
        .build_with_auto_env(&destination)
        .await?;
    let history = fixture
        .thread_store
        .load_latest_model_context(LoadThreadHistoryParams {
            thread_id: root_id,
            include_archived: false,
        })
        .await?;
    let mut options = replacement.start_thread_options().await;
    options.config.cwd = child_config.cwd.clone();
    options.config.workspace_roots = child_config.workspace_roots.clone();
    options.config.model_provider_id = child_config.model_provider_id.clone();
    options.config.model_provider = child_config.model_provider.clone();
    options.config.model_providers = child_config.model_providers.clone();
    options.environments = Some(environments);
    options.initial_history = InitialHistory::Resumed(ResumedHistory {
        conversation_id: root_id,
        history: Arc::new(history.items),
        history_revision: history.revision,
        rollout_path: None,
    });
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let mut inputs = snapshot
        .inputs()
        .clone()
        .prepare(root_id, &root_turn)
        .map_err(anyhow::Error::msg)?;
    let options = inputs
        .prepare_start(options, root_id, &root_turn)
        .map_err(anyhow::Error::msg)?;
    let root = replacement
        .thread_manager
        .start_thread(options)
        .await?
        .thread;
    let (admit_child, child_admission) = oneshot::channel();
    let child_admission = Arc::new(Mutex::new(Some(child_admission)));
    let reached_child = Arc::new(Notify::new());
    let admissions = Arc::new(Mutex::new(Vec::new()));
    let recovery = ExecRecoveryTree::prepare(
        Arc::clone(&root),
        snapshot,
        inputs,
        tokio::time::Instant::now() + Duration::from_secs(if expire { 2 } else { 30 }),
        {
            let child_admission = Arc::clone(&child_admission);
            let reached_child = Arc::clone(&reached_child);
            let admissions = Arc::clone(&admissions);
            move |id, turn| {
                let child_admission = Arc::clone(&child_admission);
                let reached_child = Arc::clone(&reached_child);
                let admissions = Arc::clone(&admissions);
                Box::pin(async move {
                    admissions.lock().await.push((id, turn));
                    if id == child_id {
                        let gate = child_admission
                            .lock()
                            .await
                            .take()
                            .ok_or("child admitted twice")?;
                        reached_child.notify_one();
                        gate.await.map_err(|_| "child admission lost")?;
                        if deny_child {
                            return Err("native child admission refused".into());
                        }
                    }
                    Ok(())
                })
            }
        },
    )
    .await
    .map_err(anyhow::Error::msg)?;
    assert!(
        replacement
            .thread_manager
            .get_thread(child_id)
            .await
            .is_err(),
        "internal child must not be resolved through public lookup"
    );
    let mut waiter = tokio::spawn({
        let recovery = Arc::clone(&recovery);
        async move { recovery.activate().await }
    });
    tokio::select! {
        result = &mut waiter => anyhow::bail!("activation ended before child admission: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(10), reached_child.notified()) => {
            result.map_err(|e| anyhow::anyhow!("waiting for child native admission: {e}"))?;
        }
    }
    assert_eq!(
        source.requests().await.len(),
        2,
        "root must wait for child native admission"
    );
    let idle = &recovery.threads[&idle_id];
    assert!(matches!(
        idle.start_or_steer_turn(text("new input for restored idle child"))
            .await?,
        TurnInputSubmission::Started { .. }
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), source.wait_for_request_count(3))
            .await
            .is_err(),
        "a new idle-child task must also wait for tree release"
    );
    waiter.abort();
    if !expire {
        admit_child.send(()).unwrap();
    }
    let result = tokio::time::timeout(Duration::from_secs(10), recovery.activate())
        .await
        .map_err(|e| anyhow::anyhow!("waiting for tree activation: {e}"))?;
    if deny_child || expire {
        assert!(result.is_err());
        if expire {
            assert!(result.as_ref().unwrap_err().contains("deadline"));
        }
        assert_eq!(
            recovery.activate().await,
            result,
            "refusal must reconcile without another admission"
        );
        assert_eq!(
            source.requests().await.len(),
            2,
            "no destination task may run on a partial admission"
        );
        for thread in recovery.threads.values() {
            tokio::time::timeout(Duration::from_secs(10), thread.wait_until_terminated()).await?;
        }
        source.shutdown().await;
        return Ok(());
    }
    let result = result.map_err(anyhow::Error::msg)?;
    assert_eq!(
        recovery.activate().await.map_err(anyhow::Error::msg)?,
        result
    );
    assert!(
        matches!(result, StartIfIdleSubmission::Started { turn_id, .. } if turn_id == root_turn)
    );
    let child = &recovery.threads[&child_id];
    for thread in [&root, child, idle] {
        tokio::time::timeout(
            Duration::from_secs(10),
            wait_for_event(thread, |event| matches!(event, EventMsg::TurnComplete(_))),
        )
        .await
        .map_err(|e| {
            anyhow::anyhow!("waiting for completion of {}: {e}", thread.exec_thread_id())
        })?;
    }
    let admitted = admissions.lock().await.clone();
    assert_eq!(
        admitted,
        vec![
            (idle_id, None),
            (root_id, Some(root_turn)),
            (child_id, Some(child_turn))
        ]
    );
    let requests = source.requests().await;
    assert_eq!(requests.len(), 5);
    let bodies = requests[2..]
        .iter()
        .map(|r| serde_json::from_slice::<serde_json::Value>(r).unwrap()["input"].to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(bodies.matches("tree root input").count(), 1);
    assert_eq!(bodies.matches("tree child input").count(), 1);
    assert_eq!(
        bodies.matches("new input for restored idle child").count(),
        1
    );
    root.shutdown_and_wait().await?;
    source.shutdown().await;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "exercise the production boundary using the captured source fixture"
)]
async fn bootstrap_tree(
    config: codex_core::config::Config,
    snapshot: ExecTreeSnapshot,
    source: &core_test_support::streaming_sse::StreamingSseServer,
    fixture: &core_test_support::test_codex::TestCodex,
    root_id: ThreadId,
    root_turn: &str,
    child_id: ThreadId,
    child_turn: &str,
) -> anyhow::Result<()> {
    use crate::in_process::*;
    use codex_app_server_protocol::ClientInfo;
    use codex_app_server_protocol::InitializeParams;
    let state_db = codex_rollout::state_db::try_init(&config).await?;
    let client = crate::in_process::start(InProcessStartArgs {
        arg0_paths: Default::default(),
        config: Arc::new(config.clone()),
        cli_overrides: Vec::new(),
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
        session_source: SessionSource::Exec,
        enable_codex_api_key_env: false,
        initialize: InitializeParams {
            client_info: ClientInfo {
                name: "codex-tree-bootstrap-test".into(),
                title: None,
                version: "test".into(),
            },
            capabilities: None,
        },
        channel_capacity: 32,
    })
    .await?;
    let sender = client.sender();
    let cancel = tokio_util::sync::CancellationToken::new();
    let pump_cancel = cancel.clone();
    let (send, mut events) = tokio::sync::mpsc::unbounded_channel();
    let pump = tokio::spawn(async move {
        let mut client = client;
        loop {
            tokio::select! {
                _ = pump_cancel.cancelled() => break,
                event = client.next_event() => match event { Some(event) => { let _ = send.send(event); }, None => break },
            }
        }
        client.shutdown().await
    });
    let _cancel_on_drop = cancel.clone().drop_guard();
    let admitted = Arc::new(Mutex::new(Vec::new()));
    let admissions = Arc::clone(&admitted);
    let request = super::super::super::ExecBootstrapRequest::new(
        config,
        snapshot,
        tokio::time::Instant::now() + Duration::from_secs(30),
        move |thread, turn| {
            let admissions = Arc::clone(&admissions);
            Box::pin(async move {
                admissions.lock().await.push((thread, turn));
                Ok(())
            })
        },
    );
    let configured = sender.bootstrap_exec(request.clone()).await?;
    assert_eq!(configured.1.session_id, root_id.into());
    assert_eq!(
        sender.bootstrap_exec(request).await?.1.session_id,
        root_id.into()
    );
    assert!(
        admitted.lock().await.is_empty(),
        "bootstrap cannot authorize model work"
    );
    assert_eq!(source.requests().await.len(), 2);
    assert!(
        sender
            .activate_exec_tree(root_id, "changed-turn".into())
            .await
            .is_err()
    );
    let result = sender.activate_exec_tree(root_id, root_turn.into()).await?;
    assert!(
        matches!(result, StartIfIdleSubmission::Started { ref turn_id, .. } if turn_id == root_turn)
    );
    assert_eq!(
        sender.activate_exec_tree(root_id, root_turn.into()).await?,
        result
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = events.recv().await {
            if let InProcessServerEvent::ServerNotification(notification) = event
                && let codex_app_server_protocol::ServerNotification::TurnCompleted(done) =
                    *notification
                && done.thread_id == root_id.to_string()
                && done.turn.id == root_turn
            {
                assert_eq!(
                    done.turn.status,
                    codex_app_server_protocol::TurnStatus::Completed
                );
                return;
            }
        }
        panic!("event pump ended before original root completion");
    })
    .await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            // The writer may advance while the guarded history read is in flight.
            // Wait for a stable complete view rather than unwrapping that refusal.
            if let Ok(history) = fixture.thread_store.load_latest_turn_history(LoadThreadHistoryParams { thread_id: child_id, include_archived: false }).await
                && history.items.iter().any(|item| matches!(item,
                codex_rollout::RolloutItem::EventMsg(EventMsg::TurnComplete(done)) if done.turn_id == child_turn)) {
                assert!(!history.items.iter().any(|item| matches!(item, codex_rollout::RolloutItem::EventMsg(EventMsg::TurnAborted(_)))));
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await?;
    assert_eq!(source.requests().await.len(), 4);
    assert_eq!(admitted.lock().await.len(), 3);
    cancel.cancel();
    pump.await??;
    Ok(())
}

#[test]
fn exec_review_tree_recovers_original_delegate_after_all_admissions() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-review-tree", || review_tree(false, false, false))
}

#[test]
fn exec_review_tree_refusal_stops_before_any_destination_request() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-review-tree-refusal", || {
        review_tree(true, false, false)
    })
}

#[test]
fn exec_review_tree_delivers_a_result_completed_while_frozen() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-review-completed-tree", || {
        review_tree(false, true, false)
    })
}

// The isolated Core fixture has no app-server runtime contributors. Supply its
// exec membership before startup so ephemeral delegates record canonical memory.
// Native qualification separately exercises the real host's registration.
struct ReviewExecHost;

impl codex_extension_api::ThreadLifecycleContributor<Config> for ReviewExecHost {
    fn on_thread_start<'a>(
        &'a self,
        input: codex_extension_api::ThreadStartInput<'a, Config>,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input
                .thread_store
                .insert(codex_alab_core::quota::ExecQuotaMember);
        })
    }
}

impl codex_extension_api::TurnLifecycleContributor for ReviewExecHost {
    fn on_turn_start<'a>(
        &'a self,
        input: codex_extension_api::TurnStartInput<'a>,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input
                .turn_store
                .insert(codex_alab_core::quota::ExecQuotaMember);
        })
    }
}

fn review_exec_host() -> Arc<codex_extension_api::ExtensionRegistry<Config>> {
    let mut builder = codex_extension_api::ExtensionRegistryBuilder::new();
    let host = Arc::new(ReviewExecHost);
    builder.thread_lifecycle_contributor(host.clone());
    builder.turn_lifecycle_contributor(host);
    Arc::new(builder.build())
}

#[test]
fn ephemeral_exec_review_tree_preserves_typed_result_without_storage() -> anyhow::Result<()> {
    run_test_with_large_stack("ephemeral-exec-review-tree", || {
        review_tree(false, false, true)
    })
}

#[test]
fn ephemeral_exec_review_tree_refusal_stops_before_sampling() -> anyhow::Result<()> {
    run_test_with_large_stack("ephemeral-exec-review-refused", || {
        review_tree(true, false, true)
    })
}

async fn review_tree(deny: bool, completed_source: bool, ephemeral: bool) -> anyhow::Result<()> {
    use codex_protocol::items::TurnItem;
    use codex_protocol::protocol::Op;
    use codex_protocol::protocol::ReviewRequest;
    use codex_protocol::protocol::ReviewTarget;
    let (source_release, source_gate) = oneshot::channel();
    let review_json = serde_json::json!({
        "findings": [], "overall_correctness": "correct",
        "overall_explanation": "original review resumed", "overall_confidence_score": 1.0
    })
    .to_string();
    let (server, _) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: Some(source_gate),
            body: if completed_source {
                responses::sse(vec![
                    responses::ev_assistant_message("source-result", &review_json),
                    responses::ev_completed("source-review"),
                ])
            } else {
                responses::sse_completed("source-review")
            },
        }],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![
                responses::ev_assistant_message("review-result", &review_json),
                responses::ev_completed("destination-review"),
            ]),
        }],
    ])
    .await;
    let mock = responses::start_mock_server().await;
    let registry = if ephemeral {
        review_exec_host()
    } else {
        codex_extension_api::empty_extension_registry()
    };
    let fixture = test_codex()
        .with_extensions(Arc::clone(&registry))
        .with_model("gpt-5.6-sol")
        .build_with_auto_env(&mock)
        .await?;
    let mut options = fixture.start_thread_options().await;
    options.config.model_provider.base_url = Some(format!("{}/v1", server.uri()));
    options.config.ephemeral = ephemeral;
    if ephemeral {
        options.history_mode = Some(codex_protocol::protocol::ThreadHistoryMode::Legacy);
    }
    options.config.model_provider_id = "review-tree-fixture".into();
    options.config.model_provider.include_internal_metadata = false;
    options.config.model_providers.insert(
        "review-tree-fixture".into(),
        options.config.model_provider.clone(),
    );
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let config = options.config.clone();
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default())?;
    let root = fixture.thread_manager.start_thread(options).await?.thread;
    let root_id = root.exec_thread_id();
    let environments = root
        .environment_selections()
        .await
        .into_iter()
        .map(codex_protocol::protocol::TurnEnvironmentSelection::into_request)
        .collect::<Vec<_>>();
    let root_turn = root
        .submit(Op::Review {
            review_request: ReviewRequest {
                target: ReviewTarget::Custom {
                    instructions: "preserve this original review".into(),
                },
                user_facing_hint: None,
            },
        })
        .await?;
    tokio::time::timeout(Duration::from_secs(10), server.wait_for_request_count(1)).await?;
    let observed = root
        .watch_exec_quota()
        .ok_or_else(|| anyhow::anyhow!("review root has no quota watcher"))?
        .root_options()
        .await
        .ok_or_else(|| anyhow::anyhow!("review root is not observable"))?;
    assert!(observed.is_review());
    assert_eq!(observed.thread(), root_id);
    assert_eq!(observed.turn(), root_turn);
    let source_child = if completed_source {
        let stored = fixture
            .thread_store
            .list_threads(codex_thread_store::ListThreadsParams {
                page_size: 10,
                cursor: None,
                sort_key: Default::default(),
                sort_direction: Default::default(),
                allowed_sources: vec![SessionSource::SubAgent(
                    codex_protocol::protocol::SubAgentSource::Review,
                )],
                model_providers: Some(Vec::new()),
                cwd_filters: None,
                section: None,
                project_id: None,
                archived: false,
                search_term: None,
                relation_filter: None,
                use_state_db_only: false,
            })
            .await?;
        Some(
            stored
                .items
                .iter()
                .find(|thread| thread.parent_thread_id == Some(root_id))
                .ok_or_else(|| anyhow::anyhow!("review delegate is not stored"))?
                .thread_id,
        )
    } else {
        None
    };
    let plan = root
        .prepare_exec_tree(&root_turn, &invocation)
        .await
        .map_err(anyhow::Error::msg)?;
    if completed_source {
        source_release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let history = match fixture
                    .thread_store
                    .load_latest_turn_history(LoadThreadHistoryParams {
                        thread_id: source_child.unwrap(),
                        include_archived: false,
                    })
                    .await
                {
                    Ok(history) => history,
                    Err(error)
                        if error
                            .to_string()
                            .contains("recovery history changed during its read") =>
                    {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                };
                if history.items.iter().any(|item| {
                    matches!(
                        item,
                        codex_rollout::RolloutItem::EventMsg(EventMsg::TurnComplete(_))
                    )
                }) {
                    return Ok::<(), anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await??;
    }
    let captured = plan
        .suspend_and_capture()
        .await
        .map_err(anyhow::Error::msg)?;
    let snapshot = ExecTreeSnapshot::capture(&captured).map_err(anyhow::Error::msg)?;
    let snapshot: ExecTreeSnapshot = serde_json::from_slice(&serde_json::to_vec(&snapshot)?)?;
    if ephemeral {
        assert!(root.rollout_path().is_none());
        for node in snapshot.threads() {
            assert!(node.ephemeral_history().is_some());
            assert_eq!(
                node.history().map_err(anyhow::Error::msg)?.thread_id,
                node.thread()
            );
            assert!(
                fixture
                    .thread_store
                    .load_history(LoadThreadHistoryParams {
                        thread_id: node.thread(),
                        include_archived: false,
                    })
                    .await
                    .is_err()
            );
        }
    }
    let delegate = snapshot.threads()[0].review().unwrap().delegate();
    let delegate_turn = snapshot.threads()[0]
        .review()
        .unwrap()
        .delegate_turn()
        .to_owned();
    let destination = responses::start_mock_server().await;
    let replacement = test_codex()
        .with_extensions(registry)
        .with_model("gpt-5.6-sol")
        .with_home(Arc::clone(&fixture.home))
        .with_thread_store(Arc::clone(&fixture.thread_store))
        .build_with_auto_env(&destination)
        .await?;
    let mut options = replacement.start_thread_options().await;
    options.config = config;
    options.environments = Some(environments);
    if let Some(memory) = snapshot.threads()[0].ephemeral_history().cloned() {
        options = memory
            .prepare_start(options, root_id, &root_turn)
            .map_err(anyhow::Error::msg)?;
    } else {
        let history = fixture
            .thread_store
            .load_latest_model_context(LoadThreadHistoryParams {
                thread_id: root_id,
                include_archived: false,
            })
            .await?;
        options.initial_history = InitialHistory::Resumed(ResumedHistory {
            conversation_id: root_id,
            history: Arc::new(history.items),
            history_revision: history.revision,
            rollout_path: None,
        });
    }
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let mut inputs = snapshot
        .inputs()
        .clone()
        .prepare(root_id, &root_turn)
        .map_err(anyhow::Error::msg)?;
    let options = inputs
        .prepare_start(options, root_id, &root_turn)
        .map_err(anyhow::Error::msg)?;
    let root = replacement
        .thread_manager
        .start_thread(options)
        .await?
        .thread;
    let (admit_child, child_admission) = oneshot::channel();
    let child_admission = Arc::new(Mutex::new(Some(child_admission)));
    let reached_child = Arc::new(Notify::new());
    let admissions = Arc::new(Mutex::new(Vec::new()));
    let recovery = ExecRecoveryTree::prepare(
        Arc::clone(&root),
        snapshot,
        inputs,
        tokio::time::Instant::now() + Duration::from_secs(30),
        {
            let child_admission = Arc::clone(&child_admission);
            let reached_child = Arc::clone(&reached_child);
            let admissions = Arc::clone(&admissions);
            move |id, turn| {
                let child_admission = Arc::clone(&child_admission);
                let reached_child = Arc::clone(&reached_child);
                let admissions = Arc::clone(&admissions);
                Box::pin(async move {
                    admissions.lock().await.push((id, turn));
                    if id == delegate {
                        let gate = child_admission
                            .lock()
                            .await
                            .take()
                            .ok_or("delegate admitted twice")?;
                        reached_child.notify_one();
                        gate.await.map_err(|_| "delegate admission lost")?;
                        if deny {
                            return Err("native review delegate admission refused".into());
                        }
                    }
                    Ok(())
                })
            }
        },
    )
    .await
    .map_err(anyhow::Error::msg)?;
    let activation = tokio::spawn({
        let recovery = Arc::clone(&recovery);
        async move { recovery.activate().await }
    });
    tokio::time::timeout(Duration::from_secs(10), reached_child.notified()).await?;
    assert_eq!(
        server.requests().await.len(),
        1,
        "review ran before delegate admission"
    );
    admit_child.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), activation).await??;
    if deny {
        assert!(result.is_err());
        assert_eq!(recovery.activate().await, result);
        assert_eq!(
            server.requests().await.len(),
            1,
            "refused review sampled the model"
        );
        assert_eq!(admissions.lock().await.len(), 2);
        for thread in recovery.threads.values() {
            tokio::time::timeout(Duration::from_secs(10), thread.wait_until_terminated()).await?;
        }
        server.shutdown().await;
        return Ok(());
    }
    let result = result.map_err(anyhow::Error::msg)?;
    assert!(
        matches!(result, StartIfIdleSubmission::Started { turn_id, .. } if turn_id == root_turn)
    );
    let output = tokio::time::timeout(Duration::from_secs(10), wait_for_event(&root, |event| {
        matches!(event, EventMsg::ItemCompleted(event) if matches!(event.item, TurnItem::ExitedReviewMode(_)))
    })).await?;
    let EventMsg::ItemCompleted(output) = output else {
        unreachable!()
    };
    assert_eq!(output.turn_id, root_turn);
    let TurnItem::ExitedReviewMode(output) = output.item else {
        unreachable!()
    };
    assert_eq!(
        serde_json::to_value(output.review_output.unwrap())?,
        serde_json::from_str::<serde_json::Value>(&review_json)?
    );
    tokio::time::timeout(
        Duration::from_secs(10),
        wait_for_event(&root, |event| {
            matches!(event,
        EventMsg::TurnComplete(event) if event.turn_id == root_turn)
        }),
    )
    .await?;
    let expected_admissions = if completed_source {
        vec![(delegate, None), (root_id, Some(root_turn.clone()))]
    } else {
        vec![
            (root_id, Some(root_turn.clone())),
            (delegate, Some(delegate_turn)),
        ]
    };
    assert_eq!(*admissions.lock().await, expected_admissions);
    assert_eq!(
        recovery.threads.len(),
        2,
        "recovery created another review delegate"
    );
    assert_eq!(
        server.requests().await.len(),
        if completed_source { 1 } else { 2 },
        "review restarted instead of resuming"
    );
    if ephemeral {
        let history = root.exec_recovery_history().await?;
        let mut entered = 0;
        let mut exited = 0;
        let mut completed = 0;
        for item in history.items {
            if let codex_rollout::RolloutItem::EventMsg(event) = item {
                match event {
                    EventMsg::ItemCompleted(event) => match event.item {
                        TurnItem::EnteredReviewMode(_) => {
                            assert_eq!(event.turn_id, root_turn);
                            entered += 1;
                        }
                        TurnItem::ExitedReviewMode(_) => {
                            assert_eq!(event.turn_id, root_turn);
                            exited += 1;
                        }
                        _ => {}
                    },
                    EventMsg::TurnComplete(event) if event.turn_id == root_turn => completed += 1,
                    EventMsg::TurnAborted(_) => panic!("ephemeral review was aborted"),
                    _ => {}
                }
            }
        }
        assert_eq!((entered, exited, completed), (1, 1, 1));
        for thread in recovery.threads.values() {
            assert!(thread.rollout_path().is_none());
            assert!(
                replacement
                    .thread_store
                    .load_history(LoadThreadHistoryParams {
                        thread_id: thread.exec_thread_id(),
                        include_archived: false,
                    })
                    .await
                    .is_err()
            );
        }
    }
    root.shutdown_and_wait().await?;
    server.shutdown().await;
    Ok(())
}
