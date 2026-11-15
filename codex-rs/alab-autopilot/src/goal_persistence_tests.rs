// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A real failed SQLite update and healthy Active reads cannot produce more model requests.
use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use core_test_support::responses::received_responses_requests;
use sqlx::Connection;

#[derive(Default)]
struct RuntimeStatusEvents {
    updates: std::sync::Mutex<Vec<codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification>>,
    changed: tokio::sync::Notify,
}

impl codex_extension_api::ExtensionEventSink for RuntimeStatusEvents {
    fn emit(&self, _: codex_protocol::protocol::Event) {}
    fn emit_warning(&self, _: codex_extension_api::ExtensionWarning) {}
    fn alab_goal_runtime_updated(&self, update: codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification) {
        self.updates.lock().unwrap().push(update);
        self.changed.notify_one();
    }
}

impl RuntimeStatusEvents {
    async fn wait_for(&self, phase: codex_protocol::protocol::AlabGoalPersistencePhase, pending: bool, after: u64) -> codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification {
        loop {
            let changed = self.changed.notified();
            if let Some(update) = self.updates.lock().unwrap().iter().rev().find(|update| update.persistence == Some(phase) && update.writes_pending == pending && update.revision > after).cloned() { return update; }
            changed.await;
        }
    }
}

#[test]
fn failed_goal_persistence_holds_native_admission_and_preserves_the_saved_goal() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let goals = Arc::new(GoalService::new());
            let h = Harness::new_with_goals(Some(goals.clone())).await;
            let db = h.test.codex.state_db().unwrap();
            goals.set_thread_goal(&db, GoalSetRequest {
                thread_id: h.rt.id,
                objective: GoalObjectiveUpdate::Set("retain this task across a failed write"),
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Keep,
                max_goal_token_budget: None,
            }, async { Ok(()) }).await.unwrap();
            let runtime = h.test.codex.thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>().unwrap();
            runtime.restore_after_resume().await.unwrap();
            let saved = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
            let baseline = codex_alab_core::goal_clock::Instant::now()
                .checked_sub(Duration::from_secs(60)).unwrap();
            runtime.alab_restore_upgrade_goal_clock(h.rt.id, &saved,
                codex_alab_core::goal_clock::checkpoint(&baseline).unwrap()).await.unwrap();
            let options = sqlx::sqlite::SqliteConnectOptions::new().filename(db.sqlite().goals_db_path());
            #[expect(clippy::disallowed_methods, reason = "owned scratch database injects a failed native goal write without filling any volume")]
            let mut writer = sqlx::SqliteConnection::connect_with(&options).await.unwrap();
            sqlx::query("CREATE TRIGGER injected_goal_failure BEFORE UPDATE ON thread_goals BEGIN SELECT RAISE(ABORT, 'injected failed goal write'); END")
                .execute(&mut writer).await.unwrap();
            let failure = runtime.alab_upgrade_goal_state_checkpoint(h.rt.id).await;
            assert!(failure.is_err(), "real native accounting must encounter the injected write failure");
            assert!(runtime.alab_goal_persistence_held());
            for _ in 0..5 {
                let still = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
                assert_eq!(still, saved, "failed accounting must not change objective, status, or usage");
                assert!(runtime.alab_automatic_input_permit().await.is_err());
                h.autopilot.clone().evaluate(h.rt.clone()).await;
            }
            assert!(received_responses_requests(&h._server).await.is_empty());
            sqlx::query("DROP TRIGGER injected_goal_failure").execute(&mut writer).await.unwrap();
            // This injected generic failure has no typed noncommit proof at the
            // goal boundary. A later healthy read cannot authorize additive replay.
            assert!(runtime.alab_upgrade_goal_state_checkpoint(h.rt.id).await.is_err());
            assert!(runtime.alab_goal_persistence_held());
            assert!(goals.set_thread_goal(&db, GoalSetRequest {
                thread_id: h.rt.id, objective: GoalObjectiveUpdate::Set("must not erase retained accounting"),
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
            }, async { Ok(()) }).await.is_err());
            assert_eq!(db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap(), saved);
            writer.close().await.unwrap();
            h.test.codex.shutdown_and_wait().await.unwrap();
        }).await.expect("goal persistence native admission hang detector");
    });
}

