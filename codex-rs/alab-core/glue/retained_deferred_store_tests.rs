// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[cfg(unix)]
#[path = "retained_local_store_tests.rs"]
mod actual_local_writer_cut_tests;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}

#[tokio::test]
async fn same_registered_writer_snapshot_is_not_invented_by_a_cold_store_or_after_shutdown()
-> TestResult {
    let home = TempDir::new()?;
    let store = Arc::new(LocalThreadStore::new(test_config(home.path()), None));
    let id = ThreadId::new();
    let mut params = create_thread_params(id);
    params.history_mode = ThreadHistoryMode::Paginated;
    let identity = store
        .create_thread_with_retained_writer(params)
        .await?
        .ok_or_else(|| std::io::Error::other("actual local acquisition identity missing"))?;
    let result: TestResult = async {
        let observed = store.retained_deferred_history(id, identity.clone()).await?.ok_or_else(|| std::io::Error::other("actual live deferred writer"))?;
        check(observed.thread_id == id && observed.rollout_id == id && observed.session_meta.id == id, "snapshot lost actual writer identity")?;
        check(observed.session_meta.history_mode == ThreadHistoryMode::Paginated && observed.items.is_empty(), "fresh writer metadata or local buffer differs")?;
        let path = store.live_rollout_path(id).await?;
        check(!tokio::fs::try_exists(&path).await?, "observation materialized the rollout")?;
        let cold = LocalThreadStore::new(test_config(home.path()), None);
        check(matches!(cold.retained_deferred_history(id, identity.clone()).await, Err(ThreadStoreError::ThreadNotFound { thread_id }) if thread_id == id), "cold store invented a live writer")?;
        store.discard_thread(id).await?;
        check(matches!(store.retained_deferred_history(id, identity.clone()).await, Err(ThreadStoreError::ThreadNotFound { thread_id }) if thread_id == id), "discarded writer remained observable")?;
        check(!tokio::fs::try_exists(path).await?, "discarded deferred rollout was created")?;
        Ok(())
    }.await;
    // If the body failed before discard, still retire the same actual writer.
    let cleanup = store.discard_thread(id).await;
    result?;
    match cleanup {
        Ok(()) | Err(ThreadStoreError::ThreadNotFound { .. }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[tokio::test]
async fn real_local_materialization_stops_deferred_observation() -> TestResult {
    let home = TempDir::new()?;
    let store = Arc::new(LocalThreadStore::new(test_config(home.path()), None));
    let id = ThreadId::new();
    let live = LiveThread::create(store.clone(), create_thread_params(id)).await?;
    let result: TestResult = async {
        check(
            live.retained_deferred_history().await?.is_some(),
            "actual deferred writer absent",
        )?;
        store.persist_thread(id, PersistContext::Standard).await?;
        check(
            live.retained_deferred_history().await?.is_none(),
            "materialized writer still reported deferred",
        )?;
        let path = store.live_rollout_path(id).await?;
        check(
            tokio::fs::try_exists(path).await?,
            "real materialization did not create rollout",
        )?;
        Ok(())
    }
    .await;
    let cleanup = live.discard().await;
    result?;
    cleanup?;
    Ok(())
}

#[path = "retained_writer_generation_tests.rs"]
mod generation_tests;

#[path = "retained_deferred_lineage_tests.rs"]
mod lineage_tests;
