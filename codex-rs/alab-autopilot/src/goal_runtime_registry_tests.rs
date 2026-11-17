// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A native goal runtime keeps one mutation owner until its matching retirement.

use super::*;
use codex_extension_api::ExtensionData;
use codex_protocol::ThreadId;

#[derive(Default)]
struct RuntimeStatusEvents {
    updates: std::sync::Mutex<Vec<codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification>>,
}

impl codex_extension_api::ExtensionEventSink for RuntimeStatusEvents {
    fn emit(&self, _: codex_protocol::protocol::Event) {}

    fn emit_warning(&self, _: codex_extension_api::ExtensionWarning) {}

    fn alab_goal_runtime_updated(
        &self,
        update: codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification,
    ) {
        self.updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(update);
    }

    fn alab_goal_runtime_inline_snapshot(
        &self,
        update: codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification,
    ) -> Option<codex_protocol::protocol::AlabGoalRuntimeSnapshot> {
        Some(codex_protocol::protocol::AlabGoalRuntimeSnapshot::new(
            update,
            |_| {},
        ))
    }
}

fn status_owner(goals: &GoalService, thread_id: ThreadId) -> usize {
    Arc::as_ptr(
        &goals
            .alab_goal_runtime(thread_id)
            .expect("registered native goal runtime"),
    ) as usize
}

fn status_incarnation(goals: &GoalService, thread_id: ThreadId) -> String {
    goals
        .alab_runtime_status_snapshot(thread_id)
        .expect("registered native goal runtime with an inline status sink")
        .update()
        .incarnation
        .clone()
}

fn production_goal_registry(
    db: Arc<codex_state::StateRuntime>,
    thread_manager: std::sync::Weak<codex_core::ThreadManager>,
    goals: Arc<GoalService>,
) -> codex_extension_api::ExtensionRegistry<Config> {
    let mut builder = codex_extension_api::ExtensionRegistryBuilder::<Config>::new();
    codex_goal_extension::install_with_backend(
        &mut builder,
        db,
        codex_analytics::AnalyticsEventsClient::disabled(),
        /*metrics_client*/ None,
        thread_manager,
        goals,
        |config: &Config| codex_goal_extension::GoalExtensionConfig {
            enabled: true,
            max_goal_token_budget: config.max_goal_token_budget,
        },
    );
    builder.build()
}

/// Settle a setup mutation without admitting the Active goal's ordinary continuation.
async fn apply_setup_goal(
    goals: &GoalService,
    db: &codex_state::StateRuntime,
    thread_id: ThreadId,
    outcome: codex_goal_extension::GoalSetOutcome,
) {
    let goal = db
        .thread_goals()
        .get_thread_goal(thread_id)
        .await
        .expect("read setup goal")
        .expect("setup goal exists");
    db.thread_goals()
        .replace_thread_goal_snapshot(&goal)
        .await
        .expect("persist setup-only continuation deferral");
    outcome.apply_runtime_effects(goals).await;
    db.thread_goals()
        .clear_thread_goal_continuation_deferral(thread_id)
        .await
        .expect("clear setup-only continuation deferral");
    assert!(
        !goals
            .alab_goal_runtime(thread_id)
            .expect("setup goal runtime")
            .alab_goal_persistence_held(),
        "the setup-only continuation deferral must settle mutation custody"
    );
}

#[test]
fn a_duplicate_goal_runtime_cannot_replace_or_remove_the_original_owner() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let original = status_owner(&goals, h.rt.id);
            let duplicate_session = ExtensionData::new(h.rt.id.to_string());
            let duplicate_thread = ExtensionData::new(h.rt.id.to_string());

            h.start_goal_lifecycle(&duplicate_session, &duplicate_thread)
                .await;
            let after_duplicate_start = status_owner(&goals, h.rt.id);
            let duplicate = duplicate_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("duplicate native runtime was constructed");
            let refusal = match duplicate.alab_model_goal_epoch_permit().await {
                Ok(_) => panic!("the refused duplicate acquired SQL mutation authority"),
                Err(error) => error,
            };
            assert!(
                refusal.contains("no longer registered"),
                "the refusal comes from exact registry ownership: {refusal}"
            );
            assert_eq!(duplicate.alab_goal_mutation_epoch(), None);
            assert!(
                duplicate.alab_goal_persistence_held(),
                "a refused runtime's accounting state is permanently closed"
            );
            h.stop_goal_lifecycle(&duplicate_session, &duplicate_thread)
                .await;
            let after_duplicate_stop = status_owner(&goals, h.rt.id);

            assert_eq!(
                after_duplicate_start, original,
                "a duplicate startup cannot replace the registered mutation owner"
            );
            assert_eq!(
                after_duplicate_stop, original,
                "the refused duplicate's teardown cannot unregister the original owner"
            );

            let primary_session = ExtensionData::new(h.rt.id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            h.start_goal_lifecycle(&duplicate_session, &duplicate_thread)
                .await;
            assert!(
                goals.alab_goal_runtime(h.rt.id).is_none(),
                "a refused duplicate remains refused after the original owner retires"
            );
            let refusal = match duplicate.alab_model_goal_epoch_permit().await {
                Ok(_) => panic!("the refused duplicate later acquired SQL mutation authority"),
                Err(error) => error,
            };
            assert!(
                refusal.contains("no longer registered"),
                "the duplicate remains refused by exact registry ownership: {refusal}"
            );
        })
        .await
        .expect("duplicate goal-runtime ownership hang detector");
    });
}

