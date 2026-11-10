// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::StartThreadOptions;
use crate::ThreadManager;
use crate::config::Config;
use crate::config::ConfigOverrides;
use crate::config::ExecConfiguration;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use codex_alab_core::quota::HeadlessExec;
use codex_protocol::protocol::InternalSessionSource;
use tokio_util::sync::CancellationToken;

struct WaitingTask(TaskKind);
impl SessionTask for WaitingTask {
    fn kind(&self) -> TaskKind {
        self.0
    }
    fn span_name(&self) -> &'static str {
        "tree_capture_test"
    }
    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _context: Arc<TurnContext>,
        _input: Vec<crate::session::input_queue::TurnInput>,
        token: CancellationToken,
    ) -> SessionTaskResult {
        token.cancelled().await;
        Ok(None)
    }
}

struct CompletedTask(Arc<tokio::sync::Notify>);
impl SessionTask for CompletedTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }
    fn span_name(&self) -> &'static str {
        "completed_child_restore_test"
    }
    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _context: Arc<TurnContext>,
        _input: Vec<crate::session::input_queue::TurnInput>,
        _token: CancellationToken,
    ) -> SessionTaskResult {
        self.0.notified().await;
        Ok(Some(crate::tasks::alab_test_output::completed(
            "original child completed",
        )))
    }
}

async fn fixture() -> (tempfile::TempDir, Config, ThreadManager, Arc<CodexThread>) {
    fixture_version(false).await
}

async fn fixture_version(
    public: bool,
) -> (tempfile::TempDir, Config, ThreadManager, Arc<CodexThread>) {
    fixture_options(public, false).await
}

