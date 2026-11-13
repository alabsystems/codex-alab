// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::StartThreadOptions;
use crate::ThreadManager;
use crate::config::Config;
use crate::config::ConfigOverrides;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use codex_alab_core::quota::HeadlessExec;
use codex_protocol::AgentPath;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InterAgentCommunication;
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

async fn fixture() -> (tempfile::TempDir, Config, ThreadManager, Arc<CodexThread>) {
    fixture_with_restriction(false).await
}

async fn fixture_with_restriction(
    restricted: bool,
) -> (tempfile::TempDir, Config, ThreadManager, Arc<CodexThread>) {
    fixture_with_identity(restricted, None).await
}

async fn fixture_with_identity(
    restricted: bool,
    identity: Option<ThreadId>,
) -> (tempfile::TempDir, Config, ThreadManager, Arc<CodexThread>) {
    let home = tempfile::tempdir().unwrap();
    let config = crate::config::ConfigBuilder::without_managed_config_for_tests()
        .codex_home(home.path().to_owned())
        .harness_overrides(ConfigOverrides {
            model: Some("gpt-5.5".into()),
            cwd: Some(home.path().to_owned()),
            ..Default::default()
        })
        .build()
        .await
        .unwrap();
    let manager = ThreadManager::with_models_provider_and_home_for_tests(
        codex_login::CodexAuth::from_api_key("fixture"),
        config.model_provider.clone(),
        home.path().to_owned(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
    );
    let mut options = StartThreadOptions::new(config.clone());
    options.reserved_thread_id = identity;
    if restricted {
        options
            .thread_extension_init
            .insert(crate::RestrictRootCapabilities);
    }
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let root = manager.start_thread(options).await.unwrap().thread;
    (home, config, manager, root)
}

async fn child(manager: &ThreadManager, parent: &CodexThread, config: &Config) -> Arc<CodexThread> {
    let mut options = StartThreadOptions::new(config.clone());
    options.session_source = Some(SessionSource::Internal(
        InternalSessionSource::MemoryConsolidation,
    ));
    if !parent.session_source.is_internal() {
        return manager
            .spawn_internal_session(parent.session.thread_id, options)
            .await
            .unwrap()
            .thread;
    }
    // Internal children are hidden from get_thread. Capture the actual parent
    // so a nested private child uses the same supported inline-parent lifecycle.
    options.internal_parent = Some(crate::InternalSessionParent {
        assignment_parent: crate::session::alab_assignment::parent(&parent.session)
            .await
            .map(|activity| activity.downgrade()),
        thread_id: parent.session.thread_id,
        auth_manager: Arc::clone(&parent.session.services.auth_manager),
        agent_control: crate::agent::control::AgentControlInit::Provided {
            control: Arc::clone(&parent.session.services.agent_control),
            runtime: parent.session.services.local_agent_runtime.clone(),
        },
        originator: parent.config_snapshot().await.originator,
        inherited_instructions: None,
    });
    super::super::ephemeral::inherit_internal_owner(
        parent.thread_extension_data(),
        &mut options.thread_extension_init,
    );
    manager.start_thread(options).await.unwrap().thread
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
async fn captures_an_active_ephemeral_quota_member_without_root_admission() {
    let (_home, config, manager, root) = fixture().await;
    let mut child_config = config.clone();
    child_config.ephemeral = true;
    let mut options = StartThreadOptions::new(child_config);
    options.session_source = Some(SessionSource::Internal(
        InternalSessionSource::MemoryConsolidation,
    ));
    options
        .thread_extension_init
        .insert(codex_alab_core::quota::ExecQuotaMember);
    let child = manager
        .spawn_internal_session(root.session.thread_id, options)
        .await
        .unwrap()
        .thread;
    assert!(child.session.live_thread().is_none());
    assert!(
        child
            .thread_extension_data()
            .get::<HeadlessExec>()
            .is_none()
    );
    let child_turn = start(&child, TaskKind::Regular).await;
    // The original event must already be in memory, before suspension marks
    // the child for restoration. Missing evidence must never be synthesized.
    let original = child.exec_recovery_history().await.unwrap();
    assert_eq!(
        super::super::ephemeral::open_turn(&original.items),
        Some(child_turn.as_str())
    );
    let root_turn = start(&root, TaskKind::Regular).await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let captured = root
        .prepare_exec_tree(&root_turn, &invocation)
        .await
        .unwrap()
        .suspend_and_capture()
        .await
        .unwrap();
    let node = captured
        .threads()
        .iter()
        .find(|node| node.thread() == child.session.thread_id)
        .unwrap();
    assert_eq!(node.recovery().unwrap().turn(), child_turn);
    let memory = node
        .ephemeral_history()
        .cloned()
        .expect("child memory history");
    let restored = memory
        .restore(child.session.thread_id, &child_turn)
        .unwrap();
    assert_eq!(
        super::super::ephemeral::open_turn(&restored),
        Some(child_turn.as_str())
    );
    assert!(!restored.iter().any(|item| matches!(
        item,
        codex_history::RolloutItem::EventMsg(EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
    )));
    assert!(!child.is_running());
    assert!(!root.is_running());
}

#[tokio::test]
async fn captures_active_and_idle_children_after_all_writers_stop() {
    let (_home, config, manager, root) = fixture().await;
    let active = child(&manager, &root, &config).await;
    let grandchild = child(&manager, &active, &config).await;
    let idle = child(&manager, &root, &config).await;
    let foreign = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .unwrap()
        .thread;
    let root_turn = start(&root, TaskKind::Regular).await;
    let child_turn = start(&active, TaskKind::Regular).await;
    let grandchild_turn = start(&grandchild, TaskKind::Regular).await;
    start(&foreign, TaskKind::Regular).await;
    idle.session
        .input_queue
        .enqueue_mailbox_communication(
            crate::PendingAgentMessage::from(InterAgentCommunication::new(
                AgentPath::root(),
                AgentPath::root(),
                Vec::new(),
                "idle mail".into(),
                false,
            )),
            Default::default(),
        )
        .await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let plan = root
        .prepare_exec_tree(&root_turn, &invocation)
        .await
        .unwrap();
    active
        .update_thread_settings(codex_protocol::protocol::ThreadSettingsOverrides {
            disabled_plugin_ids: Some(vec!["accepted-after-preflight".into()]),
            ..Default::default()
        })
        .await
        .unwrap();
    let mut first_waiter = Box::pin(plan.suspend_and_capture());
    assert!(futures::poll!(first_waiter.as_mut()).is_pending());
    drop(first_waiter);
    let captured = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        plan.suspend_and_capture(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(captured.root(), root.session.thread_id);
    assert_eq!(captured.turn(), root_turn);
    assert_eq!(captured.threads().len(), 4);
    let wire = super::super::ExecTreeSnapshot::capture(&captured).unwrap();
    let wire: super::super::ExecTreeSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
    wire.validate(captured.root(), Some(captured.turn()))
        .unwrap();
    for node in wire.threads() {
        let source = captured
            .threads()
            .iter()
            .find(|source| source.thread() == node.thread())
            .unwrap();
        node.verify_history(source.history()).unwrap();
        let settings = node
            .restore_settings(source.settings(), source.capability_roots())
            .unwrap();
        assert_eq!(
            settings.disabled_plugin_ids,
            source.settings().disabled_plugin_ids
        );
    }
    let mut altered = serde_json::to_value(&wire).unwrap();
    altered["threads"][1]["parent"] = serde_json::json!(ThreadId::new());
    let altered: super::super::ExecTreeSnapshot = serde_json::from_value(altered).unwrap();
    assert!(
        altered
            .validate(captured.root(), Some(captured.turn()))
            .is_err()
    );
    let mut altered = serde_json::to_value(&wire).unwrap();
    altered["threads"].as_array_mut().unwrap().pop();
    let altered: super::super::ExecTreeSnapshot = serde_json::from_value(altered).unwrap();
    assert!(
        altered
            .validate(captured.root(), Some(captured.turn()))
            .is_err()
    );

    assert!(Arc::ptr_eq(
        &captured,
        &plan.suspend_and_capture().await.unwrap()
    ));
    let child_record = captured
        .threads()
        .iter()
        .find(|item| item.thread() == active.session.thread_id)
        .unwrap();
    assert_eq!(child_record.parent(), Some(root.session.thread_id));
    assert_eq!(child_record.recovery().unwrap().turn(), child_turn);
    let grandchild_record = captured
        .threads()
        .iter()
        .find(|item| item.thread() == grandchild.session.thread_id)
        .unwrap();
    assert_eq!(grandchild_record.parent(), Some(active.session.thread_id));
    assert_eq!(
        grandchild_record.recovery().unwrap().turn(),
        grandchild_turn
    );
    assert_eq!(
        child_record.settings().disabled_plugin_ids.as_deref(),
        Some(["accepted-after-preflight".to_owned()].as_slice())
    );
    for record in captured
        .threads()
        .iter()
        .filter(|record| record.recovery().is_some())
    {
        assert!(!record.history().items.iter().any(|item| matches!(
            item,
            codex_history::RolloutItem::EventMsg(
                EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_)
            )
        )));
    }
    let idle_record = captured
        .threads()
        .iter()
        .find(|item| item.thread() == idle.session.thread_id)
        .unwrap();
    assert!(idle_record.recovery().is_none());
    let mut inputs = captured
        .inputs()
        .clone()
        .prepare(captured.root(), captured.turn())
        .unwrap();
    assert_eq!(inputs.take_idle(idle.session.thread_id).unwrap().len(), 1);
    assert_eq!(inputs.len(), 3);
    assert!(!root.is_running());
    assert!(!active.is_running());
    assert!(!grandchild.is_running());
    assert!(!idle.is_running());
    assert!(foreign.is_running());
    foreign.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn unsupported_child_refuses_before_any_turn_is_stopped() {
    let (_home, config, manager, root) = fixture().await;
    let child = child(&manager, &root, &config).await;
    let root_turn = start(&root, TaskKind::Regular).await;
    start(&child, TaskKind::Compact).await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    assert!(
        root.prepare_exec_tree(&root_turn, &invocation)
            .await
            .is_err()
    );
    assert!(
        root.session
            .active_turn
            .lock()
            .await
            .as_ref()
            .unwrap()
            .task
            .as_ref()
            .is_some_and(|task| !task.cancellation_token.is_cancelled())
    );
    assert!(
        child
            .session
            .active_turn
            .lock()
            .await
            .as_ref()
            .unwrap()
            .task
            .as_ref()
            .is_some_and(|task| !task.cancellation_token.is_cancelled())
    );
    // The unsuccessful preflight dropped its uncommitted barrier.
    let seal = root.seal_exec_tree_starts().await.unwrap();
    drop(seal);
    root.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn dropping_an_unstarted_plan_reopens_admission_without_stopping_the_source() {
    let (_home, config, _manager, root) = fixture().await;
    let turn = start(&root, TaskKind::Regular).await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let plan = root.prepare_exec_tree(&turn, &invocation).await.unwrap();
    assert!(root.seal_exec_tree_starts().await.is_err());
    drop(plan);
    let seal = root.seal_exec_tree_starts().await.unwrap();
    drop(seal);
    assert!(
        root.session
            .active_turn
            .lock()
            .await
            .as_ref()
            .unwrap()
            .task
            .as_ref()
            .is_some_and(|task| !task.cancellation_token.is_cancelled())
    );
    root.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn ephemeral_transport_rejects_conflicting_history_evidence() {
    let (_home, mut config, manager, original) = fixture().await;
    config.ephemeral = true;
    let mut options = StartThreadOptions::new(config.clone());
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let root = manager.start_thread(options).await.unwrap().thread;
    let turn = start(&root, TaskKind::Regular).await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let plan = root.prepare_exec_tree(&turn, &invocation).await.unwrap();
    let captured = plan.suspend_and_capture().await.unwrap();
    let wire = super::super::ExecTreeSnapshot::capture(&captured).unwrap();
    assert!(wire.threads()[0].ephemeral_history().is_some());
    let mut altered = serde_json::to_value(&wire).unwrap();
    altered["threads"][0]["history"]["records"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "kind": "inter_agent_communication_metadata", "value": {"trigger_turn": true}
        }));
    let altered: super::super::ExecTreeSnapshot = serde_json::from_value(altered).unwrap();
    assert!(altered.validate(wire.root(), wire.turn()).is_err());
    original.shutdown_and_wait().await.unwrap();
}

struct ReviewCaptureTask {
    ready: Mutex<Option<tokio::sync::oneshot::Sender<(ThreadId, String)>>>,
    finish: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}

struct CompletingDelegate(Mutex<Option<tokio::sync::oneshot::Receiver<()>>>);
impl SessionTask for CompletingDelegate {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }
    fn span_name(&self) -> &'static str {
        "completing_review_delegate"
    }
    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _context: Arc<TurnContext>,
        _input: Vec<crate::session::input_queue::TurnInput>,
        token: CancellationToken,
    ) -> SessionTaskResult {
        let finish = self.0.lock().unwrap().take().unwrap();
        tokio::select! {
            _ = token.cancelled() => Ok(None),
            result = finish => {
                result.unwrap();
                Ok(Some(crate::tasks::alab_test_output::completed(serde_json::json!({"findings":[],"overall_correctness":"correct","overall_explanation":"completed while frozen","overall_confidence_score":1.0}).to_string())))
            }
        }
    }
}
impl SessionTask for ReviewCaptureTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Review
    }
    fn span_name(&self) -> &'static str {
        "review_capture_test"
    }
    async fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        context: Arc<TurnContext>,
        _input: Vec<crate::session::input_queue::TurnInput>,
        token: CancellationToken,
    ) -> SessionTaskResult {
        let _guard = crate::tasks::review_recovery::begin(&session, &context, &token);
        let mut config = (*context.config).clone();
        config.permissions.approval_policy =
            crate::config::Constrained::allow_only(codex_protocol::protocol::AskForApproval::Never);
        let (child, io) = crate::codex_delegate::run_codex_thread_interactive(
            config,
            Arc::clone(&session.services.auth_manager),
            Arc::clone(&session.services.models_manager),
            Arc::clone(&session),
            Arc::clone(&context),
            context.initial_environments.clone(),
            token.clone(),
            codex_protocol::protocol::SubAgentSource::Review,
            codex_extension_api::SessionIsolation::Inherit,
            None,
            crate::session::GitEnrichmentPolicy::Fresh,
            codex_sandboxing::WindowsSandboxProxySettingsMode::Reconcile,
        )
        .await?;
        let child_context = child.new_default_turn().await;
        let child_turn = child_context.sub_id.clone();
        let finish = self.finish.lock().unwrap().take();
        if let Some(finish) = finish {
            child
                .spawn_task(
                    Arc::clone(&child_context),
                    Vec::new(),
                    CompletingDelegate(Mutex::new(Some(finish))),
                )
                .await;
        } else {
            child
                .spawn_task(
                    Arc::clone(&child_context),
                    Vec::new(),
                    WaitingTask(TaskKind::Regular),
                )
                .await;
        }
        child
            .emit_turn_started(&child_context, TaskKind::Regular)
            .await;
        child
            .send_event(
                &child_context,
                EventMsg::AgentMessage(codex_protocol::protocol::AgentMessageEvent {
                    moderation: None,
                    message: "retained review work".into(),
                    phase: None,
                    memory_citation: None,
                    delivery: None,
                    questions: None,
                }),
            )
            .await;
        if let Some(ready) = self.ready.lock().unwrap().take() {
            let _ = ready.send((child.thread_id, child_turn));
        }
        crate::tasks::review_recovery::process_events(session, context, io.rx_event).await;
        Ok(None)
    }
}

