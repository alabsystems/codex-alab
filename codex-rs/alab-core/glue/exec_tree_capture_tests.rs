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
    manager
        .spawn_internal_session(parent.session.thread_id, options)
        .await
        .unwrap()
        .thread
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
    let idle = child(&manager, &root, &config).await;
    let foreign = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .unwrap()
        .thread;
    let root_turn = start(&root, TaskKind::Regular).await;
    let child_turn = start(&active, TaskKind::Regular).await;
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
    assert_eq!(captured.threads().len(), 3);
    let wire = super::super::ExecTreeSnapshot::capture(&captured).unwrap();
    let wire: super::super::ExecTreeSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
    wire.validate(captured.root(), captured.turn()).unwrap();
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
    assert!(altered.validate(captured.root(), captured.turn()).is_err());
    let mut altered = serde_json::to_value(&wire).unwrap();
    altered["threads"].as_array_mut().unwrap().pop();
    let altered: super::super::ExecTreeSnapshot = serde_json::from_value(altered).unwrap();
    assert!(altered.validate(captured.root(), captured.turn()).is_err());

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
    assert_eq!(inputs.len(), 2);
    assert!(!root.is_running());
    assert!(!active.is_running());
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
    wire.validate(root.session.thread_id, &turn).unwrap();
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
    assert!(changed.validate(root.session.thread_id, &turn).is_err());
}