async fn fixture_options(
    public: bool,
    ephemeral: bool,
) -> (tempfile::TempDir, Config, ThreadManager, Arc<CodexThread>) {
    let home = tempfile::tempdir().unwrap();
    let mut config = crate::config::ConfigBuilder::without_managed_config_for_tests()
        .codex_home(home.path().to_owned())
        .harness_overrides(ConfigOverrides {
            model: Some("gpt-5.5".into()),
            cwd: Some(home.path().to_owned()),
            ..Default::default()
        })
        .build()
        .await
        .unwrap();
    config.ephemeral = ephemeral;
    if public {
        config
            .features
            .enable(codex_features::Feature::MultiAgentV2)
            .unwrap();
        config
            .features
            .enable(codex_features::Feature::Sqlite)
            .unwrap();
    }
    let manager = ThreadManager::with_models_provider_home_and_state_for_tests(
        codex_login::CodexAuth::from_api_key("fixture"),
        config.model_provider.clone(),
        home.path().to_owned(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        crate::init_state_db(&config).await,
    );
    let mut options = StartThreadOptions::new(config.clone());
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    if public {
        options.history_mode = Some(codex_protocol::protocol::ThreadHistoryMode::Paginated);
    }
    let root = manager.start_thread(options).await.unwrap().thread;
    (home, config, manager, root)
}

async fn child(manager: &ThreadManager, parent: &CodexThread, config: &Config) -> Arc<CodexThread> {
    let mut options = StartThreadOptions::new(config.clone());
    options.session_source = Some(SessionSource::Internal(
        InternalSessionSource::MemoryConsolidation,
    ));
    if config.ephemeral {
        options
            .thread_extension_init
            .insert(codex_alab_core::quota::ExecQuotaMember);
    }
    manager
        .spawn_internal_session(parent.session.thread_id, options)
        .await
        .unwrap()
        .thread
}

async fn captured_child(
    manager: &ThreadManager,
    parent: &CodexThread,
    config: &Config,
    public: bool,
) -> Arc<CodexThread> {
    if !public {
        return child(manager, parent, config).await;
    }
    // Exercise the production public-child registration and residency setup,
    // without submitting model input in this bootstrap-only fixture.
    let runtime = &parent.session.services.local_agent_runtime;
    let state = runtime.upgrade().unwrap();
    let control = runtime.control(parent.session.session_id());
    let membership = runtime.admit_start().unwrap();
    let mut reservation = runtime.registry.reserve_spawn_slot(None).unwrap();
    let path = codex_protocol::AgentPath::root()
        .join(&format!("child_{}", runtime.registry.live_agents().len()))
        .unwrap();
    let (source, mut metadata) = control
        .prepare_thread_spawn(
            &mut reservation,
            config,
            parent.session.thread_id,
            1,
            Some(path),
            None,
            None,
        )
        .unwrap();
    let mut options = StartThreadOptions::new(config.clone());
    options.reserved_thread_id = Some(runtime.generate_thread_id());
    options.session_source = Some(source.clone());
    options.thread_source = Some(codex_protocol::protocol::ThreadSource::Subagent);
    options.history_mode = Some(codex_protocol::protocol::ThreadHistoryMode::Paginated);
    options.inherited_environments = Some(parent.session.inherited_environment_snapshot().await);
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    control
        .persist_thread_spawn_edge_for_source(
            config.ephemeral,
            options.reserved_thread_id.unwrap(),
            Some(&source),
        )
        .await
        .unwrap();
    let slot = control
        .reserve_v2_residency_slot(&state, config, &membership, None)
        .await
        .unwrap();
    let started = state
        .spawn_child_thread(
            options,
            control.clone(),
            Some(parent.session.thread_id),
            Some(Arc::clone(&parent.session.services.exec_policy)),
        )
        .await
        .unwrap();
    metadata.agent_id = Some(started.thread_id);
    reservation.commit(metadata);
    slot.commit(started.thread_id);
    assert_eq!(
        started.thread.multi_agent_version(),
        Some(MultiAgentVersion::V2)
    );
    started.thread
}

async fn start(thread: &Arc<CodexThread>, kind: TaskKind) -> String {
    let context = thread.session.new_default_turn().await;
    context.extension_data.insert(HeadlessExec);
    let id = context.sub_id.clone();
    thread
        .session
        .spawn_task(Arc::clone(&context), Vec::new(), WaitingTask(kind))
        .await;
    thread.session.emit_turn_started(&context, kind).await;
    id
}

#[tokio::test]
async fn restores_child_identity_into_fresh_parent_runtime_without_starting_turn() {
    check_restore(false, true).await;
}

#[tokio::test]
async fn restores_public_v2_children_with_residency_pins() {
    check_restore(true, true).await;
}

#[tokio::test]
async fn idle_restoration_refusal_retains_the_uninstalled_mail() {
    check_restore(false, false).await;
}

#[tokio::test]
async fn restores_active_and_never_started_idle_ephemeral_members() {
    check_restore_memory(false, true, true, false).await;
}

#[tokio::test]
async fn idle_ephemeral_refusal_retains_the_uninstalled_mail() {
    check_restore_memory(false, false, true, false).await;
}

#[tokio::test]
async fn restores_completed_idle_child_status() {
    check_restore_memory(false, true, false, true).await;
}

#[tokio::test]
async fn restores_completed_idle_ephemeral_child_status() {
    check_restore_memory(false, true, true, true).await;
}

#[tokio::test]
async fn restores_public_children_of_ephemeral_root() {
    check_restore_memory(true, true, true, true).await;
}

async fn check_restore(public: bool, idle_allowed: bool) {
    check_restore_memory(public, idle_allowed, false, false).await;
}

async fn check_restore_memory(
    public: bool,
    idle_allowed: bool,
    ephemeral: bool,
    completed_idle: bool,
) {
    let (home, config, manager, root) = fixture_options(public, public && ephemeral).await;
    let mut child_config = config.clone();
    child_config.ephemeral = ephemeral;
    let active = captured_child(&manager, &root, &child_config, public).await;
    let idle = captured_child(&manager, &root, &child_config, public).await;
    if completed_idle {
        let context = idle.session.new_default_turn().await;
        let release = Arc::new(tokio::sync::Notify::new());
        idle.session
            .spawn_task(
                Arc::clone(&context),
                Vec::new(),
                CompletedTask(Arc::clone(&release)),
            )
            .await;
        idle.session
            .emit_turn_started(&context, TaskKind::Regular)
            .await;
        release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if matches!(
                    idle.next_event().await.unwrap().msg,
                    codex_protocol::protocol::EventMsg::TurnComplete(_)
                ) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            idle.agent_status().await,
            codex_protocol::protocol::AgentStatus::Completed(Some(
                "original child completed".into()
            ))
        );
    }
    if !idle_allowed {
        idle.session
            .input_queue
            .enqueue_mailbox_communication(
                crate::PendingAgentMessage::from(
                    codex_protocol::protocol::InterAgentCommunication::new(
                        codex_protocol::AgentPath::root(),
                        codex_protocol::AgentPath::root(),
                        Vec::new(),
                        "retained until admitted".into(),
                        false,
                    ),
                ),
                Default::default(),
            )
            .await;
    }
    let child_turn = start(&active, TaskKind::Regular).await;
    let root_turn = start(&root, TaskKind::Regular).await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let captured = root
        .prepare_exec_tree(&root_turn, &invocation)
        .await
        .unwrap()
        .suspend_and_capture()
        .await
        .unwrap();
    let tree = ExecTreeSnapshot::capture(&captured).unwrap();
    let mut inputs = tree
        .inputs()
        .clone()
        .prepare(tree.root(), tree.turn())
        .unwrap();
    let fresh = ThreadManager::with_models_provider_home_and_state_for_tests(
        codex_login::CodexAuth::from_api_key("replacement"),
        config.model_provider.clone(),
        home.path().to_owned(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        crate::init_state_db(&config).await,
    );
    // Bootstrap uses model context, which retains the root's V2 metadata.
    // Latest-turn evidence deliberately omits that metadata in paginated mode.
    let history = if config.ephemeral {
        None
    } else {
        Some(
            root.session
                .services
                .thread_store
                .load_latest_model_context(codex_thread_store::LoadThreadHistoryParams {
                    thread_id: root.session.thread_id,
                    include_archived: false,
                })
                .await
                .unwrap(),
        )
    };
    let mut options = StartThreadOptions::new(config);
    if public {
        options.history_mode = Some(codex_protocol::protocol::ThreadHistoryMode::Paginated);
    }
    if let Some(history) = history {
        options.initial_history = InitialHistory::Resumed(ResumedHistory {
            conversation_id: tree.root(),
            history: Arc::new(history.items),
            history_revision: history.revision,
            rollout_path: None,
        });
    }
    if let Some(memory) = tree.threads()[0].ephemeral_history() {
        options = memory
            .clone()
            .prepare_start(options, tree.root(), tree.turn())
            .unwrap();
    }
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let options = inputs
        .prepare_start(options, tree.root(), tree.turn())
        .unwrap();
    let replacement = fresh.start_thread(options).await.unwrap().thread;
    let settings = replacement.restorable_thread_settings().await;
    let roots = replacement
        .session
        .services
        .selected_capability_roots
        .clone();
    assert!(
        replacement
            .prepare_exec_child_restore(
                &tree,
                ThreadId::new(),
                settings.clone(),
                roots.clone(),
                &mut inputs
            )
            .await
            .is_err()
    );
    let plan = replacement
        .prepare_exec_child_restore(
            &tree,
            active.session.thread_id,
            settings,
            roots,
            &mut inputs,
        )
        .await
        .unwrap();
    let parent_gate = Arc::clone(&replacement.residency_gate).write_owned().await;
    let mut cancelled_waiter = Box::pin(plan.start());
    assert!(futures::poll!(cancelled_waiter.as_mut()).is_pending());
    drop(cancelled_waiter);
    drop(parent_gate);
    let restored = plan
        .start()
        .await
        .unwrap_or_else(|error| panic!("child restore: {error}"));
    assert!(Arc::ptr_eq(&restored, &plan.start().await.unwrap()));
    if public {
        let source = active
            .session
            .services
            .local_agent_runtime
            .ensure_agent_known(active.session.thread_id)
            .unwrap();
        let target = replacement
            .session
            .services
            .local_agent_runtime
            .ensure_agent_known(active.session.thread_id)
            .unwrap();
        assert_eq!(source.agent_id, target.agent_id);
        assert_eq!(source.agent_path, target.agent_path);
        assert_eq!(source.agent_nickname, target.agent_nickname);
        assert_eq!(source.agent_role, target.agent_role);
        assert_eq!(
            restored.thread().multi_agent_version(),
            Some(MultiAgentVersion::V2)
        );
        assert!(
            Arc::clone(&restored.thread().residency_gate)
                .try_write_owned()
                .is_err()
        );
        restored.release_residency_pin();
        assert!(
            Arc::clone(&restored.thread().residency_gate)
                .try_write_owned()
                .is_ok()
        );
    }
    assert_eq!(
        restored.thread().session.thread_id,
        active.session.thread_id
    );
    assert_eq!(restored.recovery().unwrap().turn(), child_turn);
    assert_eq!(restored.thread().session.live_thread().is_none(), ephemeral);
    assert!(restored.thread().session.active_turn.lock().await.is_none());
    assert!(Arc::ptr_eq(
        &replacement.session.services.local_agent_runtime.registry,
        &restored
            .thread()
            .session
            .services
            .local_agent_runtime
            .registry
    ));
    assert!(!Arc::ptr_eq(
        &root.session.services.local_agent_runtime.registry,
        &restored
            .thread()
            .session
            .services
            .local_agent_runtime
            .registry
    ));
    assert!(Arc::ptr_eq(
        &replacement.session.services.auth_manager,
        &restored.thread().session.services.auth_manager
    ));
    assert!(!Arc::ptr_eq(
        &root.session.services.auth_manager,
        &restored.thread().session.services.auth_manager
    ));
    let node = tree
        .threads()
        .iter()
        .find(|node| node.thread() == active.session.thread_id)
        .unwrap();
    let current = restored.thread().exec_recovery_history().await.unwrap();
    // Plan::run checked these against its startup and final snapshots. Reuse
    // them here to isolate negative history checks from settings normalization.
    let settings = current
        .items
        .iter()
        .filter_map(|item| match item {
            codex_history::RolloutItem::EventMsg(
                codex_protocol::protocol::EventMsg::ThreadSettingsApplied(applied),
            ) => Some(applied.thread_settings.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    verify_loaded_history(node, &current, &settings).unwrap();
    let mut changed = current.clone();
    let codex_history::RolloutItem::EventMsg(codex_protocol::protocol::EventMsg::TurnStarted(
        start,
    )) = changed
        .items
        .iter_mut()
        .find(|item| {
            matches!(
                item,
                codex_history::RolloutItem::EventMsg(
                    codex_protocol::protocol::EventMsg::TurnStarted(_)
                )
            )
        })
        .unwrap()
    else {
        panic!("original start");
    };
    start.turn_id = "different-turn".into();
    assert!(verify_loaded_history(node, &changed, &settings,).is_err());
    let mut changed = current.clone();
    changed.items.push(codex_history::RolloutItem::EventMsg(
        codex_protocol::protocol::EventMsg::ShutdownComplete,
    ));
    assert!(verify_loaded_history(node, &changed, &settings,).is_err());
    let mut unadmitted = ExtensionDataInit::new();
    unadmitted.insert(restored.recovery().unwrap().clone());
    assert!(
        restored
            .thread()
            .recover_turn_if_idle(crate::WithTurnExtensionData::new(
                crate::RecoverTurnRequest {
                    turn_id: child_turn,
                    thread_settings: Default::default(),
                    trace: None,
                    prompt_cache_ttl: None,
                    cyber_access_program: None,
                },
                unadmitted
            ))
            .await
            .is_err()
    );
    assert!(restored.thread().session.active_turn.lock().await.is_none());
    let idle_plan = replacement
        .prepare_exec_child_restore(
            &tree,
            idle.session.thread_id,
            replacement.restorable_thread_settings().await,
            replacement
                .session
                .services
                .selected_capability_roots
                .clone(),
            &mut inputs,
        )
        .await
        .unwrap();
    let restored_idle = idle_plan.start().await.unwrap();
    assert!(restored_idle.recovery().is_none());
    if completed_idle {
        assert_eq!(
            restored_idle.thread().agent_status().await,
            codex_protocol::protocol::AgentStatus::Completed(Some(
                "original child completed".into()
            ))
        );
    }
    let launch_threads = vec![
        Arc::clone(&replacement),
        Arc::clone(restored.thread()),
        Arc::clone(restored_idle.thread()),
    ];
    let launch = crate::ExecLaunchBarrier::prepare(&tree, &launch_threads).unwrap();
    assert!(launch.permit(replacement.session.thread_id).is_ok());
    assert!(launch.permit(restored.thread().session.thread_id).is_ok());
    assert!(
        launch
            .permit(restored_idle.thread().session.thread_id)
            .is_err()
    );
    assert!(crate::ExecLaunchBarrier::prepare(&tree, &launch_threads[..2]).is_err());

    if idle_allowed {
        let admissions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&admissions);
        let expected = idle.session.thread_id;
        let (release, wait) = tokio::sync::oneshot::channel();
        let mut first_waiter = Box::pin(restored_idle.restore_idle_inputs(move |thread| {
            Box::pin(async move {
                assert_eq!(thread, expected);
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                wait.await
                    .map_err(|_| "admission owner disappeared".to_owned())?;
                Ok(())
            })
        }));
        assert!(futures::poll!(first_waiter.as_mut()).is_pending());
        drop(first_waiter);
        release.send(()).unwrap();
        restored_idle
            .restore_idle_inputs(|_| Box::pin(async { panic!("reconciliation must not readmit") }))
            .await
            .unwrap();
        assert_eq!(admissions.load(std::sync::atomic::Ordering::SeqCst), 1);
        if public {
            assert!(
                Arc::clone(&restored_idle.thread().residency_gate)
                    .try_write_owned()
                    .is_err()
            );
            restored_idle.release_residency_pin();
            assert!(
                Arc::clone(&restored_idle.thread().residency_gate)
                    .try_write_owned()
                    .is_ok()
            );
        }
    } else {
        assert!(
            restored_idle
                .restore_idle_inputs(|_| Box::pin(async { Err("native policy revoked".into()) }))
                .await
                .is_err()
        );
        assert!(
            !restored_idle
                .thread()
                .session
                .input_queue
                .has_pending_mailbox_items()
                .await
        );
        assert_eq!(
            restored_idle.idle.lock().unwrap().as_ref().unwrap().len(),
            1
        );
    }
    assert!(
        restored_idle
            .thread()
            .session
            .active_turn
            .lock()
            .await
            .is_none()
    );
    replacement.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn environment_restore_requires_current_owner_workspace_and_configuration() {
    let (_home, _config, _manager, root) = fixture().await;
    let settings = root.restorable_thread_settings().await;
    assert!(
        !settings
            .environments
            .as_ref()
            .unwrap()
            .environment_requests
            .is_empty()
    );
    bound_environments(&root, &mut settings.clone())
        .await
        .unwrap();
    let mut wrong_owner = settings.clone();
    wrong_owner
        .environments
        .as_mut()
        .unwrap()
        .environment_requests[0]
        .environment_id = "missing".into();
    assert!(bound_environments(&root, &mut wrong_owner).await.is_err());
    let mut changed = settings;
    bound_environments(&root, &mut changed).await.unwrap();
    let request = &mut changed.environments.as_mut().unwrap().environment_requests[0];
    let codex_protocol::protocol::EnvironmentConfigState::Ready(config) = &mut request.config
    else {
        panic!("fixture must carry a ready owner binding");
    };
    config.allow_login_shell = !config.allow_login_shell;
    assert!(bound_environments(&root, &mut changed).await.is_err());
    root.shutdown_and_wait().await.unwrap();
}