#[test]
fn vanilla_mode_preserves_exact_registry_ownership_and_retirement() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let original = status_owner(&goals, thread_id);
            codex_alab_core::set_enhanced(false);

            let duplicate_session = ExtensionData::new("vanilla-duplicate-session");
            let duplicate_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&duplicate_session, &duplicate_thread)
                .await;
            let duplicate = duplicate_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("vanilla duplicate runtime");
            assert_eq!(status_owner(&goals, thread_id), original);
            assert!(duplicate.alab_model_goal_epoch_permit().await.is_err());
            h.stop_goal_lifecycle(&duplicate_session, &duplicate_thread)
                .await;
            assert_eq!(status_owner(&goals, thread_id), original);

            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            let successor_session = ExtensionData::new("vanilla-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("runtime born in vanilla");
            assert!(successor.alab_model_goal_epoch_permit().await.is_ok());

            codex_alab_core::set_enhanced(true);
            assert!(
                duplicate.alab_model_goal_epoch_permit().await.is_err(),
                "a refused vanilla handle cannot resurrect after Enhanced returns"
            );
            assert!(
                successor.alab_model_goal_epoch_permit().await.is_ok(),
                "the exact vanilla-born owner remains authoritative after the toggle"
            );
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("vanilla registry ownership hang detector");
    });
}

#[test]
fn native_stops_with_zero_or_positive_progress_leave_a_ready_advanced_epoch() {
    native_test(|| async {
        for progress in [0_i64, 17] {
            tokio::time::timeout(HANG, async {
                let (h, goals) = harness().await;
                let evaluation = h.rt.hold_eval_when_free().await;
                let thread_id = h.rt.id;
                let db = h.test.codex.state_db().expect("native goal database");
                let outcome = goals
                    .set_thread_goal(
                        &db,
                        GoalSetRequest {
                            thread_id,
                            objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                            token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                            max_goal_token_budget: None,
                        },
                        async { Ok(()) },
                    )
                    .await
                    .expect("seed the active goal");
                apply_setup_goal(&goals, &db, thread_id, outcome).await;
                let runtime = h
                    .test
                    .codex
                    .thread_extension_data()
                    .get::<codex_goal_extension::GoalRuntimeHandle>()
                    .expect("registered native goal runtime");
                let before = runtime
                    .alab_goal_mutation_epoch()
                    .expect("pre-stop mutation epoch");
                let registry = production_goal_registry(
                    Arc::clone(&db),
                    Arc::downgrade(&h.test.thread_manager),
                    Arc::clone(&goals),
                );
                let session_store = ExtensionData::new("native-stop-progress-session");
                let turn_store = ExtensionData::new(format!("native-stop-progress-{progress}"));
                let mode = codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: "gpt-5".to_owned(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                };
                let baseline = codex_protocol::protocol::TokenUsage::default();
                for contributor in registry.turn_lifecycle_contributors() {
                    contributor
                        .on_turn_start(codex_extension_api::TurnStartInput {
                            turn_id: turn_store.level_id(),
                            collaboration_mode: &mode,
                            token_usage_at_turn_start: Some(&baseline),
                            session_store: &session_store,
                            thread_store: h.test.codex.thread_extension_data(),
                            turn_store: &turn_store,
                        })
                        .await;
                }
                if progress > 0 {
                    let usage = codex_protocol::protocol::TokenUsage {
                        input_tokens: progress,
                        total_tokens: progress,
                        ..Default::default()
                    };
                    let usage = codex_protocol::protocol::TokenUsageInfo {
                        total_token_usage: usage.clone(),
                        last_token_usage: usage,
                        last_usage_includes_reasoning: None,
                        model_context_window: None,
                    };
                    for contributor in registry.token_usage_contributors() {
                        contributor
                            .on_token_usage(
                                &session_store,
                                h.test.codex.thread_extension_data(),
                                &turn_store,
                                &usage,
                            )
                            .await;
                    }
                }

                runtime
                    .usage_limit_active_goal_for_turn(turn_store.level_id())
                    .await
                    .expect("native stop persists");
                let stopped = goals
                    .get_thread_goal(&db, thread_id)
                    .await
                    .expect("read stopped goal")
                    .expect("goal remains");
                assert_eq!(
                    stopped.status,
                    codex_protocol::protocol::ThreadGoalStatus::UsageLimited
                );
                assert_eq!(stopped.tokens_used, progress);
                assert!(!runtime.alab_goal_persistence_held());
                let after = runtime
                    .alab_goal_mutation_epoch()
                    .expect("post-stop mutation epoch");
                assert_ne!(after, before);
                assert!(runtime.alab_goal_mutation_epoch_is_current(&after));
                assert!(!runtime.alab_goal_mutation_epoch_is_current(&before));

                for contributor in registry.turn_lifecycle_contributors() {
                    contributor
                        .on_turn_stop(codex_extension_api::TurnStopInput {
                            session_store: &session_store,
                            thread_store: h.test.codex.thread_extension_data(),
                            turn_store: &turn_store,
                        })
                        .await;
                }
                h.test.codex.shutdown_and_wait().await.unwrap();
                drop(evaluation);
            })
            .await
            .expect("native stop progress hang detector");
        }
    });
}

#[test]
fn vanilla_idle_continuation_defers_when_native_persistence_is_held() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("seed the active goal");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("registered native goal runtime");
            runtime.alab_debug_goal_persistence_write().finish(false);
            assert!(runtime.alab_goal_persistence_held());
            codex_alab_core::set_enhanced(false);

            let registry = production_goal_registry(
                Arc::clone(&db),
                Arc::downgrade(&h.test.thread_manager),
                Arc::clone(&goals),
            );
            let session_store = ExtensionData::new("vanilla-held-idle-session");
            let requests_before =
                core_test_support::responses::received_responses_requests(&h._server)
                    .await
                    .len();
            assert!(!h.test.codex.has_active_turn().await);
            for contributor in registry.thread_lifecycle_contributors() {
                contributor
                    .on_thread_idle(codex_extension_api::ThreadIdleInput {
                        cause: codex_extension_api::ThreadIdleCause::Completed,
                        session_store: &session_store,
                        thread_store: h.test.codex.thread_extension_data(),
                    })
                    .await;
            }
            for _ in 0..10 {
                tokio::task::yield_now().await;
            }
            assert!(!h.test.codex.has_active_turn().await);
            assert_eq!(
                core_test_support::responses::received_responses_requests(&h._server)
                    .await
                    .len(),
                requests_before,
                "vanilla mode cannot bypass native persistence admission"
            );
            assert!(runtime.alab_goal_persistence_held());

            codex_alab_core::set_enhanced(true);
            h.test.codex.shutdown_and_wait().await.unwrap();
            drop(evaluation);
        })
        .await
        .expect("vanilla held idle-continuation hang detector");
    });
}

