// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::atomic::Ordering;

struct Forest {
    home: tempfile::TempDir,
    config: Config,
    manager: ThreadManager,
    roots: [Arc<CodexThread>; 2],
    children: [Arc<CodexThread>; 2],
}

impl Forest {
    async fn create() -> Self {
        let (home, config, manager, first) = fixture_options(true, false).await;
        let mut options = StartThreadOptions::new(config.clone());
        options.history_mode = Some(codex_protocol::protocol::ThreadHistoryMode::Paginated);
        let second = manager.start_thread(options).await.unwrap().thread;
        let children = [
            captured_child(&manager, &first, &config, true).await,
            captured_child(&manager, &second, &config, true).await,
        ];
        assert!(!first.shares_exec_tree(&second));
        assert!(first.shares_exec_tree(&children[0]));
        assert!(second.shares_exec_tree(&children[1]));
        Self {
            home,
            config,
            manager,
            roots: [first, second],
            children,
        }
    }

    async fn complete_child(&self, index: usize) -> (serde_json::Value, ThreadId, String) {
        let root = &self.roots[index];
        let sender = self.children[index].exec_thread_id();
        let runtime = &root.session.services.local_agent_runtime;
        finish_child(&self.children[index]).await;
        let original = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(result) = exec_input_state::results::original_queued_result(root).await
                {
                    break result;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(original.1, sender);
        assert_eq!(runtime.exec_result_counter().load(Ordering::Acquire), 1);
        original
    }

    async fn prepare(&self) -> Result<crate::PreparedUpgradeForest, String> {
        let invocation =
            ExecConfiguration::capture(&self.config, &ConfigOverrides::default()).unwrap();
        self.roots[0].prepare_upgrade_forest(&invocation).await
    }
}

#[tokio::test]
async fn two_forest_parents_transfer_their_own_queued_result_to_distinct_durable_writers() {
    let forest = Forest::create().await;
    let expected = [
        forest.complete_child(0).await,
        forest.complete_child(1).await,
    ];
    let mut plan = forest.prepare().await.unwrap();
    assert!(!plan.commit_if(|| false));
    for (root, original) in forest.roots.iter().zip(&expected) {
        assert!(root.is_running());
        assert_eq!(
            exec_input_state::results::original_queued_result(root)
                .await
                .as_ref(),
            Some(original)
        );
    }
    let ids = plan
        .threads()
        .iter()
        .map(|member| member.exec_thread_id())
        .collect::<Vec<_>>();
    assert!(plan.commit_if(|| true));
    for root in &forest.roots {
        assert!(forest.manager.suspend_agent_tree(root).await.is_err());
    }
    let primary = forest.roots[0].exec_thread_id();
    let snapshot = plan.into_committed().unwrap().capture().await.unwrap();
    let bytes = serde_json::to_vec(snapshot.as_ref()).unwrap();
    let snapshot: crate::UpgradeForestSnapshot = serde_json::from_slice(&bytes).unwrap();
    let mut decoded = snapshot.prepare(primary, &ids).unwrap();
    let fresh = Arc::new(
        ThreadManager::with_models_provider_home_and_state_for_tests(
            codex_login::CodexAuth::from_api_key("fixture"),
            forest.config.model_provider.clone(),
            forest.home.path().to_owned(),
            Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
            crate::init_state_db(&forest.config).await,
        ),
    );
    let mut destinations = Vec::new();
    for (index, old) in forest.roots.iter().enumerate() {
        let root = old.exec_thread_id();
        let (tree, mut input) = decoded.take_tree(root).unwrap();
        assert_eq!(tree.threads().len(), 2);
        let history = old
            .session
            .services
            .thread_store
            .load_latest_model_context(codex_thread_store::LoadThreadHistoryParams {
                thread_id: root,
                include_archived: false,
            })
            .await
            .unwrap();
        let mut options = StartThreadOptions::new(forest.config.clone());
        options.history_mode = Some(codex_protocol::protocol::ThreadHistoryMode::Paginated);
        options.initial_history = InitialHistory::Resumed(ResumedHistory {
            conversation_id: root,
            history: Arc::new(history.items),
            history_revision: history.revision,
            rollout_path: None,
        });
        let current = fresh.start_thread(options).await.unwrap().thread;
        assert_eq!(current.exec_thread_id(), root);
        let install = current
            .prepare_exec_idle_root_restore(input.take_idle(root).unwrap())
            .unwrap();
        let manager = fresh.clone();
        let registered = Arc::clone(&current);
        install
            .restore(move |id| {
                Box::pin(async move {
                    let loaded = manager
                        .get_thread(id)
                        .await
                        .map_err(|error| error.to_string())?;
                    if !Arc::ptr_eq(&loaded, &registered) {
                        return Err("fresh root changed".into());
                    }
                    Ok(())
                })
            })
            .await
            .unwrap();
        assert_eq!(
            exec_input_state::results::original_queued_result(&current)
                .await
                .as_ref(),
            Some(&expected[index])
        );
        destinations.push(current);
    }
    assert!(decoded.is_empty());
    for (index, current) in destinations.iter().enumerate() {
        for (position, root) in destinations.iter().enumerate() {
            assert_eq!(
                root.session
                    .services
                    .local_agent_runtime
                    .exec_result_counter()
                    .load(Ordering::Acquire),
                usize::from(position >= index)
            );
        }
        let mut mail = current.session.input_queue.drain_mailbox().await;
        assert_eq!(mail.len(), 1);
        let context = current.session.new_default_turn().await;
        current
            .session
            .record_inter_agent_communication(
                &context,
                context.model_info(),
                mail.pop().unwrap().communication,
            )
            .await;
        assert_eq!(
            current
                .session
                .services
                .local_agent_runtime
                .exec_result_counter()
                .load(Ordering::Acquire),
            0
        );
        let id = expected[index].0["id"].as_str().unwrap();
        let history = current.exec_recovery_history().await.unwrap();
        assert_eq!(history.items.iter().filter(|item| {
            matches!(item, codex_history::RolloutItem::ResponseItem(input) if serde_json::to_value(&input.item).unwrap()["id"] == id)
        }).count(), 1);
        assert_eq!(
            forest.roots[index]
                .session
                .services
                .local_agent_runtime
                .exec_result_counter()
                .load(Ordering::Acquire),
            1
        );
        current.shutdown_and_wait().await.unwrap();
    }
}

#[tokio::test]
async fn a_claimed_or_indeterminate_sibling_writer_defers_the_whole_forest() {
    let forest = Forest::create().await;
    let original = forest.complete_child(0).await;
    forest.complete_child(1).await;
    let mut plan = forest.prepare().await.unwrap();
    let second = &forest.roots[1];
    let mut mail = second.session.input_queue.drain_mailbox().await;
    let claimed = mail
        .pop()
        .unwrap()
        .communication
        .consumed(second.exec_thread_id())
        .unwrap();
    assert!(
        !plan.commit_if(|| panic!("a claimed native writer must refuse before host selection"))
    );
    drop(plan);
    assert!(forest.prepare().await.is_err());
    // A dropped, unconfirmed append is indeterminate. A missing delivery cannot satisfy it.
    drop(claimed);
    assert!(forest.prepare().await.is_err());
    assert_eq!(
        second
            .session
            .services
            .local_agent_runtime
            .exec_result_counter()
            .load(Ordering::Acquire),
        1
    );
    assert_eq!(
        exec_input_state::results::original_queued_result(&forest.roots[0]).await,
        Some(original)
    );
    for root in &forest.roots {
        root.shutdown_exec_tree().await.unwrap();
    }
}

#[tokio::test]
async fn one_competing_native_suspension_leaves_other_forest_owners_untouched() {
    let forest = Forest::create().await;
    let original = forest.complete_child(0).await;
    let mut plan = forest.prepare().await.unwrap();
    // A real public suspension owns only its own root. It may finish after the
    // prepared manager releases the registry, but its disposition is selected first.
    let mut suspend = Box::pin(forest.manager.suspend_agent_tree(&forest.roots[1]));
    assert!(futures::poll!(suspend.as_mut()).is_pending());
    assert!(!plan.commit_if(|| panic!("competing native disposition must precede host selection")));
    drop(plan);
    assert!(suspend.await.unwrap().is_some());
    assert_eq!(
        exec_input_state::results::original_queued_result(&forest.roots[0]).await,
        Some(original)
    );
    assert!(
        forest
            .manager
            .suspend_agent_tree(&forest.roots[0])
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn an_evicted_receiving_parent_keeps_its_result_in_the_original_tree() {
    let forest = Forest::create().await;
    let original = forest.complete_child(0).await;
    let parent = &forest.children[1];
    let grandchild = captured_child(&forest.manager, parent, &forest.config, true).await;
    assert_eq!(
        grandchild.session_source().parent_thread_id(),
        Some(parent.exec_thread_id())
    );
    assert_eq!(
        forest
            .manager
            .try_evict_v2_thread(Arc::clone(parent))
            .await
            .unwrap(),
        crate::ThreadEvictionOutcome::Evicted
    );
    finish_child(&grandchild).await;
    let second = &forest.roots[1];
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some((_, sender, _)) =
                exec_input_state::results::original_queued_result(second).await
            {
                assert_eq!(sender, grandchild.exec_thread_id());
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        forest
            .manager
            .try_evict_v2_thread(Arc::clone(&grandchild))
            .await
            .unwrap(),
        crate::ThreadEvictionOutcome::Evicted
    );
    let mut plan = forest.prepare().await.unwrap();
    assert!(!plan.commit_if(|| panic!("an absent receiving parent has no destination writer")));
    drop(plan);
    let runtime = &second.session.services.local_agent_runtime;
    assert_eq!(runtime.exec_result_counter().load(Ordering::Acquire), 1);
    assert!(
        runtime
            .lock_exec_result_controller()
            .unwrap()
            .result_routes_belong_to(|id| id == parent.exec_thread_id())
    );
    assert_eq!(
        exec_input_state::results::original_queued_result(&forest.roots[0]).await,
        Some(original)
    );
    // Public suspension still owns the stored parent's original writer.
    assert!(
        forest
            .manager
            .suspend_agent_tree(second)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(runtime.exec_result_counter().load(Ordering::Acquire), 0);
    forest.roots[0].shutdown_exec_tree().await.unwrap();
}
