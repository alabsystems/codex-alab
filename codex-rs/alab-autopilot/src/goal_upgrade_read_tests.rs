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

#[test]
fn goal_preparation_cancelled_waiter_finishes_accounting_once_and_reopens_admission() {
    native_test(|| async {
        let goals = Arc::new(GoalService::new());
        let h = Harness::new_with_goals(Some(goals.clone())).await;
        let db = h.test.codex.state_db().unwrap();
        goals
            .set_thread_goal(
                &db,
                GoalSetRequest {
                    thread_id: h.rt.id,
                    objective: GoalObjectiveUpdate::Set("settle cancelled checkpoint accounting"),
                    status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                    token_budget: GoalTokenBudgetUpdate::Keep,
                    max_goal_token_budget: None,
                },
                async { Ok(()) },
            )
            .await
            .unwrap();
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .unwrap();
        runtime.restore_after_resume().await.unwrap();
        let saved = db
            .thread_goals()
            .get_thread_goal(h.rt.id)
            .await
            .unwrap()
            .unwrap();
        let began = std::time::Instant::now();
        let baseline = codex_alab_core::goal_clock::Instant::now()
            .checked_sub(Duration::from_secs(60))
            .unwrap();
        runtime
            .alab_restore_upgrade_goal_clock(
                h.rt.id,
                &saved,
                codex_alab_core::goal_clock::checkpoint(&baseline).unwrap(),
            )
            .await
            .unwrap();
        let options =
            sqlx::sqlite::SqliteConnectOptions::new().filename(db.sqlite().goals_db_path());
        #[expect(
            clippy::disallowed_methods,
            reason = "real scratch SQLite writer blocks native accounting across checkpoint cancellation"
        )]
        let mut lock = sqlx::SqliteConnection::connect_with(&options)
            .await
            .unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut lock)
            .await
            .unwrap();
        let waiter = tokio::spawn({
            let runtime = runtime.clone();
            let id = h.rt.id;
            async move { runtime.alab_upgrade_goal_state_checkpoint(id).await }
        });
        // The only native writer in this fixture is the checkpoint. Wait for
        // its actual goal permit, rather than guessing when a SQL write began.
        let (input,) = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let mut input = Box::pin(runtime.alab_automatic_input_permit());
                let polled = std::future::poll_fn(|cx| {
                    std::task::Poll::Ready(std::future::Future::poll(input.as_mut(), cx))
                })
                .await;
                match polled {
                    std::task::Poll::Pending => break (input,),
                    std::task::Poll::Ready(result) => drop(result.unwrap()),
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        waiter.abort();
        assert!(matches!(waiter.await, Err(error) if error.is_cancelled()));
        sqlx::query("ROLLBACK").execute(&mut lock).await.unwrap();
        lock.close().await.unwrap();
        let input = tokio::time::timeout(Duration::from_secs(10), input)
            .await
            .unwrap()
            .unwrap();
        let accounted = db
            .thread_goals()
            .get_thread_goal(h.rt.id)
            .await
            .unwrap()
            .unwrap();
        assert!(accounted.time_used_seconds >= saved.time_used_seconds + 60);
        assert_eq!(accounted.tokens_used, saved.tokens_used);
        drop(input);
        let (again, transfer) = runtime
            .alab_upgrade_goal_state_checkpoint(h.rt.id)
            .await
            .unwrap();
        let again = again.unwrap();
        // A cancelled write must update its native baseline, not charge the
        // carried minute again. Allow the measured real time spent in this test.
        let upper = saved.time_used_seconds + 60 + began.elapsed().as_secs() as i64 + 2;
        assert!(again.time_used_seconds <= upper, "{again:?}");
        drop(transfer);
        h.test.codex.shutdown_and_wait().await.unwrap();
    });
}

