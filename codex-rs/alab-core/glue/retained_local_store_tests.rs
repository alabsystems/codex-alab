// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;

#[tokio::test]
async fn actual_local_cut_denies_old_same_id_acquisition_after_real_discard_and_recreate()
-> TestResult {
    let home = TempDir::new()?;
    let store = Arc::new(LocalThreadStore::new(test_config(home.path()), None));
    let id = ThreadId::new();
    let original = LiveThread::create(store.clone(), create_thread_params(id)).await?;
    let result: TestResult = async {
        check(
            original.retained_local_history(1 << 20).await?.thread_id == id,
            "actual original writer missing",
        )?;
        store.discard_thread(id).await?;
        let replacement = LiveThread::create(store.clone(), create_thread_params(id)).await?;
        check(
            matches!(
                original.retained_local_history(1 << 20).await,
                Err(ThreadStoreError::Conflict { .. })
            ),
            "old acquisition followed replacement writer by logical ID",
        )?;
        check(
            replacement.retained_local_history(1 << 20).await?.thread_id == id,
            "actual replacement acquisition was denied",
        )?;
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
async fn resumed_actual_writer_keeps_distinct_root_thread_and_immutable_rollout_identity()
-> TestResult {
    let home = TempDir::new()?;
    let config = test_config(home.path());
    let thread = ThreadId::new();
    let root = ThreadId::new();
    let rollout = ThreadId::new();
    check(
        thread != rollout && thread != root && rollout != root,
        "fixture identities unexpectedly collided",
    )?;
    let recorder = codex_rollout::RolloutRecorder::new(
        &codex_rollout::RolloutConfig {
            codex_home: config.codex_home.clone(),
            sqlite: config.sqlite.clone(),
            cwd: home.path().to_path_buf(),
            model_provider_id: config.default_model_provider_id.clone(),
            generate_memories: false,
        },
        codex_rollout::RolloutRecorderParams::new(
            thread,
            None,
            None,
            SessionSource::Exec,
            None,
            "original distinct-rollout fixture".into(),
            BaseInstructions::default(),
            Vec::new(),
        )
        .with_session_id(root.into())
        .with_history_mode(ThreadHistoryMode::Paginated)
        .with_rollout_id(rollout),
    )
    .await?;
    let path = recorder.rollout_path().to_path_buf();
    let persisted = recorder.persist().await;
    let retired = recorder.shutdown().await; // Always drain actual writer, including persistence errors.
    persisted?;
    retired?;
    check(
        codex_rollout::rollout_id_from_path(&path) == Some(rollout),
        "original writer did not create its explicit immutable rollout",
    )?;
    let store = Arc::new(LocalThreadStore::new(config, None));
    let result: TestResult = async {
        let (live, _) = LiveThread::resume(
            store.clone(),
            ThreadHistoryMode::Paginated,
            ResumeThreadParams {
                history_revision: None,
                thread_id: thread,
                rollout_path: Some(path.clone()),
                history: None,
                include_archived: false,
                metadata: ThreadPersistenceMetadata {
                    cwd: Some(home.path().to_path_buf()),
                    model_provider: "test-provider".into(),
                    memory_mode: ThreadMemoryMode::Disabled,
                },
            },
            None,
        )
        .await?;
        let before = tokio::fs::metadata(&path).await?.len();
        let actual = live.retained_local_history(1 << 20).await?;
        check(
            actual.thread_id == thread && actual.session_meta.id == thread,
            "local cut changed logical thread",
        )?;
        check(
            actual.session_meta.session_id == root.into(),
            "local cut replaced actual root session",
        )?;
        check(
            actual.rollout_id == rollout && actual.rollout_id != actual.thread_id,
            "local cut guessed logical/root id instead of same registration rollout",
        )?;
        check(
            actual.session_meta.history_mode == ThreadHistoryMode::Paginated,
            "local cut changed original history mode",
        )?;
        check(
            tokio::fs::metadata(&path).await?.len() == before,
            "writer observation appended or forced another rollout",
        )?;
        check(
            store.live_rollout_path(thread).await? == path,
            "local observation selected a different writer path",
        )?;
        Ok(())
    }
    .await;
    let cleanup = store.shutdown_thread(thread).await;
    result?;
    match cleanup {
        Ok(()) | Err(ThreadStoreError::ThreadNotFound { .. }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}
