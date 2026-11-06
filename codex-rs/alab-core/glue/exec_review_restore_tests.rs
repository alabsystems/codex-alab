// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use codex_protocol::ThreadId;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use serde_json::json;

fn event(value: serde_json::Value) -> Event {
    Event {
        id: "delegate-turn".into(),
        msg: serde_json::from_value(value).unwrap(),
    }
}

fn snapshot() -> ExecReviewSnapshot {
    ExecReviewSnapshot {
        version: 1,
        parent: ThreadId::new(),
        turn: "parent-turn".into(),
        delegate: ThreadId::new(),
        delegate_turn: "delegate-turn".into(),
        buffered: Some(event(
            json!({"type":"agent_message","message":"before transfer"}),
        )),
        pending: vec![
            event(json!({"type":"agent_message","message":"after transfer"})),
            event(json!({"type":"warning","message":"queued warning"})),
            event(
                json!({"type":"task_complete","turn_id":"delegate-turn","last_agent_message":"{\"findings\":[],\"overall_correctness\":\"correct\",\"overall_explanation\":\"restored review\",\"overall_confidence_score\":1.0}"}),
            ),
        ],
    }
}

#[tokio::test]
async fn restored_consumer_delivers_saved_events_once_before_reading_live_events() {
    let snapshot = snapshot();
    let state = State::new(
        snapshot.parent,
        snapshot.turn.clone(),
        CancellationToken::new(),
    );
    state.restore_data(&snapshot).unwrap();
    assert!(state.restore_data(&snapshot).is_err());
    let (_send, receive) = async_channel::bounded(1);
    let (output, delivered) = async_channel::unbounded();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        Arc::clone(&state).run(receive, move |event| {
            let output = output.clone();
            async move {
                output.send(event).await.unwrap();
            }
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.overall_explanation, "restored review");
    assert!(
        matches!(delivered.try_recv().unwrap(), EventMsg::AgentMessage(message) if message.message == "before transfer")
    );
    assert!(
        matches!(delivered.try_recv().unwrap(), EventMsg::Warning(warning) if warning.message == "queued warning")
    );
    assert!(delivered.try_recv().is_err());
    assert!(state.data.lock().unwrap().pending.is_empty());
    assert_eq!(state.data.lock().unwrap().delegate, Some(snapshot.delegate));
}

#[test]
fn restore_rejects_changed_parent_or_turn_without_installing_data() {
    let snapshot = snapshot();
    for (parent, turn) in [
        (ThreadId::new(), snapshot.turn.clone()),
        (snapshot.parent, "another-turn".into()),
    ] {
        let state = State::new(parent, turn, CancellationToken::new());
        assert!(state.restore_data(&snapshot).is_err());
        assert!(state.data.lock().unwrap().delegate.is_none());
    }
}

#[test]
fn restore_rejects_a_consumer_that_already_has_a_receiver() {
    let snapshot = snapshot();
    let state = State::new(
        snapshot.parent,
        snapshot.turn.clone(),
        CancellationToken::new(),
    );
    let (_send, receive) = async_channel::bounded(1);
    state.receiver.set(receive).unwrap();
    assert!(state.restore_data(&snapshot).is_err());
    assert!(state.data.lock().unwrap().delegate.is_none());
}

struct CompleteAfter(tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>);
impl SessionTask for CompleteAfter {
    fn kind(&self) -> crate::state::TaskKind {
        crate::state::TaskKind::Regular
    }
    fn span_name(&self) -> &'static str {
        "complete_before_review_attachment"
    }
    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _context: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        _cancel: CancellationToken,
    ) -> SessionTaskResult {
        let finish = self.0.lock().await.take().unwrap();
        finish.await.unwrap();
        Ok(Some(json!({"findings":[],"overall_correctness":"correct","overall_explanation":"completed before attachment","overall_confidence_score":1.0}).to_string()))
    }
}