#[test]
fn a_queued_old_stop_cannot_overwrite_a_successful_same_goal_reactivation() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let goals = Arc::new(GoalService::new());
            let h = Harness::new_with_goals(Some(goals.clone())).await;
            h.hold_model_response().await;
            let db = h.test.codex.state_db().unwrap();
            goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id: h.rt.id,
                        objective: GoalObjectiveUpdate::Set("the person's resumed task"),
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
            let submission = h
                .test
                .codex
                .start_or_steer_turn(codex_protocol::turn_input::TurnInputRequest::user_input(
                    vec![codex_protocol::user_input::UserInput::Text {
                        text: "continue this task under the explicit correction".to_owned(),
                        text_elements: Vec::new(),
                    }],
                ))
                .await
                .unwrap();
            let codex_protocol::turn_input::TurnInputSubmission::Started { turn_id, .. } =
                submission
            else {
                panic!("the actual native person turn must start");
            };
            while received_responses_requests(&h._server).await.is_empty() {
                tokio::task::yield_now().await;
            }
            let before = db
                .thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .unwrap()
                .unwrap();
            let (entered, entry) = tokio::sync::oneshot::channel();
            let (release, released) = tokio::sync::oneshot::channel();
            let mutation = tokio::spawn({
                let service = goals.clone();
                let db = db.clone();
                let id = h.rt.id;
                async move {
                    service
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id: id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async {
                                entered.send(()).unwrap();
                                released.await.unwrap();
                                Ok(())
                            },
                        )
                        .await
                        .unwrap()
                }
            });
            entry.await.unwrap(); // Native mutation owns its actual permit.
            let mut old_stop = Box::pin(runtime.usage_limit_active_goal_for_turn(&turn_id));
            let polled = std::future::poll_fn(|cx| {
                std::task::Poll::Ready(std::future::Future::poll(old_stop.as_mut(), cx))
            })
            .await;
            assert!(
                polled.is_pending(),
                "the old stop must queue behind the real mutation permit"
            );
            release.send(()).unwrap();
            let resumed = mutation.await.unwrap();
            old_stop.await.unwrap();
            let after = db
                .thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                before.goal_id, after.goal_id,
                "same goal ID cannot stand in for mutation generation"
            );
            assert_eq!(after.status, codex_state::ThreadGoalStatus::Active);
            assert_eq!(after.objective, before.objective);
            assert!(!runtime.alab_goal_persistence_held());
            // Check the durable fence before deferred effects, which are not
            // the native mutation's linearization point.
            resumed.apply_runtime_effects(&goals).await;
            h.test.codex.shutdown_and_wait().await.unwrap();
        })
        .await
        .expect("old stop versus explicit reactivation hang detector");
    });
}