#[tokio::test]
async fn captures_review_parent_and_original_delegate_with_consumer_state() {
    capture_review(false).await;
}

#[tokio::test]
async fn captures_review_result_when_delegate_completes_after_freeze() {
    capture_review(true).await;
}

async fn capture_review(completes: bool) {
    let (_home, config, _manager, root) = fixture().await;
    let (finish, finished) = tokio::sync::oneshot::channel();
    let context = root.session.new_default_turn().await;
    context.extension_data.insert(HeadlessExec);
    let turn = context.sub_id.clone();
    root.session
        .emit_turn_item_completed(
            &context,
            codex_protocol::items::TurnItem::EnteredReviewMode(
                codex_protocol::items::EnteredReviewModeItem {
                    id: "original-review-entry".into(),
                    target: codex_protocol::protocol::ReviewTarget::Custom {
                        instructions: "review".into(),
                    },
                    user_facing_hint: "Review".into(),
                },
            ),
        )
        .await;
    let (ready, receive) = tokio::sync::oneshot::channel();
    root.session
        .spawn_task(
            Arc::clone(&context),
            Vec::new(),
            ReviewCaptureTask {
                ready: Mutex::new(Some(ready)),
                finish: Mutex::new(completes.then_some(finished)),
            },
        )
        .await;
    let (delegate, delegate_turn) =
        tokio::time::timeout(std::time::Duration::from_secs(10), receive)
            .await
            .unwrap()
            .unwrap();
    let invocation = ExecConfiguration::capture(&config, &Default::default()).unwrap();
    let inventory = root.seal_exec_tree_starts().await.unwrap();
    let child = inventory
        .loaded_threads(&root)
        .await
        .unwrap()
        .into_iter()
        .find(|thread| thread.exec_thread_id() == delegate)
        .unwrap();
    drop(inventory);
    let plan = root.prepare_exec_tree(&turn, &invocation).await.unwrap();
    if completes {
        finish.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                child.flush_rollout().await.unwrap();
                if child.exec_recovery_history().await.unwrap().items.iter().any(|item| matches!(item,
                    codex_history::RolloutItem::EventMsg(EventMsg::TurnComplete(event)) if event.turn_id == delegate_turn)) { break; }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
    }
    let captured = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        plan.suspend_and_capture(),
    )
    .await
    .unwrap()
    .unwrap();
    let wire = super::super::ExecTreeSnapshot::capture(&captured).unwrap();
    wire.validate(root.session.thread_id, Some(&turn)).unwrap();
    assert_eq!(wire.threads().len(), 2);
    let parent = wire
        .threads()
        .iter()
        .find(|node| node.thread() == root.session.thread_id)
        .unwrap();
    assert!(parent.recovery().unwrap().is_review());
    let review = parent.review().unwrap();
    assert_eq!(review.delegate(), delegate);
    assert_eq!(review.delegate_turn(), delegate_turn);
    let encoded = serde_json::to_value(review).unwrap();
    assert!(encoded.to_string().contains("retained review work"));
    assert!(!super::super::ephemeral::open_turn(&parent.history().unwrap().items).is_none());
    for node in wire.threads() {
        if completes && node.thread() == delegate {
            assert!(node.recovery().is_none());
            continue;
        }
        assert!(!node.history().unwrap().items.iter().any(|item| matches!(
            item,
            codex_history::RolloutItem::EventMsg(
                EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_)
            )
        )));
    }
    let mut changed = serde_json::to_value(&wire).unwrap();
    changed["threads"][0]["review"] = serde_json::Value::Null;
    let changed: super::super::ExecTreeSnapshot = serde_json::from_value(changed).unwrap();
    assert!(
        changed
            .validate(root.session.thread_id, Some(&turn))
            .is_err()
    );
}