#[test]
fn goal_preparation_revocation_does_not_wait_for_the_goal_permit() {
    native_test(|| async {
        let h = Harness::new_with_goals(Some(Arc::new(GoalService::new()))).await;
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .unwrap();
        let invocation = codex_core::config::ExecConfiguration::capture(
            &h.test.config,
            &codex_core::config::ConfigOverrides::default(),
        )
        .unwrap();
        let forest = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match h.test.codex.prepare_upgrade_forest(&invocation).await {
                    Ok(forest) => break forest,
                    Err(error) if error == "native admission is pending" => {
                        tokio::task::yield_now().await;
                    }
                    Err(error) => panic!("forest preparation: {error}"),
                }
            }
        })
        .await
        .unwrap();
        let held = runtime.alab_automatic_input_permit().await.unwrap();
        // Start from a completed real Core preflight: this proves cancellation
        // of the later goal phase, not cancellation of Core's own preparation.
        let mut preparing = Box::pin(crate::upgrade_source::Prepared::from_forest(
            h.rt.id, forest,
        ));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(std::future::Future::poll(
                preparing.as_mut(),
                cx
            )))
            .await
            .is_pending()
        );
        h.test
            .codex
            .submit(codex_protocol::protocol::Op::Interrupt)
            .await
            .unwrap();
        let error = tokio::time::timeout(Duration::from_secs(5), preparing)
            .await
            .unwrap()
            .err()
            .expect("queued work supersedes goal preparation while its permit stays held");
        assert!(error.contains("superseded"));
        let mut terminated = Box::pin(h.test.codex.wait_until_terminated());
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(std::future::Future::poll(
                terminated.as_mut(),
                cx
            )))
            .await
            .is_pending()
        );
        drop(held);
        // Reconcile the native checkpoint and ordinary interrupt before ending
        // the fixture. No cancelled waiter can leave a prepared goal behind.
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match runtime.alab_upgrade_goal_state_checkpoint(h.rt.id).await {
                    Ok((_, transfer)) => {
                        drop(transfer);
                        break;
                    }
                    Err(error) if error == "native admission is pending" => {
                        tokio::task::yield_now().await;
                    }
                    Err(error) => panic!("goal checkpoint: {error}"),
                }
            }
        })
        .await
        .unwrap();
        h.test.codex.shutdown_and_wait().await.unwrap();
    });
}

#[test]
fn native_clock_adoption_validates_persisted_goal_and_accounts_the_carried_interval_once() {
    native_test(|| async {
        let goals = Arc::new(GoalService::new());
        let h = Harness::new_with_goals(Some(goals.clone())).await;
        let db = h.test.codex.state_db().unwrap();
        goals
            .set_thread_goal(
                &db,
                GoalSetRequest {
                    thread_id: h.rt.id,
                    objective: GoalObjectiveUpdate::Set("preserve native clock data"),
                    status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                    token_budget: GoalTokenBudgetUpdate::Keep,
                    max_goal_token_budget: None,
                },
                async { Ok(()) },
            )
            .await
            .unwrap();
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .unwrap();
        runtime.restore_after_resume().await.unwrap();
        let saved = db
            .thread_goals()
            .get_thread_goal(h.rt.id)
            .await
            .unwrap()
            .unwrap();
        let baseline = codex_alab_core::goal_clock::Instant::now()
            .checked_sub(Duration::new(2, 999_999_999))
            .unwrap();
        let clock = codex_alab_core::goal_clock::checkpoint(&baseline).unwrap();
        let mut changed = saved.clone();
        changed.tokens_used += 1;
        assert!(
            runtime
                .alab_restore_upgrade_goal_clock(h.rt.id, &changed, clock)
                .await
                .is_err()
        );
        runtime
            .alab_restore_upgrade_goal_clock(h.rt.id, &saved, clock)
            .await
            .unwrap();
        runtime
            .alab_restore_upgrade_goal_clock(h.rt.id, &saved, clock)
            .await
            .unwrap();
        let (accounted, transfer) = runtime
            .alab_upgrade_goal_state_checkpoint(h.rt.id)
            .await
            .unwrap();
        let accounted = accounted.unwrap();
        assert!(accounted.time_used_seconds >= saved.time_used_seconds + 2);
        assert_eq!(accounted.tokens_used, saved.tokens_used);
        drop(transfer);
        assert!(
            runtime
                .alab_restore_upgrade_goal_clock(h.rt.id, &saved, clock)
                .await
                .is_err()
        );
        runtime
            .alab_restore_upgrade_goal_clock(h.rt.id, &accounted, clock)
            .await
            .unwrap();
        h.test.codex.shutdown_and_wait().await.unwrap();
    });
}

