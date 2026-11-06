// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::StartThreadOptions;
use crate::ThreadManager;
use crate::config::Config;
use crate::config::ConfigOverrides;
use codex_history::InitialHistory;
use codex_history::ResumedHistory;
use codex_protocol::protocol::AskForApproval;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

async fn fixture(
    headless: bool,
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
    config.permissions.approval_policy =
        crate::config::Constrained::allow_only(AskForApproval::Never);
    let manager = ThreadManager::with_models_provider_home_and_state_for_tests(
        codex_login::CodexAuth::from_api_key("fixture"),
        config.model_provider.clone(),
        home.path().to_owned(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        crate::init_state_db(&config).await,
    );
    let mut options = StartThreadOptions::new(config.clone());
    if headless {
        options.thread_extension_init.insert(HeadlessExec);
        options.turn_extension_init.insert(HeadlessExec);
    }
    let parent = manager.start_thread(options).await.unwrap().thread;
    (home, config, manager, parent)
}

async fn start_review(
    config: Config,
    parent: &Arc<CodexThread>,
    token: CancellationToken,
) -> Result<(Arc<Session>, SessionIo)> {
    let context = parent.session.new_default_turn().await;
    start_review_with_context(config, parent, context, token).await
}

async fn start_review_with_context(
    config: Config,
    parent: &Arc<CodexThread>,
    context: Arc<TurnContext>,
    token: CancellationToken,
) -> Result<(Arc<Session>, SessionIo)> {
    crate::codex_delegate::run_codex_thread_interactive(
        config,
        Arc::clone(&parent.session.services.auth_manager),
        Arc::clone(&parent.session.services.models_manager),
        Arc::clone(&parent.session),
        Arc::clone(&context),
        context.initial_environments.clone(),
        token,
        SubAgentSource::Review,
        codex_extension_api::SessionIsolation::Inherit,
        None,
        crate::session::GitEnrichmentPolicy::Fresh,
        codex_sandboxing::WindowsSandboxProxySettingsMode::Reconcile,
    )
    .await
}

#[tokio::test]
async fn exec_review_is_private_tracked_and_keeps_its_original_session() {
    for ephemeral in [false, true] {
        let (_home, config, manager, parent) = fixture(true, ephemeral).await;
        let (session, io) = start_review(config.clone(), &parent, CancellationToken::new())
            .await
            .unwrap();
        let id = session.thread_id;
        let tracked = manager
            .state
            .threads
            .read()
            .await
            .get(&id)
            .cloned()
            .unwrap();
        assert!(Arc::ptr_eq(&session, &tracked.session));
        assert_eq!(
            tracked.session_source,
            SessionSource::Internal(InternalSessionSource::Review)
        );
        assert_eq!(
            session.thread_config_snapshot().await.session_source,
            SessionSource::SubAgent(SubAgentSource::Review)
        );
        assert_eq!(
            session.thread_config_snapshot().await.parent_thread_id,
            Some(parent.session.thread_id)
        );
        assert!(Arc::ptr_eq(
            &session.services.auth_manager,
            &parent.session.services.auth_manager
        ));
        assert!(Arc::ptr_eq(
            &session.services.exec_policy,
            &parent.session.services.exec_policy
        ));
        assert!(manager.get_thread(id).await.is_err());
        assert!(manager.remove_thread_for_client(&id).await.is_err());
        let error = manager
            .resume_thread_with_history(
                config,
                InitialHistory::Resumed(ResumedHistory {
                    conversation_id: id,
                    history: Arc::new(Vec::new()),
                    history_revision: None,
                    rollout_path: None,
                }),
                Arc::clone(&parent.session.services.auth_manager),
                None,
                Default::default(),
            )
            .await
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("owner-private review"),
            "{error}"
        );

        let seal = parent.seal_exec_tree_starts().await.unwrap();
        let members = seal.loaded_threads(&parent).await.unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.iter().any(|member| Arc::ptr_eq(member, &tracked)));
        drop(seal);

        io.shutdown_and_wait().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.state.threads.read().await.contains_key(&id) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        manager
            .shutdown_all_threads_bounded(Duration::from_secs(10))
            .await;
    }
}

#[tokio::test]
async fn ordinary_review_remains_unregistered() {
    let (_home, config, manager, parent) = fixture(false, false).await;
    let (session, io) = start_review(config, &parent, CancellationToken::new())
        .await
        .unwrap();
    assert!(
        !manager
            .state
            .threads
            .read()
            .await
            .contains_key(&session.thread_id)
    );
    assert_eq!(
        session.thread_config_snapshot().await.session_source,
        SessionSource::SubAgent(SubAgentSource::Review)
    );
    io.shutdown_and_wait().await.unwrap();
    manager
        .shutdown_all_threads_bounded(Duration::from_secs(10))
        .await;
}

