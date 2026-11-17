// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Local writer/fork preparation/durable parent/deferred child controls.
//! No metadata/history success, source path or Native capability is fabricated.
use super::*;
use crate::AppendThreadItemsParams;
use crate::ForkBoundary;
use crate::PrepareForkParams;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

fn message(text: &str) -> RolloutItem {
    RolloutItem::ResponseItem(
        ResponseItem::Message {
            id: None,
            role: "user".into(),
            content: vec![ContentItem::InputText { text: text.into() }],
            phase: None,
            status: None,
            encrypted_content: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    )
}
fn without_headers(items: Vec<RolloutItem>) -> Vec<RolloutItem> {
    items
        .into_iter()
        .filter(|item| !matches!(item, RolloutItem::SessionMeta(_)))
        .collect()
}
async fn cleanup(store: &LocalThreadStore, id: ThreadId) -> TestResult {
    match store.discard_thread(id).await {
        Ok(()) | Err(ThreadStoreError::ThreadNotFound { .. }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[tokio::test]
async fn original_frozen_fork_prefix_survives_parent_append_and_compression_without_materializing_child()
-> TestResult {
    let home = TempDir::new()?;
    let config = test_config(home.path());
    let state = codex_state::StateRuntime::init(
        config.sqlite.clone(),
        config.default_model_provider_id.clone(),
    )
    .await?;
    let store = Arc::new(LocalThreadStore::new(config, Some(state.clone())));
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let result: TestResult = async {
        let mut parent_params = create_thread_params(parent);
        parent_params.history_mode = ThreadHistoryMode::Paginated;
        let parent_live = LiveThread::create(store.clone(), parent_params).await?;
        store
            .append_items(AppendThreadItemsParams {
                thread_id: parent,
                items: vec![message("actual inherited before fork")],
            })
            .await?;
        store
            .persist_thread(parent, PersistContext::Standard)
            .await?;
        let prepared = store
            .prepare_fork(PrepareForkParams {
                thread_id: parent,
                boundary: ForkBoundary::Latest,
            })
            .await?;
        let base = prepared.history_base.ok_or_else(|| {
            std::io::Error::other("original prepare_fork returned no actual base")
        })?;
        let path = store.live_rollout_path(parent).await?;
        let expected = without_headers(
            codex_rollout::RolloutRecorder::load_rollout_items(&path)
                .await?
                .0,
        );
        check(
            !expected.is_empty(),
            "fixture did not persist real inherited records",
        )?;
        let mut params = create_thread_params(child);
        params.history_mode = ThreadHistoryMode::Paginated;
        params.forked_from_id = Some(parent);
        params.history_base = Some(base);
        let child_live = LiveThread::create(store.clone(), params).await?;
        drop(prepared); // SAME original reservation lived through actual child construction.
        store
            .append_items(AppendThreadItemsParams {
                thread_id: parent,
                items: vec![message("actual parent after frozen fork")],
            })
            .await?;
        store
            .persist_thread(parent, PersistContext::Standard)
            .await?;
        let child_path = store.live_rollout_path(child).await?;
        let local = child_live
            .retained_deferred_history()
            .await?
            .ok_or_else(|| std::io::Error::other("child was not genuinely deferred"))?;
        check(
            local.session_meta.history_base == Some(base) && local.items.is_empty(),
            "local-only snapshot changed its original contract",
        )?;
        let display = child_live
            .retained_deferred_display_history()
            .await?
            .ok_or_else(|| std::io::Error::other("actual deferred display missing"))?;
        check(
            serde_json::to_value(&display.inherited_items)? == serde_json::to_value(&expected)?,
            "display lost/added original frozen ancestor records",
        )?;
        check(
            !tokio::fs::try_exists(&child_path).await?,
            "display read materialized deferred child",
        )?;
        parent_live.shutdown().await?;
        let bytes = tokio::fs::read(&path).await?;
        let compressed = path.with_extension("jsonl.zst");
        tokio::fs::write(&compressed, zstd::encode_all(bytes.as_slice(), 0)?).await?;
        tokio::fs::remove_file(&path).await?;
        let compressed_display = child_live
            .retained_deferred_display_history()
            .await?
            .ok_or_else(|| std::io::Error::other("compressed ancestor disappeared"))?;
        check(
            serde_json::to_value(&compressed_display.inherited_items)?
                == serde_json::to_value(&expected)?,
            "compressed lineage changed frozen prefix",
        )?;
        check(
            !tokio::fs::try_exists(&path).await? && !tokio::fs::try_exists(&child_path).await?,
            "observation decoded a durable copy or created child",
        )?;
        tokio::fs::write(&compressed, b"actual corrupted compression fixture").await?;
        check(
            child_live
                .retained_deferred_display_history()
                .await
                .is_err(),
            "unreadable real ancestor became empty-success history",
        )?;
        Ok(())
    }
    .await;
    let a = cleanup(&store, child).await;
    let b = cleanup(&store, parent).await;
    state.close().await;
    result?;
    a?;
    b
}

#[tokio::test]
async fn display_snapshot_keeps_original_writer_generation_and_materialized_none_contract()
-> TestResult {
    let home = TempDir::new()?;
    let store = Arc::new(LocalThreadStore::new(test_config(home.path()), None));
    let id = ThreadId::new();
    let result: TestResult = async {
        let old = LiveThread::create(store.clone(), create_thread_params(id)).await?;
        check(
            old.retained_deferred_display_history().await?.is_some(),
            "actual new root snapshot absent",
        )?;
        old.discard().await?;
        let current = LiveThread::create(store.clone(), create_thread_params(id)).await?;
        check(
            matches!(
                old.retained_deferred_display_history().await,
                Err(ThreadStoreError::Conflict { .. })
            ),
            "display accepted old same-ID writer handle",
        )?;
        check(
            current.retained_deferred_display_history().await?.is_some(),
            "actual replacement root snapshot absent",
        )?;
        store.persist_thread(id, PersistContext::Standard).await?;
        check(
            current.retained_deferred_display_history().await?.is_none(),
            "materialized writer became deferred display",
        )?;
        Ok(())
    }
    .await;
    let drained = cleanup(&store, id).await;
    result?;
    drained
}

fn started(id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnStarted(
        codex_protocol::protocol::TurnStartedEvent {
            turn_attribution: None,
            turn_id: id.into(),
            root_turn_id: None,
            trace_id: None,
            started_at: Some(10),
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        },
    ))
}
fn completed(id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnComplete(
        codex_protocol::protocol::TurnCompleteEvent {
            root_turn_id: None,
            turn_id: id.into(),
            last_agent_message: None,
            error: None,
            started_at: Some(10),
            completed_at: Some(20),
            duration_ms: Some(10_000),
            time_to_first_token_ms: None,
        },
    ))
}
async fn turns(store: &LocalThreadStore, id: ThreadId) -> Result<Vec<String>, ThreadStoreError> {
    Ok(store
        .list_turns(crate::ListTurnsParams {
            thread_id: id,
            include_archived: false,
            cursor: None,
            page_size: 100,
            sort_direction: crate::SortDirection::Asc,
            items_view: crate::StoredTurnItemsView::NotLoaded,
        })
        .await?
        .turns
        .into_iter()
        .map(|turn| turn.turn_id)
        .collect())
}
fn display_turns(items: &[RolloutItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            RolloutItem::EventMsg(EventMsg::TurnStarted(event)) => Some(event.turn_id.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn deferred_child_keeps_original_prefix_after_actual_parent_current_revert() -> TestResult {
    let home = TempDir::new()?;
    let config = test_config(home.path());
    let state = codex_state::StateRuntime::init(
        config.sqlite.clone(),
        config.default_model_provider_id.clone(),
    )
    .await?;
    let store = Arc::new(LocalThreadStore::new(config, Some(state.clone())));
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let result: TestResult = async {
        let mut params = create_thread_params(parent);
        params.history_mode = ThreadHistoryMode::Paginated;
        let source = LiveThread::create(store.clone(), params).await?;
        store
            .append_items(AppendThreadItemsParams {
                thread_id: parent,
                items: vec![
                    started("original-one"),
                    message("one"),
                    completed("original-one"),
                    started("original-two"),
                    message("two"),
                    completed("original-two"),
                ],
            })
            .await?;
        store
            .persist_thread(parent, PersistContext::Standard)
            .await?;
        let fork = store
            .prepare_fork(PrepareForkParams {
                thread_id: parent,
                boundary: ForkBoundary::Latest,
            })
            .await?;
        let original = store.live_rollout_path(parent).await?;
        let expected = without_headers(
            codex_rollout::RolloutRecorder::load_rollout_items(&original)
                .await?
                .0,
        );
        let mut params = create_thread_params(child);
        params.history_mode = ThreadHistoryMode::Paginated;
        params.forked_from_id = Some(parent);
        params.history_base = fork.history_base;
        let target = LiveThread::create(store.clone(), params).await?;
        drop(fork);
        source.shutdown().await?;
        // Revert consumes the normal SQLite projection of the actual writer
        // file, as in the original store revert controls. This low-level
        // fixture has no Core metadata projector running in the background.
        codex_rollout::state_db::reconcile_rollout(
            Some(state.as_ref()),
            original.as_path(),
            "test-provider",
            None,
            &[],
            Some(false),
            None,
        )
        .await;
        check(
            state.get_thread(parent).await?.is_some(),
            "original reconciliation did not project the actual parent",
        )?;
        // Actual original immutable-rollout cutover; no stored metadata edit.
        store
            .revert_thread(crate::RevertThreadParams {
                thread_id: parent,
                before_turn_id: "original-two".into(),
                multi_agent_version: None,
            })
            .await?;
        let current_path = state
            .get_thread(parent)
            .await?
            .ok_or_else(|| std::io::Error::other("reverted source metadata absent"))?
            .rollout_path;
        check(
            current_path != original,
            "original revert did not select a different actual current rollout",
        )?;
        check(
            turns(&store, parent).await? == ["original-one"],
            "parent current view did not actually lose the later turn",
        )?;
        let display = target
            .retained_deferred_display_history()
            .await?
            .ok_or_else(|| std::io::Error::other("deferred target absent"))?;
        check(
            serde_json::to_value(&display.inherited_items)? == serde_json::to_value(&expected)?,
            "child followed current reverted parent instead of original frozen base",
        )?;
        check(
            display_turns(&display.inherited_items) == ["original-one", "original-two"],
            "frozen child lost original later turn",
        )?;
        check(
            !tokio::fs::try_exists(store.live_rollout_path(child).await?).await?,
            "revert observation materialized deferred child",
        )?;
        Ok(())
    }
    .await;
    let a = cleanup(&store, child).await;
    let b = cleanup(&store, parent).await;
    state.close().await;
    result?;
    a?;
    b
}

#[tokio::test]
async fn failed_first_frozen_ancestor_copy_re_resolves_original_alternate_copy() -> TestResult {
    use std::io::Write as _;
    let home = TempDir::new()?;
    let config = test_config(home.path());
    let state = codex_state::StateRuntime::init(
        config.sqlite.clone(),
        config.default_model_provider_id.clone(),
    )
    .await?;
    let store = Arc::new(LocalThreadStore::new(config, Some(state.clone())));
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let result: TestResult = async {
        let mut params = create_thread_params(parent);
        params.history_mode = ThreadHistoryMode::Paginated;
        let source = LiveThread::create(store.clone(), params).await?;
        store
            .append_items(AppendThreadItemsParams {
                thread_id: parent,
                items: vec![
                    started("frozen-source"),
                    message("real alternate prefix"),
                    completed("frozen-source"),
                ],
            })
            .await?;
        store
            .persist_thread(parent, PersistContext::Standard)
            .await?;
        let fork = store
            .prepare_fork(PrepareForkParams {
                thread_id: parent,
                boundary: ForkBoundary::Latest,
            })
            .await?;
        let base = fork
            .history_base
            .ok_or_else(|| std::io::Error::other("original source base absent"))?;
        let active = store.live_rollout_path(parent).await?;
        let expected = without_headers(
            codex_rollout::RolloutRecorder::load_rollout_items(&active)
                .await?
                .0,
        );
        let mut params = create_thread_params(child);
        params.history_mode = ThreadHistoryMode::Paginated;
        params.forked_from_id = Some(parent);
        params.history_base = Some(base);
        let target = LiveThread::create(store.clone(), params).await?;
        drop(fork);
        source.shutdown().await?;
        let contents = tokio::fs::read(&active).await?;
        let archived_dir = home.path().join("archived_sessions");
        tokio::fs::create_dir_all(&archived_dir).await?;
        let archived = archived_dir.join(
            active
                .file_name()
                .ok_or_else(|| std::io::Error::other("original rollout filename absent"))?,
        );
        // Preserve an intact ACTUAL writer-produced alternate copy. Corrupt
        // only the indexed first file's compression trailer, keeping its real
        // metadata readable so failure occurs during the inherited read.
        tokio::fs::copy(&active, &archived).await?;
        let mut encoder = zstd::Encoder::new(Vec::new(), 0)?;
        encoder.write_all(&contents)?;
        encoder.flush()?;
        let compressed = active.with_extension("jsonl.zst");
        tokio::fs::write(&compressed, encoder.get_ref()).await?;
        drop(encoder);
        tokio::fs::remove_file(&active).await?;
        let initial = store
            .retained_inherited_lineage_for_tests(child, base)
            .await?;
        check(
            initial == compressed,
            "fixture did not select indexed incomplete first ancestor before read",
        )?;
        let display = target
            .retained_deferred_display_history()
            .await?
            .ok_or_else(|| std::io::Error::other("deferred child absent"))?;
        check(
            serde_json::to_value(&display.inherited_items)? == serde_json::to_value(&expected)?,
            "first-ancestor read did not retry intact original alternate",
        )?;
        check(
            !tokio::fs::try_exists(&active).await?
                && !tokio::fs::try_exists(store.live_rollout_path(child).await?).await?,
            "fallback materialized source/child",
        )?;
        Ok(())
    }
    .await;
    let a = cleanup(&store, child).await;
    let b = cleanup(&store, parent).await;
    state.close().await;
    result?;
    a?;
    b
}

#[tokio::test]
async fn original_attached_ancestor_display_view_hides_its_inherited_root_prefix() -> TestResult {
    let home = TempDir::new()?;
    let config = test_config(home.path());
    let state = codex_state::StateRuntime::init(
        config.sqlite.clone(),
        config.default_model_provider_id.clone(),
    )
    .await?;
    let store = Arc::new(LocalThreadStore::new(config, Some(state.clone())));
    let root = ThreadId::new();
    let attached = ThreadId::new();
    let child = ThreadId::new();
    let result: TestResult = async {
        let mut params = create_thread_params(root);
        params.history_mode = ThreadHistoryMode::Paginated;
        LiveThread::create(store.clone(), params).await?;
        store
            .append_items(AppendThreadItemsParams {
                thread_id: root,
                items: vec![
                    started("hidden-root"),
                    message("hidden inherited root"),
                    completed("hidden-root"),
                ],
            })
            .await?;
        store.persist_thread(root, PersistContext::Standard).await?;
        let prepared = store
            .prepare_fork(PrepareForkParams {
                thread_id: root,
                boundary: ForkBoundary::Latest,
            })
            .await?;
        let mut params = create_thread_params(attached);
        params.history_mode = ThreadHistoryMode::Paginated;
        params.attached_thread = true;
        params.forked_from_id = Some(root);
        params.history_base = prepared.history_base;
        LiveThread::create(store.clone(), params).await?;
        drop(prepared);
        store
            .append_items(AppendThreadItemsParams {
                thread_id: attached,
                items: vec![
                    started("visible-attached"),
                    message("actual attached turn"),
                    completed("visible-attached"),
                ],
            })
            .await?;
        store
            .persist_thread(attached, PersistContext::Standard)
            .await?;
        let prepared = store
            .prepare_fork(PrepareForkParams {
                thread_id: attached,
                boundary: ForkBoundary::Latest,
            })
            .await?;
        let mut params = create_thread_params(child);
        params.history_mode = ThreadHistoryMode::Paginated;
        params.forked_from_id = Some(attached);
        params.history_base = prepared.history_base;
        let target = LiveThread::create(store.clone(), params).await?;
        drop(prepared);
        let original_view = turns(&store, attached).await?;
        check(
            original_view == ["visible-attached"],
            "actual attached store view exposed hidden root",
        )?;
        let display = target
            .retained_deferred_display_history()
            .await?
            .ok_or_else(|| std::io::Error::other("actual child display absent"))?;
        check(
            display_turns(&display.inherited_items) == original_view,
            "deferred reader lost original attached ancestor view policy",
        )?;
        check(
            !tokio::fs::try_exists(store.live_rollout_path(child).await?).await?,
            "attached-view observation materialized child",
        )?;
        Ok(())
    }
    .await;
    let a = cleanup(&store, child).await;
    let b = cleanup(&store, attached).await;
    let c = cleanup(&store, root).await;
    state.close().await;
    result?;
    a?;
    b?;
    c
}