#[test]
fn goal_runtime_retirement_survives_cancellation_and_waits_for_accounting() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let original = status_owner(&goals, thread_id);
            let primary_session = ExtensionData::new(thread_id.to_string());
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("registered native runtime");
            let accounting = runtime
                .alab_debug_progress_accounting_permit()
                .await
                .expect("hold the old owner's accounting lane");

            let mut retirement = Box::pin(
                h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data()),
            );
            let retired_while_accounting = tokio::select! {
                () = &mut retirement => true,
                () = tokio::time::sleep(Duration::from_millis(100)) => false,
            };
            assert!(
                !retired_while_accounting,
                "retirement cannot expose a successor while old accounting can still settle"
            );
            assert_eq!(
                status_owner(&goals, thread_id),
                original,
                "the registry retains the old owner until both native lanes drain"
            );
            let retiring_epoch = runtime
                .alab_goal_mutation_epoch()
                .expect("already-admitted retirement work retains its mutation epoch");
            assert!(runtime.alab_goal_mutation_epoch_is_current(&retiring_epoch));
            assert!(
                !runtime.alab_goal_persistence_held(),
                "status retirement rejects new permits while the final persistence drain remains settleable"
            );

            drop(retirement);
            drop(accounting);
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            assert!(goals.alab_goal_runtime(thread_id).is_none());

            let successor_session = ExtensionData::new(thread_id.to_string());
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            assert_ne!(status_owner(&goals, thread_id), original);
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("goal-runtime accounting retirement hang detector");
    });
}

#[test]
fn committed_upgrade_retirement_reserves_the_registry_until_final_capture_releases_it() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("seed the active goal");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            let original = status_owner(&goals, thread_id);
            let primary_session = ExtensionData::new(thread_id.to_string());
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("registered native runtime");
            runtime
                .restore_after_resume()
                .await
                .expect("activate the setup goal clock");
            let saved = db
                .thread_goals()
                .get_thread_goal(thread_id)
                .await
                .expect("read setup goal")
                .expect("active goal exists");
            let (_, mut transfer) = runtime
                .alab_upgrade_goal_state_checkpoint(thread_id)
                .await
                .expect("prepare exact goal transfer");
            assert!(runtime.alab_commit_goal_state_upgrade(&mut transfer, || true));
            runtime
                .alab_debug_retain_upgrade_goal_usage(
                    "committed-upgrade-retained-turn",
                    &saved.goal_id,
                    37,
                )
                .expect("retain a callback which settled after commit");

            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            assert_eq!(
                status_owner(&goals, thread_id),
                original,
                "source stop keeps the committed runtime key reserved"
            );
            assert!(
                runtime.alab_goal_persistence_held(),
                "a late callback closes ordinary admission until exact final capture drains it"
            );

            let refused_session = ExtensionData::new("committed-upgrade-refused-session");
            let refused_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&refused_session, &refused_thread)
                .await;
            assert_eq!(
                status_owner(&goals, thread_id),
                original,
                "the committed transfer keeps exact registry custody"
            );
            let refused = refused_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("refused successor runtime was constructed");
            assert!(
                refused.alab_model_goal_epoch_permit().await.is_err(),
                "a successor cannot mutate while final capture owns the source"
            );
            assert_eq!(refused.alab_goal_mutation_epoch(), None);
            h.stop_goal_lifecycle(&refused_session, &refused_thread)
                .await;
            assert_eq!(status_owner(&goals, thread_id), original);

            let (captured, clock) = runtime
                .alab_final_upgrade_goal_state(thread_id, &transfer)
                .await
                .expect("capture final source accounting");
            let captured = captured.expect("capture preserves the active goal");
            assert_eq!(captured.tokens_used, saved.tokens_used + 37);
            assert!(clock.is_some(), "capture freezes the exact source clock");

            // Deferred cleanup must not account time or write the captured row,
            // even after another whole-second SQLite accounting boundary passes.
            tokio::time::sleep(Duration::from_millis(1_100)).await;
            assert!(transfer.release_final_capture());
            transfer
                .wait_for_deferred_retirement_prepared()
                .await
                .expect("deferred cleanup prepared");
            assert!(transfer.final_capture_publishable());
            assert!(transfer.publish_final_capture());
            transfer
                .wait_for_deferred_retirement()
                .await
                .expect("deferred cleanup");
            assert_eq!(
                db.thread_goals()
                    .get_thread_goal(thread_id)
                    .await
                    .expect("read row after deferred cleanup"),
                Some(captured),
                "post-capture retirement cannot mutate the transferred row"
            );
            assert!(
                goals.alab_goal_runtime(thread_id).is_none(),
                "cleanup removes the reserved owner after release"
            );
            assert!(runtime.alab_goal_persistence_held());

            let successor_session = ExtensionData::new("committed-upgrade-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            assert_ne!(status_owner(&goals, thread_id), original);
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after final capture release");
            assert!(successor.alab_model_goal_epoch_permit().await.is_ok());
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            h.test.codex.shutdown_and_wait().await.unwrap();
        })
        .await
        .expect("committed goal-retirement coordination hang detector");
    });
}

