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
                checkpoint_wal(&mut connection).await?;
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

async fn checkpoint_wal(connection: &mut SqliteConnection) -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let mut lock_budget = None;
    let mut delay = std::time::Duration::from_millis(1);
    loop {
        let (busy, frames, checkpointed): (i64, i64, i64) =
            sqlx::query_as("PRAGMA main.wal_checkpoint(FULL)")
                .fetch_one(&mut *connection)
                .await?;
        if (busy, frames, checkpointed) != (1, -1, -1) {
            anyhow::ensure!(
                busy == 0 && frames >= 0 && checkpointed == frames,
                "goal WAL durability checkpoint incomplete (busy={busy}, {checkpointed}/{frames} frames)"
            );
            return Ok(());
        }

        // A competing checkpoint owns SQLite's checkpoint lock. SQLite skips its busy
        // handler for this lock, even with busy_timeout configured. Retry only the barrier.
        let budget = match lock_budget {
            Some(budget) => budget,
            None => {
                let timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
                    .fetch_one(&mut *connection)
                    .await?;
                let budget = std::time::Duration::from_millis(u64::try_from(timeout)?);
                lock_budget = Some(budget);
                budget
            }
        };
        wait_to_retry_checkpoint(started, budget, delay).await?;
        delay = (delay * 2).min(std::time::Duration::from_millis(32));
    }
}

/// Bound admission to a new FULL attempt. SQLite owns the busy-handler wait of an admitted attempt.
async fn wait_to_retry_checkpoint(
    started: std::time::Instant,
    budget: std::time::Duration,
    delay: std::time::Duration,
) -> anyhow::Result<()> {
    let remaining = budget.saturating_sub(started.elapsed());
    if !remaining.is_zero() {
        tokio::time::sleep(delay.min(remaining)).await;
    }
    anyhow::ensure!(
        started.elapsed() < budget,
        "goal WAL durability checkpoint lock remained busy for {} ms without frame counts",
        budget.as_millis()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Connection;
    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::sqlite::SqliteJournalMode;
    use sqlx::sqlite::SqliteSynchronous;

    #[tokio::test]
    async fn checkpoint_retry_is_refused_when_its_sleep_consumes_the_remaining_budget() {
        let budget = std::time::Duration::from_millis(5);
        let started = std::time::Instant::now();
        assert!(
            wait_to_retry_checkpoint(started, budget, budget)
                .await
                .is_err()
        );
        assert!(
            wait_to_retry_checkpoint(started, budget, std::time::Duration::ZERO)
                .await
                .is_err()
        );
        assert!(
            wait_to_retry_checkpoint(
                std::time::Instant::now(),
                std::time::Duration::from_secs(60),
                std::time::Duration::ZERO
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn checkpoint_contention_waits_only_for_the_barrier_and_keeps_unknown_commits_held() {
        let scratch = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(scratch.path().join("goals.sqlite"))
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(5));
        #[expect(
            clippy::disallowed_methods,
            reason = "the fixture needs independent physical WAL connections"
        )]
        let mut writer = SqliteConnection::connect_with(&options).await.unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "the fixture must own the competing checkpoint lock"
        )]
        let mut competitor = SqliteConnection::connect_with(&options).await.unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "the fixture observes checkpoint contention on a third connection"
        )]
        let mut barrier = SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::query(
            "CREATE TABLE usage (tokens INTEGER); INSERT INTO usage VALUES (3); BEGIN IMMEDIATE",
        )
        .execute(&mut writer)
        .await
        .unwrap();

        let blocking_checkpoint = tokio::spawn(async move {
            // The passive observation below can itself briefly own the checkpoint lock.
            loop {
                let result: (i64, i64, i64) = sqlx::query_as("PRAGMA main.wal_checkpoint(FULL)")
                    .fetch_one(&mut competitor)
                    .await
                    .unwrap();
                if result != (1, -1, -1) {
                    return result;
                }
                tokio::task::yield_now().await;
            }
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let state: (i64, i64, i64) = sqlx::query_as("PRAGMA main.wal_checkpoint(PASSIVE)")
                    .fetch_one(&mut barrier)
                    .await
                    .unwrap();
                if state == (1, -1, -1) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("observe the real competing checkpoint while its writer lock is held");

        sqlx::query("PRAGMA busy_timeout = 30")
            .execute(&mut barrier)
            .await
            .unwrap();
        let start = std::time::Instant::now();
        let failure = checkpoint_wal(&mut barrier).await.unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("checkpoint lock remained busy"),
            "{failure}"
        );
        assert!(start.elapsed() >= std::time::Duration::from_millis(30));
        sqlx::query("PRAGMA busy_timeout = 1000")
            .execute(&mut barrier)
            .await
            .unwrap();
        {
            let pending = checkpoint_wal(&mut barrier);
            tokio::pin!(pending);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(30), &mut pending)
                    .await
                    .is_err()
            );
            sqlx::query("ROLLBACK").execute(&mut writer).await.unwrap();
            pending.await.unwrap();
        }
        let (busy, frames, checkpointed) = blocking_checkpoint.await.unwrap();
        assert_eq!(busy, 0);
        assert_eq!(frames, checkpointed);
        let usage: Vec<i64> = sqlx::query_scalar("SELECT tokens FROM usage")
            .fetch_all(&mut barrier)
            .await
            .unwrap();
        assert_eq!(usage, vec![3]);
    }
}
