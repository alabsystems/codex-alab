// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Unknown goal/thread reads cannot withdraw an acknowledged session description.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use codex_state::ThreadGoalStatus;
use sqlx::Connection;

/// Change only this scratch row's decoding; leave the database and other rows readable.
pub(super) async fn saved_status(db: &codex_state::StateRuntime, id: ThreadId, status: &str) {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(db.sqlite().goals_db_path())
        .pragma("ignore_check_constraints", "ON");
    #[expect(
        clippy::disallowed_methods,
        reason = "this isolated test connection injects and restores a scratch goal row decoding failure"
    )]
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .expect("scratch goal connection");
    sqlx::query("UPDATE thread_goals SET status = ? WHERE thread_id = ?")
        .bind(status)
        .bind(id.to_string())
        .execute(&mut connection)
        .await
        .expect("change only this scratch goal's status");
    connection.close().await.expect("close scratch connection");
}

#[test]
fn a_real_goal_read_fault_preserves_the_known_description_but_absent_and_complete_clear_it() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut results = Vec::new();
            for step in [None, Some("the current plan step")] {
                let h = Harness::new().await;
                h.immediate_point().await;
                let db = h.test.codex.state_db().expect("native goal database");
                db.thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "keep this known objective",
                        ThreadGoalStatus::Active,
                        None,
                    )
                    .await
                    .expect("persist an active native goal");
                h.rt.set_plan_step(step);
                h.autopilot.refresh_description(&h.rt).await;
                let known = crate::describe::describe(Some("keep this known objective"), step);
                assert_eq!(h.fake.description(), known, "healthy goal read publishes");

                saved_status(&db, h.rt.id, "unreadable-description-status").await;
                let error = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect_err("this persisted goal row really cannot decode")
                    .to_string();
                h.autopilot.refresh_description(&h.rt).await;
                let failed = h.fake.description();
                // Restore a valid row before every authoritative control and teardown.
                saved_status(&db, h.rt.id, "active").await;
                h.autopilot.refresh_description(&h.rt).await;

                db.thread_goals()
                    .delete_thread_goal(h.rt.id)
                    .await
                    .expect("healthy absent control");
                h.autopilot.refresh_description(&h.rt).await;
                let absent = h.fake.description();
                db.thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "keep this known objective",
                        ThreadGoalStatus::Active,
                        None,
                    )
                    .await
                    .expect("restore an active objective");
                h.autopilot.refresh_description(&h.rt).await;
                db.thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "keep this known objective",
                        ThreadGoalStatus::Complete,
                        None,
                    )
                    .await
                    .expect("healthy complete control");
                h.autopilot.refresh_description(&h.rt).await;
                let complete = h.fake.description();
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native description goal fault shutdown");
                results.push((step, known, failed, absent, complete, error));
            }
            for (step, known, failed, absent, complete, error) in results {
                assert_eq!(
                    failed, known,
                    "unknown goal leaves acknowledged text standing: {error}"
                );
                let no_goal = crate::describe::describe(None, step);
                assert_eq!(
                    absent, no_goal,
                    "authoritative absence removes the old goal"
                );
                assert_eq!(
                    complete, no_goal,
                    "authoritative completion removes the old goal"
                );
            }
        })
        .await
        .expect("native description goal fault hang detector");
    });
}

#[test]
fn an_unavailable_thread_manager_preserves_the_acknowledged_goal_description() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            let db = h.test.codex.state_db().expect("native goal database");
            db.thread_goals()
                .replace_thread_goal(
                    h.rt.id,
                    "keep the known thread's objective",
                    ThreadGoalStatus::Active,
                    None,
                )
                .await
                .expect("persist the native goal");
            h.autopilot.refresh_description(&h.rt).await;
            let known = h.fake.description();
            assert_eq!(
                known.as_deref(),
                Some("goal: keep the known thread's objective")
            );
            h.autopilot.bind_thread_manager(Weak::new());
            h.autopilot.refresh_description(&h.rt).await;
            let unknown = h.fake.description();
            h.autopilot
                .bind_thread_manager(Arc::downgrade(&h.test.thread_manager));
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native manager fault shutdown");
            assert_eq!(
                unknown, known,
                "an unavailable manager cannot certify an absent goal"
            );
        })
        .await
        .expect("native description manager fault hang detector");
    });
}

/// A claim transition refreshes the goal from a real manager and database even
/// though no native turn starts and no plan step moves.
#[test]
fn a_description_a_standby_withdrew_is_written_back_when_acting_resumes() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            h.rt.with_state(|state| *state = ThreadState::default());
            h.autopilot.watching.store(false, Ordering::SeqCst);
            h.autopilot.watch_claims();
            h.fake.set_effective(false, "suspended");
            upgrade_handoff_tests::wait_for_supervisor(|| !h.autopilot.sup.acting()).await;
            h.fake.set_effective(true, "enabled");
            upgrade_handoff_tests::wait_for_supervisor(|| h.autopilot.sup.acting()).await;
            h.autopilot.refresh_description(&h.rt).await;
            assert!(
                h.fake.descriptions().is_empty(),
                "a healthy absent goal writes nothing"
            );

            h.rt.set_plan_step(Some("write the inbox"));
            h.autopilot.refresh_description(&h.rt).await;
            let text = crate::describe::describe(None, Some("write the inbox"));
            assert_eq!(h.fake.description(), text);
            for off in ["suspended", "autopilot-off"] {
                h.fake.set_effective(false, off);
                assert!(
                    h.fake.wait_for(|fake| fake.description().is_none()).await,
                    "{off}: withdrawn"
                );
                h.fake.set_effective(true, "enabled");
                assert!(
                    h.fake.wait_for(|fake| fake.description() == text).await,
                    "{off}: automatically refreshed"
                );
            }
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native claim refresh shutdown");
        })
        .await
        .expect("native claim description refresh hang detector");
    });
}