#[test]
fn goal_runtime_status_failed_external_stop_holds_until_a_fresh_persisted_action() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let goals = Arc::new(GoalService::new());
            let events = Arc::new(RuntimeStatusEvents::default());
            let h = Harness::new_with_goal_status_sink(goals.clone(), events.clone()).await;
            let db = h.test.codex.state_db().unwrap();
            goals.set_thread_goal(&db, GoalSetRequest {
                thread_id: h.rt.id,
                objective: GoalObjectiveUpdate::Set("preserve a failed external stop"),
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
            }, async { Ok(()) }).await.unwrap();
            // Leave native accounting inactive: the error must come from the
            // actual requested status write, with no earlier usage write.
            let runtime = h.test.codex.thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>().unwrap();
            let saved = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
            let options = sqlx::sqlite::SqliteConnectOptions::new().filename(db.sqlite().goals_db_path());
            #[expect(clippy::disallowed_methods, reason = "owned scratch trigger injects only a failed status write")]
            let mut writer = sqlx::SqliteConnection::connect_with(&options).await.unwrap();
            sqlx::query("CREATE TRIGGER injected_stop_failure BEFORE UPDATE ON thread_goals BEGIN SELECT RAISE(ABORT, 'injected status write failure'); END")
                .execute(&mut writer).await.unwrap();
            assert!(goals.set_thread_goal(&db, GoalSetRequest {
                thread_id: h.rt.id, objective: GoalObjectiveUpdate::Keep,
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
            }, async { Ok(()) }).await.is_err());
            assert_eq!(db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap(), saved);
            assert!(runtime.alab_goal_persistence_held());
            let held = events.wait_for(codex_protocol::protocol::AlabGoalPersistencePhase::Held, false, 0).await;
            assert_eq!(held.thread_id, h.rt.id.to_string());
            // Exercise the exact read-free reattach observation used by the host.
            // A failed goal snapshot must not hide a settled native hold or fake None.
            sqlx::query("ALTER TABLE thread_goals RENAME TO injected_hidden_goals").execute(&mut writer).await.unwrap();
            let before = events.updates.lock().unwrap().len();
            goals.alab_emit_runtime_status(h.rt.id);
            assert!(goals.get_thread_goal(&db, h.rt.id).await.is_err());
            loop {
                let changed = events.changed.notified();
                if events.updates.lock().unwrap().iter().skip(before).any(|update| update.persistence == held.persistence && update.revision == held.revision && !update.writes_pending) { break; }
                changed.await;
            }
            sqlx::query("ALTER TABLE injected_hidden_goals RENAME TO thread_goals").execute(&mut writer).await.unwrap();
            assert_eq!(db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap(), saved);

            for _ in 0..5 {
                assert!(runtime.alab_automatic_input_permit().await.is_err());
            }
            assert!(goals.set_thread_goal(&db, GoalSetRequest {
                thread_id: h.rt.id, objective: GoalObjectiveUpdate::Keep,
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
            }, async { Err(codex_goal_extension::GoalServiceError::InvalidRequest("superseded automatic decision".to_owned())) }).await.is_err());
            assert!(runtime.alab_goal_persistence_held(), "a healthy validation refusal cannot clear the previous failed stop");
            sqlx::query("DROP TRIGGER injected_stop_failure").execute(&mut writer).await.unwrap();
            goals.set_thread_goal(&db, GoalSetRequest {
                thread_id: h.rt.id, objective: GoalObjectiveUpdate::Keep,
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
            }, async { Ok(()) }).await.unwrap();
            assert!(!runtime.alab_goal_persistence_held());
            let ready = events.wait_for(codex_protocol::protocol::AlabGoalPersistencePhase::Ready, false, held.revision).await;
            assert_eq!(ready.incarnation, held.incarnation);
            assert!(ready.revision > held.revision);
            assert!(runtime.alab_automatic_input_permit().await.is_ok());
            assert!(received_responses_requests(&h._server).await.is_empty());
            writer.close().await.unwrap();
            let weak = Arc::downgrade(&runtime);
            h.test.codex.shutdown_and_wait().await.unwrap();
            drop(runtime);
            drop(h);
            while weak.upgrade().is_some() { tokio::task::yield_now().await; }
        }).await.expect("failed native status write recovery hang detector");
    });
}

#[test]
fn a_healthy_prewrite_validation_refusal_does_not_hold_goal_admission() {
    native_test(|| async {
        let goals = Arc::new(GoalService::new());
        let h = Harness::new_with_goals(Some(goals.clone())).await;
        let db = h.test.codex.state_db().unwrap();
        goals
            .set_thread_goal(
                &db,
                GoalSetRequest {
                    thread_id: h.rt.id,
                    objective: GoalObjectiveUpdate::Set("retain healthy admission"),
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
        let before = db
            .thread_goals()
            .get_thread_goal(h.rt.id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id: h.rt.id,
                        objective: GoalObjectiveUpdate::Keep,
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Keep,
                        max_goal_token_budget: None,
                    },
                    async {
                        Err(codex_goal_extension::GoalServiceError::InvalidRequest(
                            "newer person input defeated this automatic decision".to_owned(),
                        ))
                    }
                )
                .await
                .is_err()
        );
        assert!(!runtime.alab_goal_persistence_held());
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .unwrap()
                .unwrap(),
            before
        );
        assert!(runtime.alab_automatic_input_permit().await.is_ok());
        h.test.codex.shutdown_and_wait().await.unwrap();
    });
}