#[tokio::test]
async fn sealed_tree_rejects_review_start_without_registering_a_child() {
    let (_home, config, manager, parent) = fixture(true, false).await;
    let seal = parent.seal_exec_tree_starts().await.unwrap();
    let error = start_review(config, &parent, CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("being transferred"), "{error}");
    assert_eq!(manager.state.threads.read().await.len(), 1);
    drop(seal);
    manager
        .shutdown_all_threads_bounded(Duration::from_secs(10))
        .await;
}

#[tokio::test]
async fn cancelled_review_start_does_not_register_a_child() {
    let (_home, config, manager, parent) = fixture(true, false).await;
    let token = CancellationToken::new();
    token.cancel();
    assert!(start_review(config, &parent, token).await.is_err());
    assert_eq!(manager.state.threads.read().await.len(), 1);
    manager
        .shutdown_all_threads_bounded(Duration::from_secs(10))
        .await;
}

#[tokio::test]
async fn cancelling_live_review_stops_and_removes_its_private_registration() {
    let (_home, config, manager, parent) = fixture(true, false).await;
    let token = CancellationToken::new();
    let (session, io) = start_review(config, &parent, token.clone()).await.unwrap();
    let id = session.thread_id;
    assert!(manager.state.threads.read().await.contains_key(&id));
    token.cancel();
    tokio::time::timeout(Duration::from_secs(5), io.session_loop_termination.clone())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.state.threads.read().await.contains_key(&id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    manager
        .shutdown_all_threads_bounded(Duration::from_secs(10))
        .await;
}

#[tokio::test]
async fn a_review_consumer_cannot_replace_its_bound_delegate() {
    let (_home, config, manager, parent) = fixture(true, false).await;
    let context = parent.session.new_default_turn().await;
    let token = CancellationToken::new();
    let _consumer =
        crate::tasks::review_recovery::begin(&parent.session, &context, &token).unwrap();
    let (first, io) =
        start_review_with_context(config.clone(), &parent, Arc::clone(&context), token.clone())
            .await
            .unwrap();
    let error = start_review_with_context(config, &parent, context, token)
        .await
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("already owns a delegate"),
        "{error}"
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.state.threads.read().await.len() != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let registered = manager
        .state
        .threads
        .read()
        .await
        .get(&first.thread_id)
        .cloned()
        .unwrap();
    assert!(Arc::ptr_eq(&registered.session, &first));
    io.shutdown_and_wait().await.unwrap();
    manager
        .shutdown_all_threads_bounded(Duration::from_secs(10))
        .await;
}

#[tokio::test]
async fn restored_private_child_keeps_review_identity_and_requires_current_parent() {
    let (_home, config, manager, parent) = fixture(true, true).await;
    let runtime = &parent.session.services.local_agent_runtime;
    let mut options = StartThreadOptions::new(config.clone());
    options.session_source = Some(SessionSource::Internal(InternalSessionSource::Review));
    options.thread_extension_init.insert(HeadlessExec);
    bind_restore_parent(&mut options.thread_extension_init, &parent.session);
    let child = manager
        .state
        .spawn_child_thread(
            options,
            runtime.control(parent.session.session_id()),
            Some(parent.session.thread_id),
            Some(Arc::clone(&parent.session.services.exec_policy)),
        )
        .await
        .unwrap()
        .thread;
    watch_private_lifetime(&manager.state, &child);
    assert_eq!(
        child.session_source,
        SessionSource::Internal(InternalSessionSource::Review)
    );
    let settings = child.session.thread_config_snapshot().await;
    assert_eq!(
        settings.session_source,
        SessionSource::SubAgent(SubAgentSource::Review)
    );
    assert_eq!(settings.parent_thread_id, Some(parent.session.thread_id));
    assert_eq!(
        child.multi_agent_version(),
        Some(codex_protocol::protocol::MultiAgentVersion::Disabled)
    );
    assert!(Arc::ptr_eq(
        &child.session.services.auth_manager,
        &parent.session.services.auth_manager
    ));
    assert!(Arc::ptr_eq(
        &child.session.services.exec_policy,
        &parent.session.services.exec_policy
    ));
    assert!(manager.get_thread(child.session.thread_id).await.is_err());
    child.io.shutdown_and_wait().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager
            .state
            .threads
            .read()
            .await
            .contains_key(&child.session.thread_id)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut missing = StartThreadOptions::new(config);
    missing.session_source = Some(SessionSource::Internal(InternalSessionSource::Review));
    let result = manager
        .state
        .spawn_child_thread(
            missing,
            runtime.control(parent.session.session_id()),
            Some(parent.session.thread_id),
            Some(Arc::clone(&parent.session.services.exec_policy)),
        )
        .await;
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("review parent changed")
    );
    manager
        .shutdown_all_threads_bounded(Duration::from_secs(10))
        .await;
}
