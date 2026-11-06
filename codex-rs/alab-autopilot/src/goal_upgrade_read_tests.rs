// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A saved goal the upgrade cannot read is unknown, rather than absent. The
//! displayed idle root cannot protect a different loaded root's persisted work.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use codex_alab_core::session::Frame;
use codex_alab_core::session::ProgramState;
use core_test_support::responses::received_responses_requests;
use sqlx::Connection;

/// Make just this scratch goal undecodable, without making an absent goal in
/// the displayed root unreadable. The same connection restores a valid status.
async fn saved_status(db: &codex_state::StateRuntime, id: ThreadId, status: &str) {
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
        .expect("change only the scratch goal row");
    connection.close().await.expect("close scratch connection");
}

#[test]
fn a_failed_goal_read_in_a_hidden_loaded_root_holds_the_upgrade() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let goals = Arc::new(GoalService::new());
            let h = Harness::new_with_goals(Some(Arc::clone(&goals))).await;
            let hidden = h
                .test
                .thread_manager
                .start_thread(codex_core::StartThreadOptions {
                    environments: Some(Vec::new()),
                    ..codex_core::StartThreadOptions::new(h.test.config.clone())
                })
                .await
                .expect("second native root");
            let rt = hidden
                .thread
                .thread_extension_data()
                .get::<ThreadRt>()
                .expect("hidden root runtime");
            let rollout = h
                .shared
                .view()
                .primary()
                .and_then(|facts| facts.rollout_path.clone())
                .expect("materialized primary rollout");
            for displayed in [rt.key(), h.rt.key()] {
                h.shared.observe(
                    &Frame {
                        primary_thread: Some(h.rt.key()),
                        displayed_thread: Some(displayed),
                        rollout_path: Some(&rollout),
                        cwd: h.test.cwd_path(),
                        model: "gpt-5.5",
                        codex_home: h.test.codex_home_path(),
                        embedded: true,
                        state: ProgramState::Idle,
                        draft_empty: true,
                        queued_input: false,
                        popup_open: false,
                        editor_open: false,
                        pending_user_turn: false,
                    },
                    now_ms(),
                );
            }
            assert!(h.shared.is_root_thread(rt.key()));
            assert_ne!(h.shared.view().displayed_thread.as_deref(), Some(rt.key()));
            let db = hidden.thread.state_db().expect("hidden root goal database");
            let displayed_db = h.test.codex.state_db().expect("displayed root database");

            // A healthy empty store grants an update; the same native manager must
            // then refuse a persisted Active goal, even while its root is hidden.
            let empty = h.autopilot.upgrade_facts().await;
            goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id: hidden.thread_id,
                        objective: GoalObjectiveUpdate::Set("preserve this root's active work"),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Keep,
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("real goal service persists the hidden root's active goal");
            let active = h.autopilot.upgrade_facts().await;
            let saved = db
                .thread_goals()
                .get_thread_goal(hidden.thread_id)
                .await
                .expect("healthy active goal read")
                .expect("persisted goal");
            assert_eq!(saved.status, codex_state::ThreadGoalStatus::Active);

            // Force a real hidden-row decoding error. The native thread manager
            // shares a store, so the displayed root's absent row stays readable.
            saved_status(&db, hidden.thread_id, "unreadable-test-status").await;
            let read_error = db
                .thread_goals()
                .get_thread_goal(hidden.thread_id)
                .await
                .expect_err("the hidden goal row really fails to decode")
                .to_string();
            let unknown = h.autopilot.upgrade_facts().await;
            let displayed_healthy = displayed_db
                .thread_goals()
                .get_thread_goal(h.rt.id)
                .await;
            let displayed_state = h.shared.view().state;
            saved_status(&db, hidden.thread_id, "active").await;
            let hidden_shutdown = hidden.thread.shutdown_and_wait().await;
            let displayed_shutdown = h.test.codex.shutdown_and_wait().await;

            hidden_shutdown.expect("hidden native root shutdown");
            displayed_shutdown.expect("displayed native root shutdown");
            assert_eq!(upgrade::check(&empty), upgrade::UpgradeCheck::Go);
            assert!(matches!(upgrade::check(&active), upgrade::UpgradeCheck::Wait(why)
                if why.contains(rt.key()) && why.contains("active goal")));
            assert!(displayed_healthy.is_ok(), "the displayed store remains readable");
            assert_eq!(displayed_state, ProgramState::Idle);
            assert!(
                matches!(upgrade::check(&unknown), upgrade::UpgradeCheck::Wait(_)),
                "an unreadable saved goal must hold the upgrade: query={read_error}; facts={unknown:?}"
            );
        })
        .await
        .expect("hidden goal read upgrade hang detector");
    });
}

