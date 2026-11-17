// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;

#[tokio::test]
async fn actual_writer_local_cut_preserves_deferred_materialized_and_resumed_records_without_forcing_persist()
-> Result<()> {
    for mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        let home = TempDir::new()?;
        let cfg = config(home.path());
        let id = ThreadId::new();
        let recorder = RolloutRecorder::new(&cfg, params(id).with_history_mode(mode)).await?;
        let result: Result<()> = async {
            recorder
                .record_canonical_items(&[item("actual first"), item("actual second")])
                .await?;
            let actual = recorder.retained_local_rollout(1 << 20).await?;
            ensure!(actual.session_meta.id == id && actual.session_meta.history_mode == mode);
            ensure!(
                serde_json::to_value(actual.items)?
                    == serde_json::to_value(vec![item("actual first"), item("actual second")])?
            );
            ensure!(
                !tokio::fs::try_exists(recorder.rollout_path()).await?,
                "writer observation materialized deferred state"
            );
            recorder.persist().await?;
            let before = recorder.retained_local_rollout(1 << 20).await?;
            recorder
                .record_canonical_items(&[item("actual third")])
                .await?;
            recorder.flush().await?;
            let after = recorder.retained_local_rollout(1 << 20).await?;
            ensure!(
                before.items.len() == 2 && after.items.len() == 3,
                "original writer cutoff changed or following append was lost"
            );
            ensure!(
                serde_json::to_value(after.items)?
                    == serde_json::to_value(vec![
                        item("actual first"),
                        item("actual second"),
                        item("actual third")
                    ])?
            );
            Ok(())
        }
        .await;
        let path = recorder.rollout_path().to_path_buf();
        let cleanup = recorder.shutdown().await;
        result?;
        cleanup?;
        ensure!(
            recorder.retained_local_rollout(1 << 20).await.is_err(),
            "stopped writer invented empty history"
        );
        let resumed = RolloutRecorder::new(&cfg, RolloutRecorderParams::resume(path)).await?;
        let result = resumed.retained_local_rollout(1 << 20).await;
        let cleanup = resumed.shutdown().await;
        ensure!(
            result?.items.len() == 3,
            "original resumed writer lost its actual local records"
        );
        cleanup?;
    }
    Ok(())
}

#[tokio::test]
async fn capacity_and_cancelled_actual_writer_reads_preserve_original_writer_and_records()
-> Result<()> {
    let home = TempDir::new()?;
    let recorder = RolloutRecorder::new(&config(home.path()), params(ThreadId::new())).await?;
    let result: Result<()> = async {
        recorder
            .record_canonical_items(&[item("actual retained record")])
            .await?;
        ensure!(
            recorder.retained_local_rollout(1).await.is_err(),
            "capacity error became partial/empty history"
        );
        let (ack, done) = tokio::sync::oneshot::channel();
        let (stop, keep_running) = tokio::sync::oneshot::channel::<()>();
        drop(keep_running);
        recorder
            .tx
            .send(RolloutCmd::ReadRetainedLocalRollout {
                ack,
                stop,
                max_bytes: 1 << 20,
            })
            .await?;
        ensure!(
            done.await?.is_err(),
            "cancelled original observation read became success"
        );
        ensure!(!tokio::fs::try_exists(recorder.rollout_path()).await?);
        recorder.persist().await?;
        ensure!(
            recorder.retained_local_rollout(1).await.is_err(),
            "materialized capacity error became success"
        );
        recorder
            .record_canonical_items(&[item("actual writer remains usable")])
            .await?;
        recorder.flush().await?;
        ensure!(recorder.retained_local_rollout(1 << 20).await?.items.len() == 2);
        Ok(())
    }
    .await;
    let cleanup = recorder.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}