#[tokio::test]
async fn captured_root_restrictions_survive_startup_without_relaxing_fresh_host_policy() {
    for source_restricted in [false, true] {
        let (_home, config, manager, root) = fixture_with_restriction(source_restricted).await;
        assert!(!root.session_source.is_non_root_agent());
        let turn = start(&root, TaskKind::Regular).await;
        let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
        let captured = root
            .prepare_exec_tree(&turn, &invocation)
            .await
            .unwrap()
            .suspend_and_capture()
            .await
            .unwrap();
        assert_eq!(
            captured.threads()[0].root_capabilities_restricted(),
            source_restricted
        );
        let wire = super::super::ExecTreeSnapshot::capture(&captured).unwrap();
        let encoded = serde_json::to_value(&wire).unwrap();
        assert_eq!(encoded["version"], 4);
        for version in [2, 3] {
            let mut old = encoded.clone();
            old["version"] = serde_json::json!(version);
            let old: super::super::ExecTreeSnapshot = serde_json::from_value(old).unwrap();
            assert!(
                old.validate(captured.root(), Some(captured.turn()))
                    .is_err()
            );
        }
        let mut missing = encoded;
        missing["threads"][0]
            .as_object_mut()
            .unwrap()
            .remove("root_capabilities_restricted");
        assert!(serde_json::from_value::<super::super::ExecTreeSnapshot>(missing).is_err());
        let wire: super::super::ExecTreeSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
        for fresh_restricted in [false, true] {
            let mut options = StartThreadOptions::new(config.clone());
            if fresh_restricted {
                options
                    .thread_extension_init
                    .insert(crate::RestrictRootCapabilities);
            }
            wire.threads()[0].restrict_root_capabilities(&mut options.thread_extension_init);
            let restored = manager.start_thread(options).await.unwrap().thread;
            assert!(!restored.session_source.is_non_root_agent());
            let turn = restored.session.new_default_turn().await;
            assert_eq!(
                turn.root_capabilities_restricted,
                source_restricted || fresh_restricted
            );
            assert_eq!(
                restored
                    .session
                    .state
                    .lock()
                    .await
                    .session_configuration
                    .root_capabilities_restricted,
                source_restricted || fresh_restricted
            );
            restored.shutdown_exec_tree().await.unwrap();
        }
        root.shutdown_exec_tree().await.unwrap();
    }
}