#[test]
fn all_goal_capture_waits_for_source_quiescence_and_survives_a_lost_waiter() {
    native_test(|| async {
        const CHILD: &str = "ALAB_FINAL_GOAL_FOREST_CASE";
        const SUCCESS: &str = "ALAB_FINAL_GOAL_FOREST_OK";
        const TEST: &str = "extension::goal_upgrade_read_tests::all_goal_capture_waits_for_source_quiescence_and_survives_a_lost_waiter";
        if std::env::var_os(CHILD).is_none() {
            let output = tokio::time::timeout(
                Duration::from_secs(60),
                tokio::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture"])
                    .env(CHILD, "1")
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains(SUCCESS),
                "isolated case did not execute its completion checks"
            );
            return;
        }
        // This exact child alone replaces stdin with its private native PTY.
        // stdout/stderr remain the parent's captured pipes; no human TTY changes.
        use std::io::Write as _;
        use std::os::fd::AsRawFd;
        use std::os::fd::FromRawFd;
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &raw mut master,
                    &raw mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let (mut master, slave) = unsafe {
            (
                std::fs::File::from_raw_fd(master),
                std::fs::File::from_raw_fd(slave),
            )
        };
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
        assert_eq!(
            unsafe { libc::fcntl(slave.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
        assert_eq!(
            unsafe { libc::dup2(slave.as_raw_fd(), libc::STDIN_FILENO) },
            0
        );
        let goals = Arc::new(GoalService::new());
        let h = Harness::new_with_goals(Some(goals.clone())).await;
        codex_alab_tui::terminal_modes::enable_raw_mode().unwrap();
        let input = codex_alab_tui::input::InputStream::default();
        master.write_all(b"a\x1b[200~part").unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let next = {
                    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
                    input.poll_next(&mut cx)
                };
                if let std::task::Poll::Ready(Some(event)) = next {
                    event.unwrap();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let other = h
            .test
            .thread_manager
            .start_thread(codex_core::StartThreadOptions {
                environments: Some(Vec::new()),
                ..codex_core::StartThreadOptions::new(h.test.config.clone())
            })
            .await
            .unwrap();
        let mut memory = h.test.config.clone();
        memory.ephemeral = true;
        let mut options = codex_core::StartThreadOptions::new(memory);
        options.session_source = Some(codex_protocol::protocol::SessionSource::Internal(
            codex_protocol::protocol::InternalSessionSource::MemoryConsolidation,
        ));
        let child = h
            .test
            .thread_manager
            .spawn_internal_session(other.thread_id, options)
            .await
            .unwrap();
        let db = h.test.codex.state_db().unwrap();
        goals
            .set_thread_goal(
                &db,
                GoalSetRequest {
                    thread_id: h.rt.id,
                    objective: GoalObjectiveUpdate::Set(
                        "carry active work through final native cleanup",
                    ),
                    status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                    token_budget: GoalTokenBudgetUpdate::Set(Some(1000)),
                    max_goal_token_budget: None,
                },
                async { Ok(()) },
            )
            .await
            .unwrap();
        db.thread_goals()
            .account_thread_goal_usage(
                h.rt.id,
                70,
                93,
                codex_state::GoalAccountingMode::ActiveStatusOnly,
                None,
            )
            .await
            .unwrap();
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .unwrap();
        runtime.restore_after_resume().await.unwrap();
        let foreign = Arc::new(codex_alab_core::upgrade_barrier::State::default());
        let mut wrong = foreign.prepare().unwrap();
        assert!(
            runtime
                .alab_final_upgrade_goal_state(h.rt.id, &wrong)
                .await
                .is_err()
        );
        assert!(wrong.commit());
        assert!(
            runtime
                .alab_final_upgrade_goal_state(h.rt.id, &wrong)
                .await
                .is_err()
        );
        let invocation = codex_core::config::ExecConfiguration::capture(
            &h.test.config,
            &codex_core::config::ConfigOverrides::default(),
        )
        .unwrap();
        let prepare = || async {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match crate::upgrade_source::Prepared::prepare(&h.test.codex, &invocation).await
                    {
                        Ok(source) => break source,
                        Err(error) if error == "native admission is pending" => {
                            tokio::task::yield_now().await
                        }
                        Err(error) => panic!("source preparation: {error}"),
                    }
                }
            })
            .await
            .unwrap()
        };
        let source = prepare().await;
        assert_eq!(source.threads().len(), 3);
        let staged = source
            .goal_snapshots()
            .find(|(id, _)| *id == h.rt.id)
            .unwrap()
            .1
            .unwrap()
            .clone();
        assert_eq!(staged.tokens_used, 93);
        let terminal =
            codex_alab_tui::upgrade_terminal::Prepared::prepare(input.prepare_upgrade().unwrap())
                .unwrap();
        assert!(source.commit(terminal, || false).unwrap().is_none());
        assert!(!codex_alab_tui::terminal_modes::preserve_on_exit());
        let source = prepare().await;
        assert!(source.valid());
        let terminal =
            codex_alab_tui::upgrade_terminal::Prepared::prepare(input.prepare_upgrade().unwrap())
                .unwrap();
        let capture = source.commit(terminal, || true).unwrap().unwrap();
        assert!(codex_alab_tui::terminal_modes::preserve_on_exit());
        // Real native idle time elapses after preparation, before the final flush.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let options =
            sqlx::sqlite::SqliteConnectOptions::new().filename(db.sqlite().goals_db_path());
        #[expect(
            clippy::disallowed_methods,
            reason = "isolated scratch goal store write lock verifies owned final accounting across cancellation"
        )]
        let mut lock = sqlx::SqliteConnection::connect_with(&options)
            .await
            .unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut lock)
            .await
            .unwrap();
        let waiter = tokio::spawn({
            let capture = capture.clone();
            async move { capture.capture().await }
        });
        tokio::time::timeout(
            Duration::from_secs(10),
            h.test.codex.wait_until_terminated(),
        )
        .await
        .unwrap();
        assert!(!waiter.is_finished());
        waiter.abort();
        assert!(matches!(waiter.await, Err(error) if error.is_cancelled()));
        sqlx::query("ROLLBACK").execute(&mut lock).await.unwrap();
        lock.close().await.unwrap();
        let final_source = tokio::time::timeout(Duration::from_secs(10), capture.capture())
            .await
            .unwrap()
            .unwrap();
        let final_state = &final_source.goals;
        let input_data: serde_json::Value =
            serde_json::from_slice(final_source.input_snapshot()).unwrap();
        assert_eq!(
            input_data["buffer"],
            serde_json::json!(b"\x1b[200~part".to_vec())
        );
        assert!(
            serde_json::to_value(final_source.mode_snapshot())
                .unwrap()
                .is_object()
        );
        assert_eq!(final_source.primary, h.rt.id);
        assert_eq!(final_state.goals.len(), 3);
        assert!(
            final_state
                .goals
                .iter()
                .find(|state| state.thread == h.rt.id)
                .unwrap()
                .clock
                .is_some()
        );
        assert!(
            final_state
                .goals
                .iter()
                .filter(|state| state.thread != h.rt.id)
                .all(|state| state.clock.is_none())
        );
        let final_goal = final_state
            .goals
            .iter()
            .find(|state| state.thread == h.rt.id)
            .unwrap()
            .goal
            .as_ref()
            .unwrap();
        assert_eq!(final_goal.goal_id, staged.goal_id);
        assert_eq!(final_goal.tokens_used, 93);
        assert!(final_goal.time_used_seconds > staged.time_used_seconds);
        assert_eq!(final_goal.status, codex_state::ThreadGoalStatus::Active);
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .unwrap()
                .as_ref(),
            Some(final_goal)
        );
        assert!(
            final_source
                .encode_state()
                .unwrap_err()
                .contains("actual TUI startup")
        );
        crate::upgrade_state::qualify_frame(&final_source);
        let encoded = final_state.encode_state(h.rt.id).unwrap();
        let decoded = crate::upgrade_goals::GoalForestState::decode(&encoded, h.rt.id).unwrap();
        assert_eq!(decoded.goals, final_state.goals);
        assert_eq!(decoded.forest.roots(), final_state.forest.roots());
        let original: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        let primary_index = original["goals"]
            .as_array()
            .unwrap()
            .iter()
            .position(|entry| entry["thread"] == serde_json::json!(h.rt.id))
            .unwrap();
        let absent_index = original["goals"]
            .as_array()
            .unwrap()
            .iter()
            .position(|entry| entry["goal"].is_null())
            .unwrap();
        for mutation in 0..11 {
            let mut wire = original.clone();
            match mutation {
                0 => wire["version"] = serde_json::json!(2),
                1 => {
                    wire["goals"].as_array_mut().unwrap().pop();
                }
                2 => {
                    let duplicate = wire["goals"][0].clone();
                    wire["goals"].as_array_mut().unwrap().push(duplicate);
                }
                3 => {
                    wire["goals"][primary_index]["goal"]["thread_id"] =
                        serde_json::json!(other.thread_id)
                }
                4 => {
                    wire["goals"][primary_index]["clock"]["nanos"] =
                        serde_json::json!(1_000_000_000u32)
                }
                5 => {
                    wire["goals"][primary_index]["goal"]["status"] =
                        serde_json::json!("future_unknown")
                }
                6 => {
                    wire["goals"][absent_index]["clock"] =
                        wire["goals"][primary_index]["clock"].clone()
                }
                7 => wire["unrecognized"] = serde_json::json!(true),
                8 => {
                    wire["goals"][absent_index]
                        .as_object_mut()
                        .unwrap()
                        .remove("goal");
                }
                9 => {
                    wire["goals"][primary_index]
                        .as_object_mut()
                        .unwrap()
                        .remove("clock");
                }
                10 => {
                    wire["goals"][primary_index]["goal"]
                        .as_object_mut()
                        .unwrap()
                        .remove("token_budget");
                }
                _ => unreachable!(),
            }
            assert!(
                crate::upgrade_goals::GoalForestState::decode(
                    &serde_json::to_vec(&wire).unwrap(),
                    h.rt.id
                )
                .is_err(),
                "invalid goal forest accepted mutation {mutation}"
            );
        }
        assert!(crate::upgrade_goals::GoalForestState::decode(&encoded, other.thread_id).is_err());
        assert!(Arc::ptr_eq(
            &final_source,
            &capture.capture().await.unwrap()
        ));
        for thread in [&h.test.codex, &other.thread, &child.thread] {
            tokio::time::timeout(Duration::from_secs(1), thread.wait_until_terminated())
                .await
                .unwrap();
        }
        final_source.restore_after_failure().unwrap();
        assert!(!codex_alab_tui::terminal_modes::preserve_on_exit());
        println!("{SUCCESS}");
    });
}