#[test]
fn abandoned_upgrade_capture_permanently_holds_the_successor() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let original = status_owner(&goals, thread_id);
            let primary_session = ExtensionData::new(thread_id.to_string());
            let mut prepared =
                crate::upgrade_goals::PreparedGoals::prepare(std::slice::from_ref(&h.test.codex))
                    .await
                    .expect("prepare the real native goal owner");
            assert!(prepared.commit_if(|| true));

            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            assert_eq!(status_owner(&goals, thread_id), original);
            drop(prepared);
            tokio::time::timeout(Duration::from_secs(2), async {
                while goals.alab_goal_runtime(thread_id).is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("dropped committed capture releases quarantined retirement");
            assert!(goals.alab_goal_runtime(thread_id).is_none());

            let successor_session = ExtensionData::new("abandoned-upgrade-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after abandoned final capture");
            assert!(
                successor.alab_goal_persistence_held(),
                "abandonment cannot publish a Ready persistence carry"
            );
            assert_eq!(
                successor.alab_goal_mutation_epoch(),
                None,
                "permanent abandonment grants no mutation epoch"
            );
            assert!(successor.alab_automatic_input_permit().await.is_err());
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            h.test.codex.shutdown_and_wait().await.unwrap();
        })
        .await
        .expect("abandoned goal-capture retirement hang detector");
    });
}

#[test]
fn cancelled_final_capture_task_quarantines_while_the_capture_owner_remains_live() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let original = status_owner(&goals, thread_id);
            let invocation = codex_core::config::ExecConfiguration::capture(
                &h.test.config,
                &codex_core::config::ConfigOverrides::default(),
            )
            .expect("capture native invocation");
            let mut forest = h
                .test
                .codex
                .prepare_upgrade_forest(&invocation)
                .await
                .expect("prepare native forest");
            let mut prepared = crate::upgrade_goals::PreparedGoals::prepare(&forest.threads())
                .await
                .expect("prepare exact native goal owners");
            assert!(
                prepared
                    .commit_forest_if(&mut forest, || true)
                    .expect("commit native forest and source readers")
            );
            let capture = prepared
                .into_capture(forest.into_committed().expect("committed native forest"))
                .expect("construct final goal capture");

            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            assert_eq!(status_owner(&goals, thread_id), original);

            let (entered, release) = capture.alab_debug_gate_capture_task();
            let waiter = capture.begin_capture();
            entered.await.expect("capture task owns its release guard");
            capture.alab_debug_abort_capture_task().await;
            drop(release);
            assert!(
                waiter.await.is_err(),
                "an aborted capture task cannot publish a snapshot"
            );
            tokio::time::timeout(Duration::from_secs(2), async {
                while goals.alab_goal_runtime(thread_id).is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("cancelled capture task releases quarantined retirement");

            let successor_session = ExtensionData::new("cancelled-capture-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after cancelled final capture");
            assert!(successor.alab_goal_persistence_held());
            assert!(successor.alab_automatic_input_permit().await.is_err());
            assert!(
                capture.capture().await.is_err(),
                "task cancellation, not capture-object destruction, released retirement"
            );
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            drop(capture);
            h.test.codex.shutdown_and_wait().await.unwrap();
        })
        .await
        .expect("cancelled final-capture task hang detector");
    });
}

#[test]
fn cancelled_capture_after_provisional_release_cannot_publish_a_ready_successor() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let original = status_owner(&goals, thread_id);
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("registered native runtime");
            let invocation = codex_core::config::ExecConfiguration::capture(
                &h.test.config,
                &codex_core::config::ConfigOverrides::default(),
            )
            .expect("capture native invocation");
            let mut forest = h
                .test
                .codex
                .prepare_upgrade_forest(&invocation)
                .await
                .expect("prepare native forest");
            let mut prepared = crate::upgrade_goals::PreparedGoals::prepare(&forest.threads())
                .await
                .expect("prepare exact native goal owners");
            assert!(
                prepared
                    .commit_forest_if(&mut forest, || true)
                    .expect("commit native forest and source readers")
            );
            let capture = prepared
                .into_capture(forest.into_committed().expect("committed native forest"))
                .expect("construct final goal capture");
            let accounting = runtime
                .alab_debug_progress_accounting_permit()
                .await
                .expect("hold deferred cleanup behind its accounting lane");

            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            assert_eq!(status_owner(&goals, thread_id), original);

            let (released, release_gate) = capture.alab_debug_gate_after_final_release();
            let waiter = capture.begin_capture();
            released
                .await
                .expect("capture provisionally released deferred cleanup");
            capture.alab_debug_abort_capture_task().await;
            drop(release_gate);
            assert!(waiter.await.is_err());
            assert_eq!(
                status_owner(&goals, thread_id),
                original,
                "the pending cleanup still retains exact registry custody"
            );

            drop(accounting);
            tokio::time::timeout(Duration::from_secs(2), async {
                while goals.alab_goal_runtime(thread_id).is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("abandoned provisional release removes quarantined retirement");

            let successor_session = ExtensionData::new("provisional-release-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after abandoned provisional release");
            assert!(successor.alab_goal_persistence_held());
            assert_eq!(successor.alab_goal_mutation_epoch(), None);
            assert!(successor.alab_automatic_input_permit().await.is_err());
            assert!(capture.capture().await.is_err());
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            drop(capture);
            h.test.codex.shutdown_and_wait().await.unwrap();
        })
        .await
        .expect("provisional-release cancellation hang detector");
    });
}