#[test_case::test_case(false, false; "ordinary thread-owned history")]
#[test_case::test_case(true, false; "checkpoint thread-owned history")]
#[test_case::test_case(false, true; "ordinary independent review")]
#[test_case::test_case(true, true; "checkpoint independent review")]
#[tokio::test]
async fn ephemeral_restore_keeps_fresh_restrictions_on_inherited_authorization(
    checkpointed: bool,
    independent: bool,
) {
    use codex_history::CodexHarnessMetadata;
    use codex_history::InitialHistory;
    use codex_history::ResponseItemEnvelope;
    use codex_history::RetainedContextEntry;
    use codex_history::RolloutItem;
    use core_test_support::responses::user_message_item;

    let (_home, mut config, manager, original) = fixture().await;
    config.ephemeral = true;
    if independent {
        config
            .features
            .disable(codex_features::Feature::GuardianReuseParentCompaction)
            .unwrap();
    } else {
        config
            .features
            .enable(codex_features::Feature::GuardianReuseParentCompaction)
            .unwrap();
    }
    let mut options = StartThreadOptions::new(config.clone());
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    options.initial_history =
        InitialHistory::Forked(vec![RolloutItem::ResponseItem(ResponseItemEnvelope {
            item: user_message_item("inherited authorization"),
            metadata: Some(CodexHarnessMetadata {
                inherited_user_message: true,
                ..Default::default()
            }),
        })]);
    let root = manager.start_thread(options).await.unwrap().thread;
    let context = root.session.new_default_turn().await;
    root.session
        .record_conversation_items(
            &context,
            context.model_info(),
            &[user_message_item("local authorization")],
        )
        .await;
    if checkpointed {
        // Use Core's real checkpoint producer and the same journal path used by
        // compaction. The checkpoint carries adopted authorization independently
        // of the model window, so startup replay must enforce fresh policy too.
        let checkpoint = root.session.guardian_fork_history().await;
        assert!(
            checkpoint
                .iter()
                .any(|item| matches!(item, RolloutItem::Compacted(_)))
        );
        assert!(root.session.persist_rollout_items(&checkpoint).await);
    }
    let turn = start(&root, TaskKind::Regular).await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let captured = root
        .prepare_exec_tree(&turn, &invocation)
        .await
        .unwrap()
        .suspend_and_capture()
        .await
        .unwrap();
    let wire = super::super::ExecTreeSnapshot::capture(&captured).unwrap();
    let memory = wire.threads()[0].ephemeral_history().unwrap();
    root.shutdown_and_wait().await.unwrap();
    manager.remove_thread(&root.session.thread_id).await;
    for restricted in [false, true] {
        let mut options = StartThreadOptions::new(config.clone());
        if restricted {
            options
                .thread_extension_init
                .insert(crate::RestrictRootCapabilities);
        }
        let options = memory
            .clone()
            .prepare_start(options, wire.root(), wire.turn().unwrap())
            .unwrap();
        let resumed = manager.start_thread(options).await.unwrap().thread;
        {
            let state = resumed.session.state.lock().await;
            assert_eq!(
                state.session_configuration.root_capabilities_restricted,
                restricted
            );
            let instructions = state
                .history
                .retained_context()
                .ordered_entries()
                .filter_map(|(_, entry)| match entry {
                    RetainedContextEntry::UserMessage(message) => Some(message.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(
                instructions.contains(&"local authorization"),
                "{instructions:?}"
            );
            assert_eq!(
                instructions.contains(&"inherited authorization"),
                !restricted,
                "restored inherited authorization must follow the fresh host: {instructions:?}"
            );
            if independent {
                let legacy = state
                    .history
                    .original_legacy_user_messages()
                    .collect::<Vec<_>>();
                assert!(
                    legacy
                        .iter()
                        .any(|message| message.text == "local authorization"),
                    "restriction preserves local independent-review evidence"
                );
                assert_eq!(
                    legacy
                        .iter()
                        .any(|message| message.text == "inherited authorization"),
                    !restricted,
                    "independent-review authorization follows fresh policy"
                );
            }
            if restricted {
                if let Some(checkpoint) = state.history.guardian_history_checkpoint() {
                    assert!(
                        checkpoint.0.iter().all(|entry| !entry
                            .metadata
                            .as_ref()
                            .is_some_and(|metadata| metadata.inherited_user_message)),
                        "raw independent-review history must also exclude the inherited prefix"
                    );
                }
                assert!(
                    !state
                        .history
                        .original_legacy_user_messages()
                        .any(|message| message.text == "inherited authorization"),
                    "fresh restrictions must also hold in the legacy authorization projection"
                );
            }
        }
        resumed.shutdown_and_wait().await.unwrap();
        manager.remove_thread(&wire.root()).await;
    }
    original.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn upgrade_manager_inventory_includes_other_roots_and_private_ephemeral_members() {
    let (_home, config, manager, root) = fixture().await;
    let other = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .unwrap()
        .thread;
    assert!(
        other
            .thread_extension_data()
            .get::<HeadlessExec>()
            .is_none()
    );
    let mut ephemeral = config.clone();
    ephemeral.ephemeral = true;
    let private = child(&manager, &root, &ephemeral).await;
    let mut barrier = other.prepare_upgrade_manager().await.unwrap();
    let ids = barrier
        .threads()
        .iter()
        .map(|thread| thread.session.thread_id)
        .collect::<HashSet<_>>();
    assert_eq!(
        ids,
        HashSet::from([
            root.session.thread_id,
            other.session.thread_id,
            private.session.thread_id
        ])
    );
    let admission = private
        .session
        .services
        .local_agent_runtime
        .admit_exec_tree_start()
        .unwrap();
    assert!(!barrier.valid());
    assert!(!barrier.commit_if(|| panic!("private startup revoked this inventory")));
    drop(admission);
    drop(barrier);
    let mut next = root.prepare_upgrade_manager().await.unwrap();
    assert!(!next.commit_if(|| false));
    assert!(next.valid());
    drop(next);
    let mut input_barrier = root.prepare_upgrade_manager().await.unwrap();
    other
        .submit(codex_protocol::protocol::Op::Interrupt)
        .await
        .unwrap();
    assert!(
        !input_barrier
            .commit_if(|| panic!("accepted input on a different root revoked the handoff"))
    );
    drop(input_barrier);
    let mut committed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match root.prepare_upgrade_manager().await {
                Ok(barrier) => break barrier,
                Err(_) => tokio::task::yield_now().await,
            }
        }
    })
    .await
    .expect("accepted interrupt dispatch drains");
    assert!(committed.commit_if(|| true));
    drop(committed);
    assert!(
        other
            .submit(codex_protocol::protocol::Op::Interrupt)
            .await
            .is_err()
    );
    assert!(
        private
            .session
            .services
            .local_agent_runtime
            .admit_exec_tree_start()
            .is_err()
    );
    // Teardown remains callable for every root after the successful commit.
    other.shutdown_and_wait().await.unwrap();
    root.shutdown_exec_tree().await.unwrap();
}

#[tokio::test]
async fn committed_idle_root_captures_loaded_and_late_evicted_mail_without_a_root_turn() {
    let (_home, config, manager, root) = fixture().await;
    let child = child(&manager, &root, &config).await;
    let unloaded = ThreadId::new();
    let mail = |text: &str| {
        crate::PendingAgentMessage::from(InterAgentCommunication::new(
            AgentPath::root(),
            AgentPath::root(),
            Vec::new(),
            text.into(),
            false,
        ))
    };
    root.session
        .input_queue
        .enqueue_mailbox_communication(mail("root-local"), Default::default())
        .await;
    child
        .session
        .input_queue
        .enqueue_mailbox_communication(mail("child-local"), Default::default())
        .await;
    let runtime = &root.session.services.local_agent_runtime;
    runtime
        .retain_exec_evicted_mail(unloaded, vec![mail("unloaded-before-seal")])
        .unwrap();
    let mut prepared = root.prepare_upgrade_manager().await.unwrap();
    root.validate_upgrade_idle_inputs(&prepared).await.unwrap();
    child.validate_upgrade_idle_inputs(&prepared).await.unwrap();
    assert!(prepared.commit_if(|| true));
    let committed = prepared.into_committed().unwrap();
    root.seal_upgrade_idle_inputs(&committed).await.unwrap();
    child.seal_upgrade_idle_inputs(&committed).await.unwrap();
    root.seal_upgrade_idle_inputs(&committed).await.unwrap();
    assert!(
        ExecInputBatch::capture_idle_root(
            root.exec_thread_id(),
            &[],
            &[root.as_ref(), child.as_ref()]
        )
        .await
        .is_err()
    );
    // An eviction accepted before commitment may settle after controller sealing.
    runtime
        .retain_exec_evicted_mail(unloaded, vec![mail("evicted-after-seal")])
        .unwrap();
    root.shutdown_exec_tree().await.unwrap();
    let batch = ExecInputBatch::capture_idle_root(
        root.exec_thread_id(),
        &[],
        &[root.as_ref(), child.as_ref()],
    )
    .await
    .unwrap();
    let json = serde_json::to_value(&batch).unwrap();
    assert!(json["frames"].as_array().unwrap().is_empty());
    assert_eq!(
        json["idle"][0]["controller_routes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let decoded: ExecInputBatch = serde_json::from_value(json).unwrap();
    let mut inputs = decoded.prepare_idle(root.exec_thread_id()).unwrap();
    assert_eq!(inputs.take_idle(root.exec_thread_id()).unwrap().len(), 3);
    assert_eq!(inputs.take_idle(child.exec_thread_id()).unwrap().len(), 1);
    assert!(inputs.is_empty());
    assert_eq!(
        committed.threads().len(),
        2,
        "source handles survive registry retirement"
    );
}

async fn idle_root_frame(identity: ThreadId) -> super::super::PreparedExecIdleInput {
    let (_home, _config, _manager, source) = fixture_with_identity(false, Some(identity)).await;
    source
        .session
        .services
        .local_agent_runtime
        .retain_exec_evicted_mail(
            ThreadId::new(),
            vec![crate::PendingAgentMessage::from(
                InterAgentCommunication::new(
                    AgentPath::root(),
                    AgentPath::root(),
                    Vec::new(),
                    "retained root controller mail".into(),
                    false,
                ),
            )],
        )
        .unwrap();
    let mut prepared = source.prepare_upgrade_manager().await.unwrap();
    source
        .validate_upgrade_idle_inputs(&prepared)
        .await
        .unwrap();
    assert!(prepared.commit_if(|| true));
    let committed = prepared.into_committed().unwrap();
    source.seal_upgrade_idle_inputs(&committed).await.unwrap();
    source.shutdown_exec_tree().await.unwrap();
    ExecInputBatch::capture_idle_root(identity, &[], &[source.as_ref()])
        .await
        .unwrap()
        .prepare_idle(identity)
        .unwrap()
        .take_idle(identity)
        .unwrap()
}

#[tokio::test]
async fn idle_root_restore_survives_cancelled_waiter_and_admits_once_before_activation() {
    let (_home, _config, _manager, root) = fixture().await;
    let input = idle_root_frame(root.exec_thread_id()).await;
    let owner = root.prepare_exec_idle_root_restore(input).unwrap();
    assert!(owner.activate().await.is_err());
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (entered, entered_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let waiter_owner = Arc::clone(&owner);
    let count = Arc::clone(&calls);
    let expected = root.exec_thread_id();
    let waiter = tokio::spawn(async move {
        waiter_owner
            .restore(move |thread| {
                Box::pin(async move {
                    assert_eq!(thread, expected);
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    entered.send(()).unwrap();
                    release_rx.await.unwrap();
                    Ok(())
                })
            })
            .await
    });
    entered_rx.await.unwrap();
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert!(
        root.session
            .services
            .local_agent_runtime
            .exec_mailboxes()
            .unwrap()
            .is_empty()
    );
    assert!(owner.activate().await.is_err());
    release.send(()).unwrap();
    owner
        .restore(|_| panic!("retry must reconcile original admission"))
        .await
        .unwrap();
    owner
        .restore(|_| panic!("completed installation must not admit again"))
        .await
        .unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        root.session
            .services
            .local_agent_runtime
            .exec_mailboxes()
            .unwrap()
            .len(),
        1
    );
    assert!(root.session.active_turn.lock().await.is_none());
    owner.activate().await.unwrap();
    owner.activate().await.unwrap();
    root.shutdown_exec_tree().await.unwrap();
}

#[tokio::test]
async fn idle_root_restore_retains_native_refusal_and_rejects_wrong_or_retired_owner() {
    let (_home, config, manager, root) = fixture().await;
    let wrong = idle_root_frame(ThreadId::new()).await;
    assert!(root.prepare_exec_idle_root_restore(wrong).is_err());
    let descendant = child(&manager, &root, &config).await;
    let child_input = idle_root_frame(descendant.exec_thread_id()).await;
    assert!(
        descendant
            .prepare_exec_idle_root_restore(child_input)
            .is_err()
    );
    let input = idle_root_frame(root.exec_thread_id()).await;
    let refused = root.prepare_exec_idle_root_restore(input).unwrap();
    let error = refused
        .restore(|_| Box::pin(async { Err("native host refused".into()) }))
        .await
        .unwrap_err();
    assert_eq!(error, "native host refused");
    assert_eq!(
        refused
            .restore(|_| panic!("refusal is final for this attempt"))
            .await
            .unwrap_err(),
        error
    );
    assert!(refused.activate().await.is_err());
    assert!(
        root.session
            .services
            .local_agent_runtime
            .exec_mailboxes()
            .unwrap()
            .is_empty()
    );
    let input = idle_root_frame(root.exec_thread_id()).await;
    let retired = root.prepare_exec_idle_root_restore(input).unwrap();
    root.shutdown_exec_tree().await.unwrap();
    assert!(
        retired
            .restore(|_| panic!("retired native member cannot request admission"))
            .await
            .is_err()
    );
    assert!(retired.activate().await.is_err());
}

#[tokio::test]
async fn idle_root_restore_rechecks_retirement_after_native_admission_wait() {
    let (_home, _config, _manager, root) = fixture().await;
    let input = idle_root_frame(root.exec_thread_id()).await;
    let owner = root.prepare_exec_idle_root_restore(input).unwrap();
    let (entered, entered_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let waiter = tokio::spawn({
        let owner = Arc::clone(&owner);
        async move {
            owner
                .restore(move |_| {
                    Box::pin(async move {
                        entered.send(()).unwrap();
                        release_rx.await.unwrap();
                        Ok(())
                    })
                })
                .await
        }
    });
    entered_rx.await.unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        root.shutdown_exec_tree(),
    )
    .await
    .unwrap()
    .unwrap();
    release.send(()).unwrap();
    assert_eq!(
        waiter.await.unwrap().unwrap_err(),
        "idle input owner retired during admission"
    );
    assert!(owner.activate().await.is_err());
}

#[tokio::test]
async fn idle_tree_snapshot_preserves_history_private_members_and_distinct_root_ownership() {
    let (_home, config, manager, root) = fixture().await;
    let descendant = child(&manager, &root, &config).await;
    let other = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .unwrap()
        .thread;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let mut prepared = root.prepare_upgrade_manager().await.unwrap();
    for member in prepared.threads() {
        member
            .validate_upgrade_idle_inputs(&prepared)
            .await
            .unwrap();
    }
    assert!(prepared.commit_if(|| true));
    let inventory = prepared.into_committed().unwrap();
    for member in inventory.threads() {
        member.seal_upgrade_idle_inputs(&inventory).await.unwrap();
    }
    assert!(
        super::super::ExecTreeSnapshot::capture_idle(&root, &inventory, &invocation)
            .await
            .is_err()
    );
    root.shutdown_exec_tree().await.unwrap();
    other.shutdown_and_wait().await.unwrap();
    assert!(
        super::super::ExecTreeSnapshot::capture_idle(&other, &inventory, &invocation)
            .await
            .is_err()
    );
    other.shutdown_exec_tree().await.unwrap();
    let tree = super::super::ExecTreeSnapshot::capture_idle(&root, &inventory, &invocation)
        .await
        .unwrap();
    assert_eq!(tree.turn(), None);
    tree.validate(root.exec_thread_id(), None).unwrap();
    assert!(
        tree.validate(root.exec_thread_id(), Some("invented-root-turn"))
            .is_err()
    );
    assert_eq!(tree.threads().len(), 2);
    assert_eq!(tree.threads()[0].thread(), root.exec_thread_id());
    let child = &tree.threads()[1];
    assert_eq!(child.thread(), descendant.exec_thread_id());
    assert_eq!(child.parent(), Some(root.exec_thread_id()));
    assert_eq!(
        child.ephemeral_history().is_some(),
        descendant.config_snapshot().await.ephemeral
    );
    for node in tree.threads() {
        assert!(node.recovery().is_none());
        assert!(super::super::ephemeral::open_turn(&node.history().unwrap().items).is_none());
    }
    let other_tree = super::super::ExecTreeSnapshot::capture_idle(&other, &inventory, &invocation)
        .await
        .unwrap();
    assert_eq!(other_tree.threads().len(), 1);
    assert_eq!(other_tree.threads()[0].thread(), other.exec_thread_id());
    let mut changed = serde_json::to_value(&tree).unwrap();
    changed["turn"] = serde_json::json!("invented-root-turn");
    let changed: super::super::ExecTreeSnapshot = serde_json::from_value(changed).unwrap();
    assert!(
        changed
            .validate(root.exec_thread_id(), Some("invented-root-turn"))
            .is_err()
    );
}

#[tokio::test]
async fn upgrade_forest_prepares_every_root_and_private_memory_member_before_commit() {
    let (_home, config, manager, other) = fixture().await;
    let mut memory = config.clone();
    memory.ephemeral = true;
    let primary = manager
        .start_thread(StartThreadOptions::new(memory.clone()))
        .await
        .unwrap()
        .thread;
    let descendant = child(&manager, &primary, &memory).await;
    for member in [&primary, &descendant] {
        assert!(
            member
                .thread_extension_data()
                .get::<HeadlessExec>()
                .is_none()
        );
        assert!(
            member
                .thread_extension_data()
                .get::<codex_alab_core::quota::ExecQuotaMember>()
                .is_none()
        );
        assert!(member.exec_recovery_history().await.is_ok());
    }
    let blocker = primary
        .session
        .services
        .local_agent_runtime
        .admit_start()
        .unwrap();
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let mut plan = primary.prepare_upgrade_forest(&invocation).await.unwrap();
    assert_eq!(
        plan.roots(),
        vec![primary.exec_thread_id(), other.exec_thread_id()]
    );
    let expected = plan
        .threads()
        .iter()
        .map(|member| member.exec_thread_id())
        .collect::<Vec<_>>();
    assert_eq!(expected.len(), 3);
    assert!(plan.valid());
    assert!(plan.commit_if(|| true));
    let source = plan.into_committed().unwrap();
    let waiter = tokio::spawn({
        let source = Arc::clone(&source);
        async move { source.capture().await }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        primary.wait_until_terminated(),
    )
    .await
    .unwrap();
    waiter.abort();
    match waiter.await {
        Err(error) => assert!(error.is_cancelled()),
        Ok(_) => panic!("capture waiter completed before producer retirement"),
    }
    assert!(
        other.is_running(),
        "original producer custody delays completion before the next root"
    );
    drop(blocker);
    let snapshot = source.capture().await.unwrap();
    assert!(Arc::ptr_eq(&snapshot, &source.capture().await.unwrap()));
    assert!(!primary.is_running());
    assert!(!descendant.is_running());
    assert!(!other.is_running());
    snapshot
        .validate(primary.exec_thread_id(), &expected)
        .unwrap();
    let encoded = serde_json::to_value(snapshot.as_ref()).unwrap();
    assert!(encoded["inputs"]["input"]["publications"].is_array());
    let mut decoded = snapshot
        .as_ref()
        .clone()
        .prepare(primary.exec_thread_id(), &expected)
        .unwrap();
    let (tree, inputs) = decoded.take_tree(primary.exec_thread_id()).unwrap();
    assert_eq!(tree.threads().len(), 2);
    assert!(
        tree.threads()
            .iter()
            .all(|node| node.ephemeral_history().is_some())
    );
    assert_eq!(inputs.len(), 2);
    assert!(decoded.take_tree(primary.exec_thread_id()).is_err());
    let (tree, inputs) = decoded.take_tree(other.exec_thread_id()).unwrap();
    assert_eq!(tree.threads().len(), 1);
    assert_eq!(inputs.len(), 1);
    assert!(decoded.is_empty());
    let mut missing = encoded.clone();
    missing["trees"].as_array_mut().unwrap().pop();
    let missing: super::UpgradeForestSnapshot = serde_json::from_value(missing).unwrap();
    assert!(
        missing
            .validate(primary.exec_thread_id(), &expected)
            .is_err()
    );
    let snapshot: super::UpgradeForestSnapshot = serde_json::from_value(encoded).unwrap();
    assert!(
        snapshot
            .validate(other.exec_thread_id(), &expected)
            .is_err()
    );
    assert!(
        snapshot
            .validate(primary.exec_thread_id(), &expected[..2])
            .is_err()
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_invalid_type,
    reason = "isolated configuration lock proves revocation returns without waiting for its reader"
)]
async fn upgrade_forest_revocation_cancels_a_blocked_configuration_read() {
    let (_home, config, _manager, root) = fixture().await;
    let held = root.session.state.lock().await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let mut preparing = Box::pin(root.prepare_upgrade_forest(&invocation));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(std::future::Future::poll(
            preparing.as_mut(),
            cx
        ),))
        .await
        .is_pending()
    );
    let input = root
        .session
        .services
        .local_agent_runtime
        .alab_upgrade
        .admit()
        .unwrap();
    assert!(input.interrupted_transfer());
    drop(input);
    let error = tokio::time::timeout(std::time::Duration::from_secs(10), preparing)
        .await
        .unwrap()
        .err()
        .expect("revoked inventory must return while configuration stays locked");
    assert!(error.contains("superseded"));
    assert!(root.is_running());
    drop(held);
    drop(root.prepare_upgrade_forest(&invocation).await.unwrap());
    root.shutdown_exec_tree().await.unwrap();
}

#[tokio::test]
async fn upgrade_forest_refuses_active_work_and_late_input_without_stopping_members() {
    let (_home, config, manager, root) = fixture().await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let mut plan = root.prepare_upgrade_forest(&invocation).await.unwrap();
    root.update_thread_settings(codex_protocol::protocol::ThreadSettingsOverrides {
        disabled_plugin_ids: Some(vec!["accepted-after-forest-preflight".into()]),
        ..Default::default()
    })
    .await
    .unwrap();
    assert!(!plan.valid());
    let called = std::sync::atomic::AtomicBool::new(false);
    assert!(!plan.commit_if(|| {
        called.store(true, std::sync::atomic::Ordering::SeqCst);
        true
    }));
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    assert!(
        !plan
            .commit_with_readers_if(|| panic!("a revoked forest cannot close source SDK readers"))
            .unwrap()
    );
    drop(plan);
    assert!(root.is_running());
    let prepared = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match root.prepare_upgrade_forest(&invocation).await {
                Ok(prepared) => break prepared,
                Err(error) if error == "native admission is pending" => {
                    tokio::task::yield_now().await
                }
                Err(error) => {
                    panic!("forest could not prepare after accepted settings retired: {error}")
                }
            }
        }
    })
    .await
    .unwrap();
    drop(prepared);
    let _turn = start(&root, TaskKind::Regular).await;
    assert!(root.prepare_upgrade_forest(&invocation).await.is_err());
    assert!(root.is_running());
    assert!(root.session.active_turn.lock().await.is_some());
    root.shutdown_exec_tree().await.unwrap();
    let next = manager
        .start_thread(StartThreadOptions::new(config))
        .await
        .unwrap()
        .thread;
    assert!(
        next.is_running(),
        "failed preflight must leave manager admission open"
    );
    next.shutdown_exec_tree().await.unwrap();
}

