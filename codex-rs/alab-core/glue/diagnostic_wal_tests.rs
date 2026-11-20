// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::migrations::runtime_logs_migrator;
use codex_utils_absolute_path::test_support::PathExt;

async fn limit(connection: &mut SqliteConnection) -> Result<i64, Error> {
    sqlx::query_scalar("PRAGMA journal_size_limit")
        .fetch_one(connection)
        .await
}

#[tokio::test]
async fn diagnostic_wal_releases_burst_space_on_the_next_write() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let original = SqliteConfig::new_for_testing(home.path().abs());
    let moved = original
        .clone()
        .with_local_logs_home(home.path().join("logs"));
    for legacy in [false, true] {
        let config = if legacy { &original } else { &moved };
        let primary = config.open_logs_db(&runtime_logs_migrator(), None).await?;
        // Hold the first physical connection, including its reader snapshot. A later
        // writer must be configured independently, including legacy cleanup writers.
        let mut reader = primary.begin().await?;
        assert!((0..=LIMIT_BYTES).contains(&limit(&mut reader).await?));
        let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM logs");
        assert_eq!(count.fetch_one(&mut *reader).await?, 0);
        let cleanup = if legacy {
            moved.open_legacy_logs_for_cleanup().await?
        } else {
            None
        };
        assert_eq!(cleanup.is_some(), legacy);
        let mut writer = match &cleanup {
            Some(pool) => cleanup_writer(pool).await?,
            None => primary.acquire().await?,
        };
        sqlx::query("INSERT INTO logs(ts,ts_nanos,level,target,feedback_log_body) VALUES(1,0,'INFO','burst',zeroblob(8388608))")
            .execute(&mut *writer)
            .await?;
        let checkpoint = "PRAGMA wal_checkpoint(PASSIVE)";
        let (_, frames, done): (i64, i64, i64) =
            sqlx::query_as(checkpoint).fetch_one(&mut *writer).await?;
        assert!(frames > done);
        let wal = config.logs_db_path().with_extension("sqlite-wal");
        let peak = tokio::fs::metadata(&wal).await?.len();
        assert!(peak > LIMIT_BYTES as u64);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM logs")
                .fetch_one(&mut *reader)
                .await?,
            0
        );
        reader.rollback().await?;
        let (busy, frames, done): (i64, i64, i64) =
            sqlx::query_as(checkpoint).fetch_one(&mut *writer).await?;
        assert_eq!(busy, 0);
        assert_eq!(frames, done);
        assert_eq!(tokio::fs::metadata(&wal).await?.len(), peak);
        sqlx::query("INSERT INTO logs(ts,ts_nanos,level,target,feedback_log_body) VALUES(2,0,'INFO','small','x')")
            .execute(&mut *writer)
            .await?;
        assert!(tokio::fs::metadata(&wal).await?.len() <= LIMIT_BYTES as u64);
        let data: (i64, i64) =
            sqlx::query_as("SELECT count(*), sum(length(feedback_log_body)) FROM logs")
                .fetch_one(&mut *writer)
                .await?;
        assert_eq!(data, (2, 8_388_609));
        writer.return_to_pool().await;
        if let Some(cleanup) = cleanup {
            cleanup.close().await;
        }
        primary.close().await;
    }
    Ok(())
}

#[tokio::test]
async fn diagnostic_wal_preserves_stricter_limits_and_other_journals() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let state_path = sqlite.state_db_path();
    let state = sqlite.open_read_write_pool(&state_path).await?;
    let raw = sqlite.open_read_only_pool(&state_path, None).await?;
    let mut writer = state.acquire().await?;
    let mut observer = raw.acquire().await?;
    assert_eq!(limit(&mut writer).await?, limit(&mut observer).await?);
    observer.return_to_pool().await;
    raw.close().await;

    for setting in [
        "PRAGMA journal_size_limit=1048576",
        "PRAGMA journal_size_limit=0",
    ] {
        let expected: i64 = sqlx::query_scalar(setting).fetch_one(&mut *writer).await?;
        configure_existing(&mut writer).await?;
        assert_eq!(limit(&mut writer).await?, expected);
    }
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode=DELETE")
        .fetch_one(&mut *writer)
        .await?;
    assert_eq!(mode, "delete");
    sqlx::query("PRAGMA journal_size_limit=-1")
        .execute(&mut *writer)
        .await?;
    configure_existing(&mut writer).await?;
    assert_eq!(limit(&mut writer).await?, -1);
    writer.return_to_pool().await;
    state.close().await;
    Ok(())
}