#[test]
fn a_failed_goal_read_holds_native_submission_and_owes_a_recheck() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let goals = Arc::new(GoalService::new());
            let h = Harness::new_with_goals(Some(Arc::clone(&goals))).await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let db = h.test.codex.state_db().expect("native goal database");
            let deadline = clock() + Duration::from_secs(3600);
            h.rt.with_state(|state| {
                state.observe_turn_end(
                    decide::TurnEnd {
                        at: clock(),
                        by_autopilot: true,
                        by_person: false,
                        text: None,
                        earlier: None,
                        questions: false,
                        worked: Duration::from_secs(600),
                        wall: Some(decide::Wall::Server {
                            what: "model at capacity".into(),
                            cut_off: false,
                            retry_at: Some(deadline),
                        }),
                        aborted: false,
                        ran_tools: false,
                        restored: false,
                    },
                    &h.autopilot.timing,
                );
            });
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            let original = h.rt.owed_timer().expect("actual future provider wait");
            assert!(h.rt.timer_pending() && h.shared.owed_act().is_some());
            let policy = h.autopilot.sup.policy().expect("proven native policy");
            let authority = DecisionAuthority {
                    quota: None,
                    quota_identity: None,
                policy: policy.clone(),
                offs: h.autopilot.sup.offs(),
                point: h.rt.state().point_at(),
                accepted_turns: h.rt.accepted_turns.load(Ordering::SeqCst),
                native_origin: h.rt.state().native_origin,
            };
            let absent_holds = h
                .autopilot
                .goal_owns_pace(&h.rt, &h.test.codex, &authority)
                .await;
            assert_eq!(h.rt.owed_timer().map(|owed| owed.not_before), Some(original.not_before));
            db.thread_goals()
                .replace_thread_goal(h.rt.id, "already complete", codex_state::ThreadGoalStatus::Complete, None)
                .await
                .expect("real completed goal");
            assert!(!h.autopilot.goal_owns_pace(&h.rt, &h.test.codex, &authority).await);
            assert_eq!(h.rt.owed_timer().map(|owed| owed.not_before), Some(original.not_before));
            goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id: h.rt.id,
                        objective: GoalObjectiveUpdate::Set("preserve this paused task"),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                        token_budget: GoalTokenBudgetUpdate::Keep,
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("real goal service persists a paused goal");
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            assert!(!h.rt.timer_pending() && h.rt.owed_timer().is_none());
            assert!(h.shared.owed_act().is_none(), "a paused goal retires the former wall carry");
            assert_eq!(h.rt.state().point_at(), authority.point, "the unresolved point remains");
            assert_eq!(h.rt.state().wall.as_ref().expect("unspent wall").attempts, 0);
            let paused_upgrade = h.autopilot.upgrade_facts().await;
            assert!(!paused_upgrade.waits_pending, "no stale timer holds an idle update");
            assert!(h.fake.acts().is_empty(), "goal ownership starts no fallback turn");
            let paused_holds = h
                .autopilot
                .goal_owns_pace(&h.rt, &h.test.codex, &authority)
                .await;
            saved_status(&db, h.rt.id, "unreadable-test-status").await;
            let read_error = db
                .thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .expect_err("the saved goal is really unreadable")
                .to_string();
            let unknown_holds = h
                .autopilot
                .goal_owns_pace(&h.rt, &h.test.codex, &authority)
                .await;
            let owes_recheck = h.rt.timer_pending();
            let decision = decide::decide(
                &h.rt.state(),
                &policy,
                &h.autopilot.timing,
                clock(),
                Duration::ZERO,
            );
            assert!(matches!(decision.act, Act::Type { .. }));
            let unknown_started = h
                .autopilot
                .submit(
                    &h.rt,
                    &h.test.codex,
                    &authority,
                    &decision,
                    words::RULE_CONTINUE,
                    "Continue only the scratch test task".into(),
                )
                .await;
            // Restore the scratch row before native teardown. A read failure
            // grants no change to the person's paused goal.
            saved_status(&db, h.rt.id, "paused").await;
            let restored_holds = h
                .autopilot
                .goal_owns_pace(&h.rt, &h.test.codex, &authority)
                .await;
            goals
                .clear_thread_goal(&db, h.rt.id, async { Ok(()) })
                .await
                .expect("clear only the restored scratch goal");
            let recovered_holds = h
                .autopilot
                .goal_owns_pace(&h.rt, &h.test.codex, &authority)
                .await;
            h.test
                .codex
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "A new private native turn".into(),
                    text_elements: Vec::new(),
                }]))
                .await
                .expect("new native input accepted");
            // Core sends the model request after its turn-start hooks settle,
            // including cancellation of the previous native point's timer.
            while received_responses_requests(&h._server).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            h.autopilot.schedule(&h.rt, clock() + Duration::from_secs(3600));
            assert!(h.rt.timer_pending());
            let new_generation = lock(&h.rt.timer).as_ref().expect("new wait").generation;
            let stale_retired = h.autopilot.retire_decided_wait(&h.rt, &authority).await;
            assert!(!stale_retired && h.rt.timer_pending(), "an old goal decision cannot retire the new native turn's wait");
            assert!(h.autopilot.goal_read_owns_pace(&h.rt, Err(read_error.clone()), &authority).await);
            assert_eq!(lock(&h.rt.timer).as_ref().expect("new wait remains").generation, new_generation, "an old unread goal cannot replace a new native wait with its recheck");
            Autopilot::cancel_timer(&h.rt).await;
            let shutdown = h.test.codex.shutdown_and_wait().await;
            shutdown.expect("native goal read fixture shutdown");
            assert!(!absent_holds, "a healthy absent goal does not own the pace");
            assert!(paused_holds && restored_holds, "a healthy paused goal owns its pace");
            assert!(!recovered_holds, "a recovered empty goal store releases the guard");
            assert!(
                unknown_holds && owes_recheck && !unknown_started,
                "unreadable goal must hold and retry: query={read_error}; holds={unknown_holds}; recheck={owes_recheck}; native_started={unknown_started}"
            );
        })
        .await
        .expect("goal read native admission hang detector");
    });
}

#[test]
fn a_healthy_absent_goal_allows_native_submission() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let policy = h.autopilot.sup.policy().expect("proven native policy");
            let authority = DecisionAuthority {
                quota: None,
                quota_identity: None,
                policy: policy.clone(),
                offs: h.autopilot.sup.offs(),
                point: h.rt.state().point_at(),
                accepted_turns: h.rt.accepted_turns.load(Ordering::SeqCst),
                native_origin: h.rt.state().native_origin,
            };
            let decision = decide::decide(
                &h.rt.state(),
                &policy,
                &h.autopilot.timing,
                clock(),
                Duration::ZERO,
            );
            let started = h
                .autopilot
                .submit(
                    &h.rt,
                    &h.test.codex,
                    &authority,
                    &decision,
                    words::RULE_CONTINUE,
                    "Continue only the scratch positive control".into(),
                )
                .await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native positive control shutdown");
            assert!(started, "a healthy absent goal allows the native turn");
        })
        .await
        .expect("healthy goal read native admission hang detector");
    });
}
