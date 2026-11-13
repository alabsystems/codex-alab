// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::models::BaseInstructions;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::ThreadMemoryMode;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use codex_protocol::user_input::UserInput;
use codex_rollout::RolloutItem;
use codex_thread_store::AppendThreadItemsParams;
use codex_thread_store::ArchiveThreadParams;
use codex_thread_store::CreateThreadParams;
use codex_thread_store::DeleteThreadParams;
use codex_thread_store::ListThreadsParams;
use codex_thread_store::LiveThread;
use codex_thread_store::LoadThreadHistoryParams;
use codex_thread_store::LocalThreadStore;
use codex_thread_store::LocalThreadStoreConfig;
use codex_thread_store::PersistContext;
use codex_thread_store::ReadThreadByRolloutPathParams;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ResumeThreadParams;
use codex_thread_store::StoredThread;
use codex_thread_store::StoredThreadHistory;
use codex_thread_store::ThreadPage;
use codex_thread_store::ThreadPersistenceMetadata;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreFuture;
use codex_thread_store::UpdateThreadMetadataParams;
use tempfile::TempDir;
use tokio::sync::Notify;

use super::prepare_when;

/// Pause a real first-input append after canonical persistence but before LiveThread
/// receives its completion and publishes the pending metadata. No SQLite row is edited.
struct FirstInputStore {
    inner: Arc<LocalThreadStore>,
    written: Notify,
    release: Notify,
    preparations: AtomicUsize,
}

macro_rules! delegate_store_methods {
    ($(fn $name:ident($param:ident: $params:ty) -> $result:ty;)*) => {
        $(fn $name(&self, $param: $params) -> ThreadStoreFuture<'_, $result> {
            ThreadStore::$name(self.inner.as_ref(), $param)
        })*
    };
}

impl ThreadStore for FirstInputStore {
    fn as_any(&self) -> &dyn Any {
        self
    }

    delegate_store_methods! {
        fn create_thread(params: CreateThreadParams) -> ();
        fn resume_thread(params: ResumeThreadParams) -> Arc<Vec<RolloutItem>>;
        fn discard_thread(thread_id: ThreadId) -> ();
        fn load_history(params: LoadThreadHistoryParams) -> StoredThreadHistory;
        fn read_thread(params: ReadThreadParams) -> StoredThread;
        fn read_thread_by_rollout_path(params: ReadThreadByRolloutPathParams) -> StoredThread;
        fn list_threads(params: ListThreadsParams) -> ThreadPage;
        fn archive_thread(params: ArchiveThreadParams) -> ();
        fn unarchive_thread(params: ArchiveThreadParams) -> StoredThread;
        fn delete_thread(params: DeleteThreadParams) -> ();
        fn flush_thread(thread_id: ThreadId) -> ();
        fn shutdown_thread(thread_id: ThreadId) -> ();
        fn update_thread_metadata(params: UpdateThreadMetadataParams) -> Option<StoredThread>;
    }

    fn append_items(&self, params: AppendThreadItemsParams) -> ThreadStoreFuture<'_, ()> {
        Box::pin(async move {
            self.inner.append_items(params).await?;
            self.written.notify_one();
            self.release.notified().await;
            Ok(())
        })
    }

    fn persist_thread(
        &self,
        thread_id: ThreadId,
        context: PersistContext,
    ) -> ThreadStoreFuture<'_, ()> {
        if context == PersistContext::ThreadPreparation {
            self.preparations.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.persist_thread(thread_id, context)
    }
}

fn create_params(home: &TempDir, history_mode: ThreadHistoryMode) -> CreateThreadParams {
    let thread_id = ThreadId::new();
    CreateThreadParams {
        attached_thread: false,
        creator_user_id: None,
        creator_account_id: None,
        session_id: thread_id.into(),
        thread_id,
        extra_config: None,
        forked_from_id: None,
        parent_thread_id: None,
        source: SessionSource::Exec,
        thread_source: None,
        originator: "projection-test".to_owned(),
        base_instructions: BaseInstructions::default(),
        dynamic_tools: Vec::new(),
        selected_capability_roots: Vec::new(),
        multi_agent_version: None,
        history_mode,
        history_base: None,
        subagent_history_start_ordinal: None,
        initial_window_id: codex_protocol::ContextWindowId::new_uuid_v7(),
        runtime_workspace_roots: None,
        metadata: ThreadPersistenceMetadata {
            cwd: Some(home.path().to_path_buf()),
            model_provider: "test-provider".to_owned(),
            memory_mode: ThreadMemoryMode::Enabled,
        },
    }
}

