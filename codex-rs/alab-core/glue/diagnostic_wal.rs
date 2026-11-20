// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep a diagnostic WAL burst from occupying its peak size on every subsequent reset.
//! A reader can pin any amount of live data. SQLite enforces this limit only after
//! checkpointing, when a writer can reuse the start of the file.

use crate::SqliteConfig;
use sqlx::Error;
use sqlx::Sqlite;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use sqlx::pool::PoolConnection;
use std::path::Path;

// One ordinary 1,000-frame checkpoint at the default 4 KiB page size fits.
const LIMIT_BYTES: i64 = 4 * 1024 * 1024;

impl SqliteConfig {
    pub(crate) fn is_diagnostic_database(&self, path: &Path) -> bool {
        path == self.logs_db_path() || self.legacy_logs_db_path().as_deref() == Some(path)
    }
}

// Legacy cleanup uses a one-connection pool with no busy wait. After acquiring its
// writer, return any contention directly; SQLx retries errors from after_connect.
pub(crate) async fn cleanup_writer(pool: &SqlitePool) -> Result<PoolConnection<Sqlite>, Error> {
    let mut writer = pool.acquire().await?;
    configure_existing(&mut writer).await?;
    Ok(writer)
}

async fn configure_existing(connection: &mut SqliteConnection) -> Result<(), Error> {
    let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut *connection)
        .await?;
    configure(connection, true, &journal).await
}

pub(crate) async fn configure(
    connection: &mut SqliteConnection,
    diagnostics: bool,
    journal: &str,
) -> Result<(), Error> {
    if diagnostics && journal == "wal" {
        let current: i64 = sqlx::query_scalar("PRAGMA journal_size_limit")
            .fetch_one(&mut *connection)
            .await?;
        if !(0..=LIMIT_BYTES).contains(&current) {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "PRAGMA journal_size_limit={LIMIT_BYTES}"
            )))
            .execute(connection)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "diagnostic_wal_tests.rs"]
mod tests;
