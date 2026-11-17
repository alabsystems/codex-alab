// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl GoalStore {
    /// Make every goal commit preceding this call durable before a separate
    /// fsynced chronology marker can be removed. DELETE-journal connections use
    /// synchronous=EXTRA; WAL/NORMAL requires an explicit completed checkpoint.
    pub async fn alab_durable_goal_barrier(&self) -> anyhow::Result<()> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(());
        }
        let mut connection = self.pool.acquire().await?;
        let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut *connection)
            .await?;
        let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
            .fetch_one(&mut *connection)
            .await?;
        match journal.to_ascii_lowercase().as_str() {
            "wal" => {
                anyhow::ensure!(
                    synchronous > 0,
                    "goal WAL durability is disabled by synchronous=OFF"
                );
                let (busy, frames, checkpointed): (i64, i64, i64) =
                    sqlx::query_as("PRAGMA wal_checkpoint(FULL)")
                        .fetch_one(&mut *connection)
                        .await?;
                anyhow::ensure!(
                    busy == 0 && checkpointed == frames,
                    "goal WAL durability checkpoint remained busy ({checkpointed}/{frames} frames)"
                );
            }
            "delete" => anyhow::ensure!(
                synchronous >= 3,
                "goal DELETE-journal durability requires synchronous=EXTRA"
            ),
            mode => anyhow::bail!(
                "unsupported goal durability journal mode `{mode}`; expected WAL or DELETE"
            ),
        }
        Ok(())
    }
}