#[test]
fn a_standalone_auth_retry_cannot_bypass_a_failed_goal_mutation_hold() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let goals = Arc::new(GoalService::new());
            let h = Harness::new_with_goals(Some(goals.clone())).await;
            let db = h.test.codex.state_db().unwrap();
            goals.set_thread_goal(&db, GoalSetRequest {
                thread_id: h.rt.id, objective: GoalObjectiveUpdate::Set("clear this goal safely"),
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
            }, async { Ok(()) }).await.unwrap();
            let runtime = h.test.codex.thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>().unwrap();
            let options = sqlx::sqlite::SqliteConnectOptions::new().filename(db.sqlite().goals_db_path());
            #[expect(clippy::disallowed_methods, reason = "owned scratch trigger injects a failed native clear")]
            let mut writer = sqlx::SqliteConnection::connect_with(&options).await.unwrap();
            sqlx::query("CREATE TRIGGER injected_clear_failure BEFORE DELETE ON thread_goals BEGIN SELECT RAISE(ABORT, 'injected clear failure'); END")
                .execute(&mut writer).await.unwrap();
            assert!(goals.clear_thread_goal(&db, h.rt.id, async { Ok(()) }).await.is_err());
            assert!(runtime.alab_goal_persistence_held());
            sqlx::query("DROP TRIGGER injected_clear_failure").execute(&mut writer).await.unwrap();
            // Model a native clear reaching disk without an observer acknowledging
            // it: durable absence does not acknowledge the live mutation hold.
            db.thread_goals().delete_thread_goal(h.rt.id).await.unwrap();
            db.thread_goals().alab_record_auth_standalone(h.rt.id, "standalone-wall").await.unwrap();
            assert_eq!(db.thread_goals().alab_auth_goal_context(h.rt.id, "standalone-wall").await.unwrap(), Some(None));
            assert!(runtime.alab_auth_recovery_permit_transient("standalone-wall", None, "reserved-retry").await.is_err());
            assert!(runtime.alab_goal_persistence_held());
            assert!(received_responses_requests(&h._server).await.is_empty());
            writer.close().await.unwrap();
            h.test.codex.shutdown_and_wait().await.unwrap();
        }).await.expect("standalone auth persistence barrier hang detector");
    });
}

#[test]
fn goal_runtime_status_healthy_owned_write_rechecks_after_observer_cancellation() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let goals = Arc::new(GoalService::new());
            let events = Arc::new(RuntimeStatusEvents::default());
            let h = Harness::new_with_goal_status_sink(goals.clone(), events.clone()).await;
            h.hold_model_response().await;
            let db = h.test.codex.state_db().unwrap();
            goals.set_thread_goal(&db, GoalSetRequest {
                thread_id: h.rt.id, objective: GoalObjectiveUpdate::Set("resume after the healthy write settles"),
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
            }, async { Ok(()) }).await.unwrap();
            let runtime = h.test.codex.thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>().unwrap();
            runtime.restore_after_resume().await.unwrap();
            let saved = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
            let baseline = codex_alab_core::goal_clock::Instant::now()
                .checked_sub(Duration::from_secs(60)).unwrap();
            runtime.alab_restore_upgrade_goal_clock(h.rt.id, &saved,
                codex_alab_core::goal_clock::checkpoint(&baseline).unwrap()).await.unwrap();
            let options = sqlx::sqlite::SqliteConnectOptions::new().filename(db.sqlite().goals_db_path());
            #[expect(clippy::disallowed_methods, reason = "owned scratch writer blocks actual native idle accounting across observer cancellation")]
            let mut writer = sqlx::SqliteConnection::connect_with(&options).await.unwrap();
            sqlx::query("BEGIN IMMEDIATE").execute(&mut writer).await.unwrap();
            // This public preparation operation dispatches the real native idle
            // accounting owner. No goal mutation is performed by this fixture.
            let observer = tokio::spawn({
                let runtime = runtime.clone();
                async move { runtime.prepare_external_goal_mutation().await }
            });
            while !runtime.alab_accounting_write_pending() { tokio::task::yield_now().await; }
            let writing = events.wait_for(codex_protocol::protocol::AlabGoalPersistencePhase::Writing, true, 0).await;
            assert!(runtime.alab_goal_persistence_held());
            assert!(runtime.alab_automatic_input_permit().await.is_err());
            assert!(runtime.alab_goal_admission_recovery_pending());
            observer.abort();
            assert!(observer.await.unwrap_err().is_cancelled());
            sqlx::query("ROLLBACK").execute(&mut writer).await.unwrap();
            let ready = events.wait_for(codex_protocol::protocol::AlabGoalPersistencePhase::Ready, false, writing.revision).await;
            assert_eq!(ready.incarnation, writing.incarnation);
            while received_responses_requests(&h._server).await.is_empty()
                || runtime.alab_goal_admission_recovery_pending()
            { tokio::time::sleep(Duration::from_millis(10)).await; }
            let accounted = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
            assert_eq!(accounted.tokens_used, saved.tokens_used);
            assert!(accounted.time_used_seconds >= saved.time_used_seconds + 60);
            assert_eq!(received_responses_requests(&h._server).await.len(), 1);
            assert!(!runtime.alab_goal_persistence_held());
            writer.close().await.unwrap();
            h.test.codex.shutdown_and_wait().await.unwrap();
        }).await.expect("healthy owned write admission recovery hang detector");
    });
}