#[tokio::test]
async fn attachment_consumes_an_already_completed_original_delegate_from_live_io() {
    let home = tempfile::tempdir().unwrap();
    let mut config = crate::config::ConfigBuilder::without_managed_config_for_tests()
        .codex_home(home.path().to_owned())
        .harness_overrides(crate::config::ConfigOverrides {
            model: Some("gpt-5.5".into()),
            cwd: Some(home.path().to_owned()),
            ..Default::default()
        })
        .build()
        .await
        .unwrap();
    config.ephemeral = false;
    config.permissions.approval_policy =
        crate::config::Constrained::allow_only(codex_protocol::protocol::AskForApproval::Never);
    let manager = crate::ThreadManager::with_models_provider_and_home_for_tests(
        codex_login::CodexAuth::from_api_key("fixture"),
        config.model_provider.clone(),
        home.path().to_owned(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
    );
    let mut options = crate::StartThreadOptions::new(config.clone());
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let parent = manager.start_thread(options).await.unwrap().thread;
    let context = parent.session.new_default_turn().await;
    let original_cancel = CancellationToken::new();
    let original_guard =
        crate::tasks::review_recovery::begin(&parent.session, &context, &original_cancel);
    let (session, io) = crate::codex_delegate::run_codex_thread_interactive(
        config,
        Arc::clone(&parent.session.services.auth_manager),
        Arc::clone(&parent.session.services.models_manager),
        Arc::clone(&parent.session),
        Arc::clone(&context),
        context.initial_environments.clone(),
        original_cancel,
        SubAgentSource::Review,
        codex_extension_api::SessionIsolation::Inherit,
        None,
        crate::session::GitEnrichmentPolicy::Fresh,
        codex_sandboxing::WindowsSandboxProxySettingsMode::Reconcile,
    )
    .await
    .unwrap();
    let inventory = parent.seal_exec_tree_starts().await.unwrap();
    let registered = inventory
        .loaded_threads(&parent)
        .await
        .unwrap()
        .into_iter()
        .find(|child| child.session.thread_id == session.thread_id)
        .unwrap();
    drop(inventory);
    let child = Arc::new(CodexThread::new(
        session,
        io,
        crate::ThreadStartupMetadata::from(
            &registered.startup_metadata().to_session_configured_event(),
        ),
        registered.rollout_path(),
        registered.session_source.clone(),
    ));
    drop(original_guard);
    let child_context = child.session.new_default_turn().await;
    let (finish, receive) = tokio::sync::oneshot::channel();
    child
        .session
        .spawn_task(
            Arc::clone(&child_context),
            Vec::new(),
            CompleteAfter(tokio::sync::Mutex::new(Some(receive))),
        )
        .await;
    child
        .session
        .emit_turn_started(&child_context, crate::state::TaskKind::Regular)
        .await;
    finish.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while child.session.active_turn.lock().await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let cancel = CancellationToken::new();
    let guard = crate::tasks::review_recovery::begin(&parent.session, &context, &cancel);
    context.extension_data.insert(Prepared {
        snapshot: ExecReviewSnapshot {
            version: 1,
            parent: parent.session.thread_id,
            turn: context.sub_id.clone(),
            delegate: child.session.thread_id,
            delegate_turn: child_context.sub_id.clone(),
            buffered: None,
            pending: Vec::new(),
        },
        parent: Arc::downgrade(&parent.session),
        child: Arc::clone(&child),
    });
    let receive = start_or_resume(
        Arc::clone(&parent.session),
        Arc::clone(&context),
        Vec::new(),
        cancel,
    )
    .await
    .unwrap()
    .unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::tasks::review_recovery::process_events(
            Arc::clone(&parent.session),
            context,
            receive,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.overall_explanation, "completed before attachment");
    assert!(child.session.active_turn.lock().await.is_none());
    drop(guard);
    manager
        .shutdown_all_threads_bounded(std::time::Duration::from_secs(10))
        .await;
}