#[tokio::test]
async fn upgrade_forest_joins_source_reader_closure_before_returning_its_capsule() {
    const CHILD: &str = "ALAB_UPGRADE_READER_FOREST_CASE";
    const TEST: &str = "session::alab_exec_recovery_options::tree_capture::tests::upgrade_forest_joins_source_reader_closure_before_returning_its_capsule";
    if std::env::var_os(CHILD).is_none() {
        // The committed process-global catalog intentionally never reopens.
        // Run this case in its own process, preserving the native SDK environment.
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CHILD, "1")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("isolated source reader handoff deadline")
        .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let (_home, config, manager, root) = fixture().await;
    let mut memory = config.clone();
    memory.ephemeral = true;
    let other = manager
        .start_thread(StartThreadOptions::new(memory.clone()))
        .await
        .unwrap()
        .thread;
    let private = child(&manager, &other, &memory).await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let mut plan = root.prepare_upgrade_forest(&invocation).await.unwrap();
    let decided = std::sync::atomic::AtomicUsize::new(0);
    assert!(
        !plan
            .commit_with_readers_if(|| {
                decided.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                false
            })
            .unwrap()
    );
    assert!(
        plan.valid(),
        "a rejected host decision preserves Core preparation"
    );
    codex_alab_core::manager_catalog::global()
        .seal()
        .expect("native reader catalog reopened after host rejection")
        .cancel_before_effects()
        .unwrap();
    assert!(
        plan.commit_with_readers_if(|| {
            decided.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            true
        })
        .unwrap()
    );
    assert_eq!(decided.load(std::sync::atomic::Ordering::SeqCst), 2);
    let committed = plan.into_committed().unwrap();
    let snapshot = tokio::time::timeout(std::time::Duration::from_secs(10), committed.capture())
        .await
        .unwrap()
        .unwrap();
    snapshot
        .validate(
            root.exec_thread_id(),
            &[
                root.exec_thread_id(),
                other.exec_thread_id(),
                private.exec_thread_id(),
            ],
        )
        .unwrap();
    assert!(Arc::ptr_eq(&snapshot, &committed.capture().await.unwrap()));
    assert!(
        !codex_alab_core::manager_catalog::global()
            .factory(None)
            .unwrap()
            .admitted()
    );
    assert!(!root.is_running());
    assert!(!other.is_running());
    assert!(!private.is_running());
}