async fn first_input_resume(active: bool, history_mode: ThreadHistoryMode) {
    let home = TempDir::new().unwrap();
    let sqlite = codex_state::SqliteConfig::new_for_testing(home.path().try_into().unwrap());
    let state_db = codex_state::StateRuntime::init(sqlite.clone(), "test-provider".to_owned())
        .await
        .unwrap();
    let inner = Arc::new(LocalThreadStore::new(
        LocalThreadStoreConfig {
            codex_home: home.path().to_path_buf(),
            sqlite,
            default_model_provider_id: "test-provider".to_owned(),
        },
        Some(Arc::clone(&state_db)),
    ));
    let store = Arc::new(FirstInputStore {
        inner,
        written: Notify::new(),
        release: Notify::new(),
        preparations: AtomicUsize::new(0),
    });
    let params = create_params(&home, history_mode);
    let thread_id = params.thread_id;
    let live = LiveThread::create(store.clone(), params).await.unwrap();
    // A real untouched live writer has no stored record. The production hook is
    // after this read, so rejected untouched/ephemeral resumes never prepare it.
    let read = ReadThreadParams {
        thread_id,
        include_archived: true,
        include_history: false,
    };
    assert!(store.read_thread(read.clone()).await.is_err());
    assert!(state_db.get_thread(thread_id).await.unwrap().is_none());
    assert_eq!(store.preparations.load(Ordering::SeqCst), 0);

    let writer = live.clone();
    let append = tokio::spawn(async move {
        writer
            .append_items(&[
                RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
                    turn_attribution: None,
                    turn_id: "first-turn".to_owned(),
                    root_turn_id: None,
                    trace_id: None,
                    started_at: Some(10),
                    model_context_window: None,
                    collaboration_mode_kind: Default::default(),
                })),
                RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
                    message: "first accepted input".to_owned(),
                    ..Default::default()
                })),
                RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
                    content_token_count: None,
                    thread_id,
                    turn_id: "first-turn".to_owned(),
                    item: TurnItem::UserMessage(UserMessageItem {
                        id: "first-user-item".to_owned(),
                        client_id: None,
                        content: vec![UserInput::Text {
                            text: "first accepted input".to_owned(),
                            text_elements: Vec::new(),
                        }],
                    }),
                    started_at_ms: Some(10),
                    completed_at_ms: 10,
                })),
            ])
            .await
    });
    tokio::time::timeout(Duration::from_secs(60), store.written.notified())
        .await
        .expect("first input did not reach durable storage");
    let stored = store.read_thread(read).await.unwrap();
    assert_eq!(stored.history_mode, history_mode);
    let path = stored.rollout_path.unwrap();
    assert!(codex_rollout::existing_rollout_path(&path).await.is_some());
    let metadata_existed = state_db.get_thread(thread_id).await.unwrap().is_some();
    let paginated = history_mode == ThreadHistoryMode::Paginated;
    if paginated {
        assert!(!metadata_existed, "first append has not published metadata");
        let error = super::super::ThreadRequestProcessor::paginated_resume_backwards_cursors(
            store.inner.as_ref(),
            thread_id,
        )
        .await
        .expect_err("a canonical header alone cannot satisfy cursor reads");
        assert_eq!(error.code, -32601);
    }

    prepare_when(active, paginated, || async {
        live.persist(PersistContext::ThreadPreparation)
            .await
            .map_err(std::io::Error::other)
    })
    .await
    .unwrap();
    if active && paginated {
        assert!(state_db.get_thread(thread_id).await.unwrap().is_some());
        let cursors = super::super::ThreadRequestProcessor::paginated_resume_backwards_cursors(
            store.inner.as_ref(),
            thread_id,
        )
        .await
        .expect("preparation must make both cursor reads available");
        assert!(cursors.0.is_some());
        assert!(cursors.1.is_some());
        assert_eq!(store.preparations.load(Ordering::SeqCst), 1);
    } else {
        assert_eq!(
            state_db.get_thread(thread_id).await.unwrap().is_some(),
            metadata_existed
        );
        assert_eq!(store.preparations.load(Ordering::SeqCst), 0);
    }
    store.release.notify_one();
    tokio::time::timeout(Duration::from_secs(60), append)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    live.shutdown().await.unwrap();
}

#[tokio::test]
async fn active_paginated_resume_prepares_first_input_metadata_before_cursors() {
    first_input_resume(true, ThreadHistoryMode::Paginated).await;
}

#[tokio::test]
async fn inactive_paginated_resume_keeps_upstream_persistence() {
    first_input_resume(false, ThreadHistoryMode::Paginated).await;
}

#[tokio::test]
async fn active_legacy_resume_does_not_prepare_projection() {
    first_input_resume(true, ThreadHistoryMode::Legacy).await;
}

#[tokio::test]
async fn preparation_preserves_typed_resume_errors() {
    for (error, code) in [
        (
            super::ThreadStoreError::InvalidRequest {
                message: "fixture".to_owned(),
            },
            -32600,
        ),
        (
            super::ThreadStoreError::Unsupported {
                operation: "fixture",
            },
            -32601,
        ),
        (
            super::ThreadStoreError::Internal {
                message: "fixture".to_owned(),
            },
            -32603,
        ),
    ] {
        let result = prepare_when(true, true, || async { Err(std::io::Error::other(error)) })
            .await
            .expect_err("preparation failure must reach the resume caller");
        assert_eq!(result.code, code);
    }
}