#[test]
fn failed_settlement_during_retirement_holds_the_successor() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let primary_session = ExtensionData::new(thread_id.to_string());
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("registered native runtime");
            let failed = runtime.alab_debug_goal_persistence_write();
            let accounting = runtime
                .alab_debug_progress_accounting_permit()
                .await
                .expect("hold retirement behind the old accounting lane");

            let mut retirement = Box::pin(
                h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data()),
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(100), &mut retirement)
                    .await
                    .is_err()
            );
            failed.finish(false);
            drop(accounting);
            retirement.await;

            let successor_session = ExtensionData::new("failed-settlement-successor");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after failed retirement settlement");
            assert!(successor.alab_goal_persistence_held());
            let epoch = successor
                .alab_goal_mutation_epoch()
                .expect("recoverably held successor retains an epoch");
            assert!(!successor.alab_goal_mutation_epoch_is_pristine(&epoch));
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            drop(goals);
        })
        .await
        .expect("failed retirement settlement hang detector");
    });
}

#[test]
fn permanent_quarantine_survives_replacement_and_rejects_a_later_set() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("seed the quarantined goal");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            let original = db
                .thread_goals()
                .get_thread_goal(thread_id)
                .await
                .expect("read original goal")
                .expect("seeded goal");
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("registered native runtime");
            runtime.alab_debug_quarantine_goal_persistence(&original.goal_id);

            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            let successor_session = ExtensionData::new("quarantine-successor");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after quarantine");
            assert!(successor.alab_goal_persistence_held());
            assert!(
                goals
                    .set_thread_goal(
                        &db,
                        GoalSetRequest {
                            thread_id,
                            objective: GoalObjectiveUpdate::Set("must not replace quarantine"),
                            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active,),
                            token_budget: GoalTokenBudgetUpdate::Keep,
                            max_goal_token_budget: None,
                        },
                        async { Ok(()) },
                    )
                    .await
                    .is_err(),
                "a later explicit mutation cannot clear permanent quarantine"
            );
            assert_eq!(
                goals
                    .get_thread_goal(&db, thread_id)
                    .await
                    .expect("read preserved goal")
                    .expect("quarantine preserves the row")
                    .objective,
                original.objective
            );
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("permanent quarantine replacement hang detector");
    });
}

#[test]
fn a_dead_published_weak_owner_permanently_fences_its_successor() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = ThreadId::new();
            let orphan_session = ExtensionData::new("orphan-runtime-session");
            let orphan_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&orphan_session, &orphan_thread)
                .await;
            assert!(goals.alab_goal_runtime(thread_id).is_some());
            drop(orphan_thread);
            drop(orphan_session);

            let successor_session = ExtensionData::new("dead-owner-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after vanished runtime");
            assert!(
                successor.alab_goal_persistence_held(),
                "framework loss is not clean retirement"
            );
            assert_eq!(
                successor.alab_goal_mutation_epoch(),
                None,
                "an unclean owner loss cannot mint replacement mutation authority"
            );
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("dead weak runtime fence hang detector");
    });
}

#[test]
fn a_runtime_gap_mutation_makes_every_successor_epoch_non_pristine() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("seed the original registered goal");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;

            let (entered, intent_entered) = tokio::sync::oneshot::channel();
            let (release_intent, released) = tokio::sync::oneshot::channel();
            let setter = tokio::spawn({
                let goals = Arc::clone(&goals);
                let db = db.clone();
                async move {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async move {
                                let _ = entered.send(());
                                released.await.map_err(|error| {
                                    GoalServiceError::Internal(format!(
                                        "runtime-gap release ended: {error}"
                                    ))
                                })
                            },
                        )
                        .await
                }
            });
            intent_entered
                .await
                .expect("runtime-gap intent exists before its SQL write");

            let successor_session = ExtensionData::new("pending-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("fresh successor runtime");
            let pending_epoch = successor
                .alab_goal_mutation_epoch()
                .expect("pending gap owns a successor epoch");
            assert!(!successor.alab_goal_mutation_epoch_is_pristine(&pending_epoch));
            assert!(successor.alab_goal_persistence_held());
            assert!(
                successor.alab_automatic_input_permit().await.is_err(),
                "a successor cannot admit work before the absent mutation settles"
            );

            release_intent
                .send(())
                .expect("release the runtime-gap SQL");
            let outcome = setter
                .await
                .expect("join the runtime-gap mutation")
                .expect("persist the runtime-gap mutation");
            outcome.apply_runtime_effects(&goals).await;
            assert!(!successor.alab_goal_persistence_held());
            let settled_epoch = successor
                .alab_goal_mutation_epoch()
                .expect("settled successor epoch");
            assert!(!successor.alab_goal_mutation_epoch_is_pristine(&settled_epoch));
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;

            let (cancel_entered, cancel_intent_entered) = tokio::sync::oneshot::channel();
            let (_release_cancel, cancel_released) = tokio::sync::oneshot::channel::<()>();
            let cancelled_setter = tokio::spawn({
                let goals = Arc::clone(&goals);
                let db = db.clone();
                async move {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async move {
                                let _ = cancel_entered.send(());
                                cancel_released.await.map_err(|error| {
                                    GoalServiceError::Internal(format!(
                                        "cancelled runtime-gap release ended: {error}"
                                    ))
                                })
                            },
                        )
                        .await
                }
            });
            cancel_intent_entered
                .await
                .expect("cancellable runtime-gap intent exists");
            let cancelled_session = ExtensionData::new("cancelled-successor-session");
            let cancelled_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&cancelled_session, &cancelled_thread)
                .await;
            let cancelled = cancelled_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor created during cancellable gap");
            assert!(cancelled.alab_goal_persistence_held());
            cancelled_setter.abort();
            assert!(
                cancelled_setter
                    .await
                    .expect_err("setter is cancelled")
                    .is_cancelled()
            );
            assert!(
                !cancelled.alab_goal_persistence_held(),
                "cancelling before dispatch releases the attached hold without restoring pristine chronology"
            );
            let cancelled_epoch = cancelled
                .alab_goal_mutation_epoch()
                .expect("cancelled successor retains a non-pristine epoch");
            assert!(!cancelled.alab_goal_mutation_epoch_is_pristine(&cancelled_epoch));
            h.stop_goal_lifecycle(&cancelled_session, &cancelled_thread)
                .await;

            let replacement_session = ExtensionData::new("replacement-successor-session");
            let replacement_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&replacement_session, &replacement_thread)
                .await;
            let replacement = replacement_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after cancelled gap");
            assert!(
                !replacement.alab_goal_persistence_held(),
                "a pre-dispatch cancellation remains ready across runtime replacement"
            );
            let replacement_epoch = replacement
                .alab_goal_mutation_epoch()
                .expect("replacement successor retains a non-pristine epoch");
            assert!(
                !replacement.alab_goal_mutation_epoch_is_pristine(&replacement_epoch),
                "pre-dispatch cancellation advances chronology across runtime replacement"
            );
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Keep,
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Blocked),
                        token_budget: GoalTokenBudgetUpdate::Keep,
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("a later live mutation can dispatch after pre-dispatch cancellation");
            outcome.apply_runtime_effects(&goals).await;
            assert!(
                !replacement.alab_goal_persistence_held(),
                "the acknowledged live mutation leaves the ready successor ready"
            );
            h.stop_goal_lifecycle(&replacement_session, &replacement_thread)
                .await;

            let later_session = ExtensionData::new("later-successor-session");
            let later_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&later_session, &later_thread).await;
            let later = later_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("later fresh successor runtime");
            let later_epoch = later
                .alab_goal_mutation_epoch()
                .expect("later successor retains a usable fenced epoch");
            assert!(
                !later.alab_goal_mutation_epoch_is_pristine(&later_epoch),
                "the durable process-local gap fence applies to every successor"
            );
            assert!(
                !later.alab_goal_persistence_held(),
                "live reconciliation persists across another runtime replacement"
            );
            h.stop_goal_lifecycle(&later_session, &later_thread).await;
        })
        .await
        .expect("runtime-gap mutation fence hang detector");
    });
}

