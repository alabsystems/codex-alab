// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::RolloutConfig;
use crate::RolloutRecorderParams;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_protocol::ThreadId;
use codex_protocol::models::BaseInstructions;
use codex_protocol::protocol::AgentMessageEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HistoryPosition;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_utils_absolute_path::test_support::PathExt;
use std::path::Path;
use tempfile::TempDir;

#[cfg(unix)]
#[path = "retained_local_rollout_tests.rs"]
mod actual_local_writer_cut_tests;

fn config(home: &Path) -> RolloutConfig {
    RolloutConfig {
        codex_home: home.to_path_buf(),
        sqlite: codex_state::SqliteConfig::new_for_testing(home.abs()),
        cwd: home.to_path_buf(),
        model_provider_id: "deferred-history-test".to_string(),
        generate_memories: false,
    }
}

fn params(id: ThreadId) -> RolloutRecorderParams {
    RolloutRecorderParams::new(
        id,
        None,
        None,
        SessionSource::Exec,
        None,
        "deferred-history-test".to_string(),
        BaseInstructions::default(),
        Vec::new(),
    )
    .with_history_mode(ThreadHistoryMode::Paginated)
}

fn item(text: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::AgentMessage(AgentMessageEvent {
        moderation: None,
        message: text.to_string(),
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    }))
}

#[tokio::test]
async fn actual_writer_snapshot_preserves_metadata_all_pending_items_and_order() -> Result<()> {
    let home = TempDir::new()?;
    let id = ThreadId::new();
    let recorder = RolloutRecorder::new(&config(home.path()), params(id)).await?;
    let result: Result<()> = async {
        let empty = recorder
            .retained_deferred_history()
            .await?
            .context("new actual deferred writer")?;
        ensure!(empty.session_meta.id == id && empty.items.is_empty());
        let original_meta = recorder
            .unpersisted_session_meta()
            .await?
            .context("actual creation metadata")?;
        recorder.record_canonical_items(&[item("first")]).await?;
        recorder
            .record_canonical_items(&[item("second"), item("third")])
            .await?;
        let observed = recorder
            .retained_deferred_history()
            .await?
            .context("same deferred writer")?;
        ensure!(
            serde_json::to_value(&observed.session_meta)? == serde_json::to_value(original_meta)?
        );
        ensure!(
            serde_json::to_value(&observed.items)?
                == serde_json::to_value(vec![item("first"), item("second"), item("third")])?
        );
        ensure!(
            !tokio::fs::try_exists(recorder.rollout_path()).await?,
            "snapshot must not create rollout"
        );
        Ok(())
    }
    .await;
    let cleanup = recorder.discard().await;
    result?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn materialized_and_resumed_are_none_but_stopped_writer_is_error() -> Result<()> {
    let home = TempDir::new()?;
    let cfg = config(home.path());
    let recorder = RolloutRecorder::new(&cfg, params(ThreadId::new())).await?;
    let result: Result<()> = async {
        recorder.persist().await?;
        ensure!(recorder.retained_deferred_history().await?.is_none());
        Ok(())
    }
    .await;
    let path = recorder.rollout_path().to_path_buf();
    let cleanup = recorder.shutdown().await;
    result?;
    cleanup?;
    ensure!(
        recorder.retained_deferred_history().await.is_err(),
        "stopped is not empty history"
    );
    let resumed = RolloutRecorder::new(&cfg, RolloutRecorderParams::resume(path)).await?;
    let result = resumed.retained_deferred_history().await;
    let cleanup = resumed.shutdown().await;
    ensure!(result?.is_none());
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn actual_failed_persist_denies_snapshot_until_original_writer_recovers() -> Result<()> {
    let home = TempDir::new()?;
    let recorder = RolloutRecorder::new(&config(home.path()), params(ThreadId::new())).await?;
    let blocker = home.path().join("sessions");
    let result: Result<()> = async {
        recorder
            .record_canonical_items(&[item("retained-after-error")])
            .await?;
        std::fs::File::create(&blocker)?;
        ensure!(
            recorder.persist().await.is_err(),
            "real blocked directory must fail IO"
        );
        ensure!(
            recorder.retained_deferred_history().await.is_err(),
            "unresolved IO is not a safe snapshot"
        );
        std::fs::remove_file(&blocker)?;
        recorder.flush().await?;
        ensure!(recorder.retained_deferred_history().await?.is_none());
        let text = tokio::fs::read_to_string(recorder.rollout_path()).await?;
        ensure!(text.contains("retained-after-error"));
        Ok(())
    }
    .await;
    let unblock = if blocker.is_file() {
        std::fs::remove_file(&blocker)
    } else {
        Ok(())
    };
    let cleanup = recorder.discard().await;
    result?;
    unblock?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn deferred_fork_keeps_real_inherited_boundary_without_claiming_buffer_is_full_history()
-> Result<()> {
    let home = TempDir::new()?;
    let parent = ThreadId::new();
    let base = HistoryPosition {
        thread_id: parent,
        end_ordinal_exclusive: 41,
        end_byte_offset: 123,
    };
    let fork = RolloutRecorderParams::new(
        ThreadId::new(),
        Some(parent),
        None,
        SessionSource::Exec,
        None,
        "deferred-history-test".to_string(),
        BaseInstructions::default(),
        Vec::new(),
    )
    .with_history_mode(ThreadHistoryMode::Paginated)
    .with_history_base(Some(base));
    let recorder = RolloutRecorder::new(&config(home.path()), fork).await?;
    let result: Result<()> = async {
        recorder
            .record_canonical_items(&[item("local-child-record")])
            .await?;
        let observed = recorder
            .retained_deferred_history()
            .await?
            .context("deferred fork writer")?;
        ensure!(observed.session_meta.history_base == Some(base));
        ensure!(
            serde_json::to_value(observed.items)?
                == serde_json::to_value(vec![item("local-child-record")])?
        );
        ensure!(!tokio::fs::try_exists(recorder.rollout_path()).await?);
        Ok(())
    }
    .await;
    let cleanup = recorder.discard().await;
    result?;
    cleanup?;
    Ok(())
}