#[test]
fn all_goal_checkpoints_protect_absence_and_hidden_stopped_state_until_one_commit() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(30), async {
            let goals = Arc::new(GoalService::new());
            let h = Harness::new_with_goals(Some(goals.clone())).await;
            let hidden = h
                .test
                .thread_manager
                .start_thread(codex_core::StartThreadOptions {
                    environments: Some(Vec::new()),
                    ..codex_core::StartThreadOptions::new(h.test.config.clone())
                })
                .await
                .unwrap();
            let db = hidden.thread.state_db().unwrap();
            goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id: hidden.thread_id,
                        objective: GoalObjectiveUpdate::Set("preserve the hidden paused goal"),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(900)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .unwrap();
            let mut memory = h.test.config.clone();
            memory.ephemeral = true;
            let mut child_options = codex_core::StartThreadOptions::new(memory);
            child_options.session_source = Some(codex_protocol::protocol::SessionSource::Internal(
                codex_protocol::protocol::InternalSessionSource::MemoryConsolidation,
            ));
            let child = h
                .test
                .thread_manager
                .spawn_internal_session(hidden.thread_id, child_options)
                .await
                .unwrap();
            let threads = vec![
                h.test.codex.clone(),
                hidden.thread.clone(),
                child.thread.clone(),
            ];
            let invocation = codex_core::config::ExecConfiguration::capture(
                &h.test.config,
                &codex_core::config::ConfigOverrides::default(),
            )
            .unwrap();
            let mut forest = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match h.test.codex.prepare_upgrade_forest(&invocation).await {
                        Ok(forest) => break forest,
                        Err(error) if error == "native admission is pending" => {
                            tokio::task::yield_now().await
                        }
                        Err(error) => panic!("native forest preparation: {error}"),
                    }
                }
            })
            .await
            .unwrap();
            let mut partial =
                crate::upgrade_goals::PreparedGoals::prepare(std::slice::from_ref(&h.test.codex))
                    .await
                    .unwrap();
            assert!(
                partial
                    .commit_forest_if(&mut forest, || panic!(
                        "partial goal inventory cannot close the forest"
                    ))
                    .is_err()
            );
            assert!(partial.valid());
            assert!(forest.valid());
            drop(partial);
            drop(forest);
            let mut prepared = crate::upgrade_goals::PreparedGoals::prepare(&threads)
                .await
                .unwrap();
            let snapshots = prepared.snapshots().collect::<Vec<_>>();
            assert_eq!(snapshots[0], (h.rt.id, None));
            let saved = snapshots[1].1.unwrap().clone();
            assert_eq!(saved.thread_id, hidden.thread_id);
            assert_eq!(saved.status, codex_state::ThreadGoalStatus::Paused);
            assert_eq!(saved.token_budget, Some(900));
            assert_eq!(snapshots[2], (child.thread_id, None));
            goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id: hidden.thread_id,
                        objective: GoalObjectiveUpdate::Set(
                            "updated paused work during preparation",
                        ),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                        token_budget: GoalTokenBudgetUpdate::Keep,
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .unwrap();
            assert!(!prepared.valid());
            assert!(
                !prepared
                    .commit_if(|| panic!("hidden goal mutation must revoke the whole decision"))
            );
            drop(prepared);
            let primary_runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .unwrap();
            assert!(
                primary_runtime
                    .alab_upgrade_goal_state_checkpoint(hidden.thread_id)
                    .await
                    .is_err()
            );
            assert!(
                crate::upgrade_goals::PreparedGoals::prepare(&[])
                    .await
                    .is_err()
            );
            assert!(
                crate::upgrade_goals::PreparedGoals::prepare(&[
                    h.test.codex.clone(),
                    h.test.codex.clone()
                ])
                .await
                .is_err()
            );
            let mut prepared = crate::upgrade_goals::PreparedGoals::prepare(&threads)
                .await
                .unwrap();
            assert_eq!(
                prepared.snapshots().nth(1).unwrap().1.unwrap().status,
                codex_state::ThreadGoalStatus::Paused
            );
            assert!(!prepared.commit_if(|| false));
            assert!(
                prepared.valid(),
                "host rejection leaves all guards revocable"
            );
            assert!(prepared.commit_if(|| true));
            for thread in &threads {
                let runtime = thread
                    .thread_extension_data()
                    .get::<codex_goal_extension::GoalRuntimeHandle>()
                    .unwrap();
                assert!(runtime.alab_automatic_input_permit().await.is_err());
            }
            assert_eq!(
                db.thread_goals()
                    .get_thread_goal(hidden.thread_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .objective,
                "updated paused work during preparation"
            );
            h.test.codex.shutdown_and_wait().await.unwrap();
            hidden.thread.shutdown_and_wait().await.unwrap();
            child.thread.shutdown_and_wait().await.unwrap();
        })
        .await
        .expect("all-goal checkpoint deadline");
    });
}