#[test]
fn a_gap_intent_rechecks_permanent_carry_immediately_before_dispatch() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("seed the original goal");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;

            let (entered, record_entered) = tokio::sync::oneshot::channel();
            let (release, released) = tokio::sync::oneshot::channel();
            let pending = tokio::spawn({
                let goals = Arc::clone(&goals);
                let db = db.clone();
                async move {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id,
                                objective: GoalObjectiveUpdate::Set("must not dispatch"),
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async move {
                                let _ = entered.send(());
                                released.await.map_err(|error| {
                                    GoalServiceError::Internal(format!(
                                        "dispatch barrier ended: {error}"
                                    ))
                                })
                            },
                        )
                        .await
                }
            });
            record_entered.await.expect("gap intent waits before SQL");
            goals.alab_debug_permanently_hold_unowned_goal_mutation(thread_id);
            release.send(()).expect("release the pre-SQL intent");
            assert!(
                pending.await.expect("join gap setter").is_err(),
                "live permanent carry must revoke earlier dispatch authorization"
            );
            assert_eq!(
                goals
                    .get_thread_goal(&db, thread_id)
                    .await
                    .expect("read preserved row")
                    .expect("original row remains")
                    .objective,
                OBJECTIVE
            );
        })
        .await
        .expect("gap dispatch recheck hang detector");
    });
}

#[test]
fn a_live_mutation_is_refused_while_an_attached_gap_owns_the_epoch() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("seed the original goal");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;

            let (entered, intent_entered) = tokio::sync::oneshot::channel();
            let (release_intent, released) = tokio::sync::oneshot::channel();
            let gap = tokio::spawn({
                let goals = Arc::clone(&goals);
                let db = db.clone();
                async move {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async move {
                                let _ = entered.send(());
                                released.await.map_err(|error| {
                                    GoalServiceError::Internal(format!(
                                        "overlap release ended: {error}"
                                    ))
                                })
                            },
                        )
                        .await
                }
            });
            intent_entered.await.expect("pending runtime-gap intent");

            let overlap_session = ExtensionData::new("overlap-successor-session");
            let overlap_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&overlap_session, &overlap_thread)
                .await;
            let overlap = overlap_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("runtime attached to the pending gap");
            assert!(overlap.alab_goal_persistence_held());
            let overlap_result = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Keep,
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                        token_budget: GoalTokenBudgetUpdate::Keep,
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await;
            assert!(
                overlap_result.is_err(),
                "the successor cannot dispatch even the same row mutation while the gap owns it"
            );
            release_intent.send(()).expect("release the older gap");
            let outcome = gap
                .await
                .expect("join the older gap")
                .expect("persist the older gap mutation");
            outcome.apply_runtime_effects(&goals).await;
            assert!(!overlap.alab_goal_persistence_held());
            h.stop_goal_lifecycle(&overlap_session, &overlap_thread)
                .await;

            let successor_session = ExtensionData::new("post-overlap-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after overlap");
            let successor_epoch = successor
                .alab_goal_mutation_epoch()
                .expect("the acknowledged gap leaves a usable successor epoch");
            assert!(!successor.alab_goal_mutation_epoch_is_pristine(&successor_epoch));
            assert!(!successor.alab_goal_persistence_held());
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("runtime-gap overlap hang detector");
    });
}

