// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual storage incarnation controls. No ID/marker supplies Core or Native authority.
use super::*;

#[tokio::test]
async fn discard_recreate_same_id_and_mode_denies_old_handle_and_clone() -> TestResult {
    let home = TempDir::new()?;
    let store = Arc::new(LocalThreadStore::new(test_config(home.path()), None));
    let id = ThreadId::new();
    let mut params = create_thread_params(id);
    params.history_mode = ThreadHistoryMode::Paginated;
    let original = LiveThread::create(store.clone(), params.clone()).await?;
    let clone = original.clone();
    let result: TestResult = async {
        check(
            original.retained_deferred_history().await?.is_some(),
            "original actual writer absent",
        )?;
        original.discard().await?;
        let replacement = LiveThread::create(store.clone(), params).await?;
        // Creation itself proves that retaining the original handle/clone's
        // identity did not keep its real writer/file lock alive.
        let result: TestResult = async {
            check(
                matches!(
                    original.retained_deferred_history().await,
                    Err(ThreadStoreError::Conflict { .. })
                ),
                "old handle followed a replacement writer",
            )?;
            check(
                matches!(
                    clone.retained_deferred_history().await,
                    Err(ThreadStoreError::Conflict { .. })
                ),
                "old clone followed a replacement writer",
            )?;
            let actual = replacement
                .retained_deferred_history()
                .await?
                .ok_or_else(|| std::io::Error::other("new actual deferred writer absent"))?;
            check(
                actual.thread_id == id
                    && actual.session_meta.id == id
                    && actual.session_meta.history_mode == ThreadHistoryMode::Paginated,
                "replacement identity/mode differs",
            )?;
            Ok(())
        }
        .await;
        let cleanup = replacement.discard().await;
        result?;
        cleanup?;
        Ok(())
    }
    .await;
    // On errors before replacement cleanup, still close whichever actual
    // registered writer this fixture created; never invoke the stale handle.
    let cleanup = store.discard_thread(id).await;
    result?;
    match cleanup {
        Ok(()) | Err(ThreadStoreError::ThreadNotFound { .. }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[tokio::test]
async fn actual_resume_returns_new_identity_and_preserves_materialized_none() -> TestResult {
    let home = TempDir::new()?;
    let store = Arc::new(LocalThreadStore::new(test_config(home.path()), None));
    let id = ThreadId::new();
    let params = create_thread_params(id);
    let metadata = params.metadata.clone();
    let original = LiveThread::create(store.clone(), params).await?;
    let result: TestResult = async {
        store.persist_thread(id, PersistContext::Standard).await?;
        let path = store.live_rollout_path(id).await?;
        original.shutdown().await?;
        let (resumed, history) = LiveThread::resume(
            store.clone(),
            ThreadHistoryMode::Legacy,
            crate::ResumeThreadParams {
                thread_id: id,
                rollout_path: Some(path),
                history: None,
                history_revision: None,
                include_archived: true,
                metadata,
            },
            None,
        )
        .await?;
        let result: TestResult = async {
            check(
                !history.is_empty(),
                "original resume lost actual durable creation metadata",
            )?;
            check(
                matches!(
                    original.retained_deferred_history().await,
                    Err(ThreadStoreError::Conflict { .. })
                ),
                "old handle followed reopened writer",
            )?;
            check(
                resumed.retained_deferred_history().await?.is_none(),
                "resumed durable writer became fabricated deferred history",
            )?;
            Ok(())
        }
        .await;
        let cleanup = resumed.discard().await;
        result?;
        cleanup?;
        Ok(())
    }
    .await;
    let cleanup = store.discard_thread(id).await;
    result?;
    match cleanup {
        Ok(()) | Err(ThreadStoreError::ThreadNotFound { .. }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[tokio::test]
async fn unsupported_store_runs_original_acquisition_once_and_supplies_no_snapshot() -> TestResult {
    let store = Arc::new(crate::InMemoryThreadStore::default());
    let id = ThreadId::new();
    let live = LiveThread::create(store.clone(), create_thread_params(id)).await?;
    let result: TestResult = async {
        check(
            live.retained_deferred_history().await?.is_none(),
            "unsupported store invented writer evidence",
        )?;
        check(
            store.calls().await.create_thread == 1,
            "companion repeated or bypassed original acquisition",
        )?;
        Ok(())
    }
    .await;
    let cleanup = live.discard().await;
    result?;
    cleanup?;
    Ok(())
}
