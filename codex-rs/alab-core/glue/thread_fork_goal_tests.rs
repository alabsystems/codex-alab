// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_utils_absolute_path::test_support::PathExt;
use sqlx::Connection;
use tempfile::TempDir;

#[test]
fn inherited_goal_snapshot_requires_a_durable_wal_boundary() {
    const CHILD: &str = "ALAB_TEST_FORK_GOAL_DURABILITY_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let exact = std::thread::current()
            .name()
            .expect("libtest names the fork durability test")
            .to_owned();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", exact.as_str(), "--nocapture"])
            .env(CHILD, "1")
            .output()
            .expect("run isolated fork durability test");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "isolated fork durability test failed\nstdout:\n{}\nstderr:\n{}",
            stdout,
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    codex_alab_core::activate();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let home = TempDir::new().unwrap();
            let state_db = StateRuntime::init(
                codex_state::SqliteConfig::new_for_testing(home.path().abs()),
                "test_provider".to_owned(),
            )
            .await
            .unwrap();
            let source = ThreadId::new();
            let target = ThreadId::new();
            state_db
                .thread_goals()
                .replace_thread_goal(
                    source,
                    "copy only across a durable fork boundary",
                    codex_state::ThreadGoalStatus::Paused,
                    None,
                )
                .await
                .unwrap();
            state_db
                .thread_goals()
                .alab_durable_goal_barrier()
                .await
                .unwrap();

            let options = sqlx::sqlite::SqliteConnectOptions::new()
                .filename(state_db.sqlite().goals_db_path());
            #[expect(
                clippy::disallowed_methods,
                reason = "owned scratch reader deterministically blocks the fork-inheritance WAL checkpoint"
            )]
            let mut reader = sqlx::SqliteConnection::connect_with(&options)
                .await
                .unwrap();
            sqlx::query("BEGIN").execute(&mut reader).await.unwrap();
            let visible: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM thread_goals WHERE thread_id = ?")
                    .bind(source.to_string())
                    .fetch_one(&mut reader)
                    .await
                    .unwrap();
            assert_eq!(visible, 1, "reader must pin the pre-copy snapshot");

            assert!(
                inherit_thread_goal_snapshot(state_db.as_ref(), source, target)
                    .await
                    .is_err(),
                "a busy checkpoint must not acknowledge inherited goal state"
            );
            assert!(
                state_db
                    .thread_goals()
                    .get_thread_goal(target)
                    .await
                    .unwrap()
                    .is_some(),
                "the copied row became live-visible before its barrier failed"
            );

            reader.close().await.unwrap();
            state_db.close().await;
        });
}