#[test]
fn multiple_gap_intents_dispatch_only_the_oldest_without_poisoning_the_successor() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("seed the original goal");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;

            let (first_entered, first_waiting) = tokio::sync::oneshot::channel();
            let (release_first, first_released) = tokio::sync::oneshot::channel();
            let first = tokio::spawn({
                let goals = Arc::clone(&goals);
                let db = db.clone();
                async move {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async move {
                                let _ = first_entered.send(());
                                first_released.await.map_err(|error| {
                                    GoalServiceError::Internal(format!(
                                        "first gap release ended: {error}"
                                    ))
                                })
                            },
                        )
                        .await
                }
            });
            first_waiting.await.expect("first gap intent is pending");

            let second = tokio::spawn({
                let goals = Arc::clone(&goals);
                let db = db.clone();
                async move {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Blocked),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async move { panic!("a rejected overlap must not record provenance") },
                        )
                        .await
                }
            });
            assert!(second.await.expect("join rejected overlap").is_err());

            let successor_session = ExtensionData::new("multiple-gap-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor for both pending gaps");
            assert!(successor.alab_goal_persistence_held());
            assert!(successor.alab_goal_mutation_epoch().is_some());

            release_first.send(()).expect("release first gap");
            let outcome = first
                .await
                .expect("join first gap")
                .expect("the oldest gap owns the exact SQL dispatch");
            outcome.apply_runtime_effects(&goals).await;
            let preserved = goals
                .get_thread_goal(&db, thread_id)
                .await
                .expect("read serialized goal")
                .expect("goal remains");
            assert_eq!(preserved.objective, OBJECTIVE);
            assert_eq!(
                preserved.status,
                codex_protocol::protocol::ThreadGoalStatus::Paused
            );
            assert!(!successor.alab_goal_persistence_held());
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("multiple runtime-gap intents hang detector");
    });
}

#[test]
fn a_later_live_failure_revokes_an_attached_gap_before_sql_dispatch() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("seed the original goal");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;

            let (entered, waiting) = tokio::sync::oneshot::channel();
            let (release, released) = tokio::sync::oneshot::channel();
            let pending = tokio::spawn({
                let goals = Arc::clone(&goals);
                let db = db.clone();
                async move {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id,
                                objective: GoalObjectiveUpdate::Set("must not dispatch"),
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async move {
                                let _ = entered.send(());
                                released.await.map_err(|error| {
                                    GoalServiceError::Internal(format!(
                                        "late-failure gap release ended: {error}"
                                    ))
                                })
                            },
                        )
                        .await
                }
            });
            waiting.await.expect("gap intent waits before SQL");

            let successor_session = ExtensionData::new("failed-gap-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor with attached gap");
            successor.alab_debug_goal_persistence_write().finish(false);

            release.send(()).expect("release gap after live failure");
            assert!(pending.await.expect("join rejected gap").is_err());
            assert_eq!(
                goals
                    .get_thread_goal(&db, thread_id)
                    .await
                    .expect("read preserved goal")
                    .expect("original goal remains")
                    .objective,
                OBJECTIVE
            );
            assert!(successor.alab_goal_persistence_held());
            let failed_epoch = successor
                .alab_goal_mutation_epoch()
                .expect("a recoverable failed write retains an explicit mutation epoch");
            assert!(!successor.alab_goal_mutation_epoch_is_pristine(&failed_epoch));
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("attached gap live-failure hang detector");
    });
}

#[test]
fn a_native_stop_is_refused_before_sql_while_an_attached_gap_owns_the_mutation() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("seed the original goal");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;

            let (entered, waiting) = tokio::sync::oneshot::channel();
            let (release, released) = tokio::sync::oneshot::channel();
            let gap = tokio::spawn({
                let goals = Arc::clone(&goals);
                let db = db.clone();
                async move {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async move {
                                let _ = entered.send(());
                                released.await.map_err(|error| {
                                    GoalServiceError::Internal(format!(
                                        "native-stop gap release ended: {error}"
                                    ))
                                })
                            },
                        )
                        .await
                }
            });
            waiting.await.expect("gap intent waits before SQL");

            let successor_session = ExtensionData::new("native-stop-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor with attached gap");

            let stop = successor
                .usage_limit_active_goal_for_turn("attached-gap-native-stop")
                .await;
            assert!(
                stop.is_err(),
                "the native stop cannot dispatch beside an attached gap mutation"
            );
            assert_eq!(
                goals
                    .get_thread_goal(&db, thread_id)
                    .await
                    .expect("read row before the gap settles")
                    .expect("original row remains")
                    .status,
                codex_protocol::protocol::ThreadGoalStatus::Active
            );

            release.send(()).expect("release older gap");
            let outcome = gap
                .await
                .expect("join older gap")
                .expect("the refused native stop leaves the older gap exact");
            outcome.apply_runtime_effects(&goals).await;
            assert_eq!(
                goals
                    .get_thread_goal(&db, thread_id)
                    .await
                    .expect("read settled gap row")
                    .expect("goal remains")
                    .status,
                codex_protocol::protocol::ThreadGoalStatus::Paused
            );
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("attached gap versus native stop hang detector");
    });
}

#[test]
fn a_failed_live_owner_holds_successors_until_an_acknowledged_mutation() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let thread_id = h.rt.id;
            let db = h.test.codex.state_db().expect("native goal database");
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("registered native runtime");
            let (entered, record_entered) = tokio::sync::oneshot::channel();
            let (_release, released) = tokio::sync::oneshot::channel::<()>();
            let failed = tokio::spawn({
                let goals = Arc::clone(&goals);
                let db = db.clone();
                async move {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id,
                                objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                                token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                                max_goal_token_budget: None,
                            },
                            async move {
                                let _ = entered.send(());
                                released.await.map_err(|error| {
                                    GoalServiceError::Internal(format!(
                                        "failed-owner release ended: {error}"
                                    ))
                                })
                            },
                        )
                        .await
                }
            });
            record_entered
                .await
                .expect("live owner began its recording");
            failed.abort();
            assert!(failed.await.expect_err("owner is cancelled").is_cancelled());
            assert!(runtime.alab_goal_persistence_held());

            let primary_session = ExtensionData::new(thread_id.to_string());
            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            let successor_session = ExtensionData::new("failed-owner-successor-session");
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after failed owner");
            assert!(
                successor.alab_goal_persistence_held(),
                "a retired owner's failed admission transfers to its successor"
            );

            let outcome = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id,
                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("reconcile the successor with an acknowledged mutation");
            apply_setup_goal(&goals, &db, thread_id, outcome).await;
            assert!(!successor.alab_goal_persistence_held());
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;

            let final_session = ExtensionData::new("reconciled-successor-session");
            let final_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&final_session, &final_thread).await;
            let final_runtime = final_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("successor after reconciliation");
            assert!(
                !final_runtime.alab_goal_persistence_held(),
                "reconciliation persists across runtime replacement"
            );
            h.stop_goal_lifecycle(&final_session, &final_thread).await;
        })
        .await
        .expect("failed live-owner successor fence hang detector");
    });
}