#[test]
fn an_absent_goal_checkpoint_is_revoked_before_a_new_goal_is_persisted() {
    native_test(|| async {
        let goals = Arc::new(GoalService::new());
        let h = Harness::new_with_goals(Some(goals.clone())).await;
        let db = h.test.codex.state_db().unwrap();
        let mut prepared =
            crate::upgrade_goals::PreparedGoals::prepare(std::slice::from_ref(&h.test.codex))
                .await
                .unwrap();
        assert!(prepared.snapshots().next().unwrap().1.is_none());
        goals
            .set_thread_goal(
                &db,
                GoalSetRequest {
                    thread_id: h.rt.id,
                    objective: GoalObjectiveUpdate::Set(
                        "new work has priority over the old empty snapshot",
                    ),
                    status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                    token_budget: GoalTokenBudgetUpdate::Keep,
                    max_goal_token_budget: None,
                },
                async {
                    assert!(!prepared.valid());
                    Ok(())
                },
            )
            .await
            .unwrap();
        assert!(!prepared.commit_if(|| panic!("cannot commit an obsolete absent goal")));
        drop(prepared);
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            codex_state::ThreadGoalStatus::Active
        );
        h.test.codex.shutdown_and_wait().await.unwrap();
    });
}

#[test]
fn native_goal_checkpoint_revokes_before_mutation_and_commits_without_losing_accounting() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let goals = Arc::new(GoalService::new());
            let h = Harness::new_with_goals(Some(Arc::clone(&goals))).await;
            let db = h.test.codex.state_db().expect("primary goal database");
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("actual native goal runtime");
            assert!(runtime.alab_upgrade_goal_checkpoint().await.is_err());
            goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id: h.rt.id,
                        objective: GoalObjectiveUpdate::Set("carry the exact active goal"),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(5000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("persist active goal");
            db.thread_goals()
                .account_thread_goal_usage(
                    h.rt.id,
                    37,
                    123,
                    codex_state::GoalAccountingMode::ActiveStatusOnly,
                    None,
                )
                .await
                .expect("native persisted accounting");
            runtime
                .restore_after_resume()
                .await
                .expect("native resume starts idle goal accounting");
            let before = db
                .thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .expect("checkpoint input")
                .expect("saved goal");
            // Native accounting uses the real monotonic clock. Wait for one whole
            // idle second; there is no upper latency bound on this assertion.
            tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_secs(1)))
                .await
                .expect("native monotonic idle interval");
            let (snapshot, mut transfer) = runtime
                .alab_upgrade_goal_checkpoint()
                .await
                .expect("idle native checkpoint");
            assert!(snapshot.time_used_seconds > before.time_used_seconds);
            let mut accounted = before;
            accounted.time_used_seconds = snapshot.time_used_seconds;
            accounted.updated_at = snapshot.updated_at;
            assert_eq!(snapshot, accounted);
            assert_eq!(snapshot.tokens_used, 123);
            let before = snapshot;
            assert!(transfer.valid());
            let admission = runtime
                .alab_automatic_input_permit()
                .await
                .expect("new work retains priority during preparation");
            assert!(!transfer.valid());
            drop(admission);
            assert!(!runtime.alab_commit_goal_upgrade(&mut transfer, || panic!(
                "revoked checkpoint cannot commit the UI"
            )));
            drop(transfer);
            tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_secs(1)))
                .await
                .expect("native monotonic idle interval");

            db.thread_goals()
                .replace_thread_goal_snapshot(&before)
                .await
                .expect("persist native continuation deferral");
            assert!(runtime.alab_upgrade_goal_checkpoint().await.is_err());
            assert_eq!(
                db.thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("deferred snapshot")
                    .expect("goal remains"),
                before
            );
            db.thread_goals()
                .clear_thread_goal_continuation_deferral(h.rt.id)
                .await
                .expect("clear only the test deferral");
            let (resumed, mut transfer) = runtime
                .alab_upgrade_goal_checkpoint()
                .await
                .expect("cleared deferral permits preparation");
            assert!(resumed.time_used_seconds > before.time_used_seconds);
            assert_eq!(resumed.tokens_used, before.tokens_used);
            let before = resumed;
            goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id: h.rt.id,
                        objective: GoalObjectiveUpdate::Keep,
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                        token_budget: GoalTokenBudgetUpdate::Keep,
                        max_goal_token_budget: None,
                    },
                    async {
                        assert!(!transfer.valid(), "revoke before recording user intent");
                        Ok(())
                    },
                )
                .await
                .expect("a real native user pause is never stranded by preparation");
            assert!(!runtime.alab_commit_goal_upgrade(&mut transfer, || panic!(
                "a user mutation revoked the UI decision"
            )));
            drop(transfer);
            let after = db
                .thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .expect("read user pause")
                .expect("same saved goal");
            assert_eq!(after.status, codex_state::ThreadGoalStatus::Paused);
            assert_eq!(after.goal_id, before.goal_id);
            assert_eq!(after.objective, before.objective);
            assert_eq!(after.token_budget, before.token_budget);
            assert_eq!(after.tokens_used, before.tokens_used);
            assert!(after.time_used_seconds >= before.time_used_seconds);
            assert!(runtime.alab_upgrade_goal_checkpoint().await.is_err());
            saved_status(&db, h.rt.id, "unreadable-upgrade-checkpoint").await;
            assert!(runtime.alab_upgrade_goal_checkpoint().await.is_err());
            saved_status(&db, h.rt.id, "active").await;
            let (_, mut transfer) = runtime
                .alab_upgrade_goal_checkpoint()
                .await
                .expect("restored native row");
            let fake = Arc::new(codex_alab_core::upgrade_barrier::State::default());
            let mut foreign = fake.prepare().expect("unrelated runtime token");
            assert!(!runtime.alab_commit_goal_upgrade(&mut foreign, || panic!(
                "foreign token has no authority"
            )));
            assert!(!runtime.alab_commit_goal_upgrade(&mut transfer, || false));
            assert!(transfer.valid(), "a refused UI can still roll back");
            let committed_goal = db
                .thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .expect("read before commitment")
                .expect("saved active goal");
            assert!(runtime.alab_commit_goal_upgrade(&mut transfer, || true));
            drop(transfer);
            assert!(runtime.alab_automatic_input_permit().await.is_err());
            assert!(runtime.alab_upgrade_goal_checkpoint().await.is_err());
            assert!(
                goals
                    .set_thread_goal(
                        &db,
                        GoalSetRequest {
                            thread_id: h.rt.id,
                            objective: GoalObjectiveUpdate::Keep,
                            status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                            token_budget: GoalTokenBudgetUpdate::Keep,
                            max_goal_token_budget: None,
                        },
                        async { panic!("committed source cannot record a new mutation") }
                    )
                    .await
                    .is_err()
            );
            assert_eq!(
                db.thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("read after commitment")
                    .expect("goal remains"),
                committed_goal
            );
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("committed idle root drains");
        })
        .await
        .expect("checkpoint test hang detector");
    });
}

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