#[tokio::test]
async fn upgrade_forest_rejects_nonportable_mail_still_in_the_accepted_channel() {
    struct Observer(std::sync::atomic::AtomicUsize);
    impl codex_extension_api::MessageLifecycleContributor for Observer {
        fn on_publish(&self, _: &codex_extension_api::MessageLifecycleInput) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        fn on_consumed(&self, _: &codex_extension_api::MessageLifecycleInput, _: ThreadId) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let (_home, config, _manager, root) = fixture().await;
    let observer = Arc::new(Observer(std::sync::atomic::AtomicUsize::new(0)));
    let observers: Vec<Arc<dyn codex_extension_api::MessageLifecycleContributor>> =
        vec![observer.clone()];
    let publication =
        crate::agent_communication::MessageDelivery::publish(root.exec_thread_id(), &observers);
    root.session
        .input_queue
        .enqueue_mailbox_communication(
            crate::PendingAgentMessage::new(
                InterAgentCommunication::new(
                    AgentPath::root(),
                    AgentPath::root(),
                    Vec::new(),
                    "retained opaque publication".into(),
                    false,
                ),
                Some(publication),
            ),
            Default::default(),
        )
        .await;
    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let error = match root.prepare_upgrade_forest(&invocation).await {
        Ok(_) => panic!("opaque publication cannot cross an exec boundary"),
        Err(error) => error,
    };
    assert!(error.contains("nonportable lifecycle observers"), "{error}");
    assert!(root.is_running());
    assert!(root.session.input_queue.has_pending_mailbox_items().await);
    assert_eq!(
        observer.0.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "preflight must not republish or consume accepted mail"
    );
    assert!(root.prepare_upgrade_manager().await.unwrap().valid());
    root.shutdown_exec_tree().await.unwrap();
}