#[test]
fn goal_runtime_retirement_waits_for_the_registered_owners_mutation() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let original = status_owner(&goals, h.rt.id);
            let db = h.test.codex.state_db().expect("native goal database");
            let thread_id = h.rt.id;
            let primary_session = ExtensionData::new(thread_id.to_string());
            let (entered, mutation_entered) = tokio::sync::oneshot::channel();
            let (release_mutation, released) = tokio::sync::oneshot::channel();
            let setter_db = db.clone();
            let setter = tokio::spawn({
                let goals = Arc::clone(&goals);
                async move {
                    goals
                        .alab_scope_host_goal_mutation(thread_id, async {
                            let outcome = goals
                                .set_thread_goal(
                                    &setter_db,
                                    GoalSetRequest {
                                        thread_id,
                                        objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                                        status: Some(
                                            codex_protocol::protocol::ThreadGoalStatus::Active,
                                        ),
                                        token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                                        max_goal_token_budget: None,
                                    },
                                    async move {
                                        let _ = entered.send(());
                                        released.await.map_err(|error| {
                                            GoalServiceError::Internal(format!(
                                                "mutation release ended: {error}"
                                            ))
                                        })
                                    },
                                )
                                .await?;
                            apply_setup_goal(&goals, &setter_db, thread_id, outcome).await;
                            Ok::<(), GoalServiceError>(())
                        })
                        .await
                        .map_err(GoalServiceError::Internal)?
                }
            });
            mutation_entered
                .await
                .expect("goal mutation reached its native permit");

            let mut retirement = Box::pin(
                h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data()),
            );
            let retired_while_mutating = tokio::select! {
                () = &mut retirement => true,
                () = tokio::time::sleep(Duration::from_millis(100)) => false,
            };
            release_mutation
                .send(())
                .expect("release the registered mutation");
            setter
                .await
                .expect("join the registered mutation")
                .expect("persist the registered mutation");
            if !retired_while_mutating {
                retirement.await;
            }

            assert!(
                !retired_while_mutating,
                "retirement cannot publish absence while its registered owner can still commit"
            );
            assert!(
                goals.alab_goal_runtime(h.rt.id).is_none(),
                "the matching retirement removes the owner after its mutation settles"
            );

            let retired = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("retired native runtime remains in its extension store");
            h.start_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            assert!(
                goals.alab_goal_runtime(h.rt.id).is_none(),
                "a retired handle can never re-register itself"
            );
            let refusal = match retired.alab_model_goal_epoch_permit().await {
                Ok(_) => panic!("the retired handle reacquired SQL mutation authority"),
                Err(error) => error,
            };
            assert!(
                refusal.contains("transferring to another process"),
                "retired owner is refused by its closed transfer barrier: {refusal}"
            );

            let successor_session = ExtensionData::new(thread_id.to_string());
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            assert_ne!(
                status_owner(&goals, thread_id),
                original,
                "a fresh successor owns a distinct runtime incarnation"
            );
            let successor = successor_thread
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("fresh successor runtime");
            let successor_epoch = successor
                .alab_goal_mutation_epoch()
                .expect("successful predecessor mutation remains usable");
            assert!(
                !successor.alab_goal_mutation_epoch_is_pristine(&successor_epoch),
                "a clean retirement cannot erase predecessor mutation chronology"
            );
            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("goal-runtime retirement hang detector");
    });
}

#[test]
fn goal_runtime_retirement_is_published_before_a_successor_becomes_live() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let goals = Arc::new(GoalService::new());
            let events = Arc::new(RuntimeStatusEvents::default());
            let h = Harness::new_with_goal_status_sink(goals.clone(), events.clone()).await;
            let thread_id = h.rt.id;
            let original = status_incarnation(&goals, thread_id);
            let primary_session = ExtensionData::new(thread_id.to_string());

            h.stop_goal_lifecycle(&primary_session, h.test.codex.thread_extension_data())
                .await;
            let successor_session = ExtensionData::new(thread_id.to_string());
            let successor_thread = ExtensionData::new(thread_id.to_string());
            h.start_goal_lifecycle(&successor_session, &successor_thread)
                .await;
            let successor = status_incarnation(&goals, thread_id);
            goals.alab_emit_runtime_status(thread_id);

            {
                let updates = events
                    .updates
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let retired_at = updates
                    .iter()
                    .rposition(|update| {
                        update.incarnation == original && update.persistence.is_none()
                    })
                    .expect("the original owner published retirement");
                let successor_at = updates
                    .iter()
                    .rposition(|update| {
                        update.incarnation == successor && update.persistence.is_some()
                    })
                    .expect("the successor published live status");
                assert!(
                    retired_at < successor_at,
                    "retirement must be ordered before the registry exposes its successor"
                );
                assert!(
                    updates[successor_at + 1..]
                        .iter()
                        .all(|update| update.incarnation != original),
                    "the retired watcher cannot overwrite a successor's live publication"
                );
            }

            h.stop_goal_lifecycle(&successor_session, &successor_thread)
                .await;
        })
        .await
        .expect("goal-runtime successor publication hang detector");
    });
}
