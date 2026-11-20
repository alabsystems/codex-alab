// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the production updater with actual native manager admission.
use super::*;

#[test]
fn a_pending_provider_reset_keeps_the_original_goal_timer_on_upgrade_refusal() {
    native_test(|| async {
        let goals = Arc::new(GoalService::new());
        let mut h = Harness::new_with_goal_capabilities(
            Some(Arc::clone(&goals)),
            HarnessOptions {
                seed_pending_act: false,
                ..HarnessOptions::default()
            },
        )
        .await;
        h.fake.set_policy_key("limit_wait", "true");
        h.fake.set_policy_key("probe_api", "false");
        wait_for_supervisor(|| {
            h.autopilot.sup.policy().is_some_and(|policy| {
                policy.upgrade && policy.limit_wait && policy.resume_limits && !policy.probe_api
            })
        })
        .await;
        let resets_at = now_ms() / 1000 + 300;
        responses::mount_response_handler(&h._server, move |_| {
            wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "error": {"type": "usage_limit_reached", "plan_type": "pro", "resets_at": resets_at}
            }))
        })
        .await;
        let db = h.test.codex.state_db().expect("goal database");
        goals
            .set_thread_goal(
                &db,
                GoalSetRequest {
                    thread_id: h.rt.id,
                    objective: GoalObjectiveUpdate::Set("Finish the original task"),
                    status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                    token_budget: GoalTokenBudgetUpdate::Keep,
                    max_goal_token_budget: None,
                },
                async { Ok(()) },
            )
            .await
            .expect("start the native goal")
            .apply_runtime_effects(&goals)
            .await;
        let EventMsg::TurnComplete(done) = core_test_support::wait_for_event_with_timeout(
            &h.test.codex,
            |event| matches!(event, EventMsg::TurnComplete(_)),
            Duration::from_secs(60),
        )
        .await
        else {
            unreachable!()
        };
        assert_eq!(
            done.error.and_then(|error| error.codex_error_info),
            Some(codex_protocol::protocol::CodexErrorInfo::UsageLimitExceeded)
        );
        wait_for_supervisor(|| h.rt.owed_timer().is_some()).await;
        // The timer is published inside evaluate. Allow that decision to finish so
        // the upgrade reaches its pending-wait check on the idle, original thread.
        drop(h.rt.hold_eval_when_free().await);
        let original = h.rt.owed_timer().expect("the provider wall owes a resume");
        assert!(original.resume_goal);
        assert_eq!(original.rule, words::RULE_LIMIT_RESUME);
        assert_eq!(
            original.native_origin.as_ref().unwrap().turn_id,
            done.turn_id
        );
        let point = h.rt.state().point.expect("native terminal point");
        let Some(Wall::Usage {
            reset_at: Some(reset),
            ..
        }) = point.wall
        else {
            panic!("typed usage wall lost its dated reset");
        };
        assert_eq!(
            original.not_before,
            reset + h.autopilot.timing.reset_grace + h.rt.jitter
        );
        assert_eq!(h.rt.timer_due(), Some(original.not_before));
        let goal_at_wall = db
            .thread_goals()
            .get_thread_goal(h.rt.id)
            .await
            .expect("read the walled goal")
            .expect("the original goal");
        assert_eq!(
            goal_at_wall.status,
            codex_state::ThreadGoalStatus::UsageLimited
        );

        assert!(!h.autopilot.try_upgrade().await);
        let journal = h.autopilot.sup.journal();
        assert!(
            journal
                .iter()
                .any(|line| line.contains("upgrade waits: an autopilot wait is pending")),
            "{journal:?}"
        );
        assert!(h.requests.try_recv().is_err());
        assert!(!h.shared.exec_pending());
        assert_eq!(h.rt.state().point_at(), Some(point.at));
        assert_eq!(h.rt.timer_due(), Some(original.not_before));
        assert_eq!(h.rt.owed_timer(), Some(original));
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .expect("read the goal after deferral"),
            Some(goal_at_wall)
        );
        h.test.codex.shutdown_and_wait().await.unwrap();
    });
}

#[test]
fn unpublished_source_refusal_preserves_work_and_reopens_native_admission() {
    native_test(|| async {
        let mut h = Harness::new_with_goals(Some(Arc::new(GoalService::new()))).await;
        let owed = h.rt.owed_timer().expect("original paced work");
        assert!(
            codex_alab_tui::upgrade::test_support::prepare(h.shared)
                .unwrap()
                .native_builds()
                .is_err(),
            "this fixture runs outside a published native package"
        );
        let pilot = Arc::clone(&h.autopilot);
        let mut upgrading = tokio::spawn(async move { pilot.try_upgrade().await });
        let Err((leaving, journal)) =
            before_upgrade_ends(&mut upgrading, h.requests.recv(), &h.autopilot.sup).await
        else {
            panic!("an unpublished source queued a terminal handoff");
        };
        assert!(!leaving);
        assert!(journal.iter().any(|line| {
            line.contains("upgrade native image recipe: the running image has no exact native published build")
        }), "{journal:?}");
        assert!(!h.shared.exec_pending());
        assert!(h.requests.try_recv().is_err());
        let restored =
            h.rt.owed_timer()
                .expect("refused update retains paced work");
        assert_eq!(restored.rule, owed.rule);
        assert_eq!(restored.text, owed.text);
        let forest = h.autopilot.prepare_upgrade_forest().await.unwrap();
        let source =
            crate::upgrade_source::Prepared::from_forest(h.test.codex.exec_thread_id(), forest)
                .await
                .unwrap();
        assert!(
            source.valid(),
            "Core and goal admission reopened after refusal"
        );
        drop(source);
        h.test
            .codex
            .submit(codex_protocol::protocol::Op::Interrupt)
            .await
            .expect("ordinary native input remains admitted");
        h.test.codex.shutdown_and_wait().await.unwrap();
    });
}

#[test]
fn native_fixture_prepares_its_own_runtime_forest_without_global_tui_inputs() {
    native_test(|| async {
        let h = Harness::new().await;
        let forest = h.autopilot.prepare_upgrade_forest().await.unwrap();
        assert_eq!(forest.roots(), &[h.test.codex.exec_thread_id()]);
        assert!(forest.valid());
        let source =
            crate::upgrade_source::Prepared::from_forest(h.test.codex.exec_thread_id(), forest)
                .await
                .unwrap();
        assert!(source.valid());
        assert!(source.goal_snapshots().all(|(_, goal)| goal.is_none()));
        drop(source);
        assert!(!h.shared.exec_pending());
        h.test.codex.shutdown_and_wait().await.unwrap();
    });
}

#[test]
fn upgrade_facts_use_captured_handles_while_a_new_root_waits_for_the_registry() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            let options = h.test.start_thread_options().await;
            let mut inventory = h.autopilot.prepare_upgrade_inventory().await.unwrap();
            let threads = inventory.threads();
            assert_eq!(threads.len(), 1);
            let mut startup = Box::pin(h.test.thread_manager.start_thread(options));
            tokio::select! {
                result = &mut startup => panic!("startup passed the retained registry: {}", result.is_ok()),
                () = async {
                    while inventory.valid() {
                        tokio::task::yield_now().await;
                    }
                } => {}
            }
            // The writer is now waiting for our inventory. A fresh manager read
            // here would deadlock behind that writer under Tokio's fairness rule.
            let facts = h.autopilot.upgrade_facts_inventory(None, Some(&threads)).await;
            assert_eq!(facts.threads.len(), 1);
            assert_eq!(facts.threads[0].id, h.rt.key());
            assert!(!inventory.commit_if(|| panic!("new startup revoked the handoff")));
            drop(inventory);
            let second = startup.await.expect("rollback permits the original startup").thread;
            second.shutdown_and_wait().await.unwrap();
            h.test.codex.shutdown_and_wait().await.unwrap();
        }).await.expect("native inventory and writer hang detector");
    });
}

#[test]
fn native_queued_input_revokes_the_actual_upgrade_without_a_shared_generation_change() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for new_input in [false, true] {
                let mut h = Harness::new().await;
                let owed = h.rt.owed_timer().expect("original owed act");
                let pilot = Arc::clone(&h.autopilot);
                let mut upgrading = Box::pin(pilot.try_upgrade());
                let request = tokio::select! {
                    request = h.requests.recv() => request.expect("production conditional handoff"),
                    result = &mut upgrading => panic!("upgrade ended before its native decision: {result}"),
                };
                assert!(!h.shared.exec_pending());
                let carried_deadline = h.shared.owed_act().unwrap().not_before_ms;
                let generation = h.shared.upgrade_generation();
                if new_input {
                    h.test.codex.submit(codex_protocol::protocol::Op::Interrupt)
                        .await.expect("native input remains accepted during preparation");
                    assert_eq!(h.shared.upgrade_generation(), generation,
                        "this control isolates native manager admission from Shared invalidation");
                }
                let rollback_before = (clock(), now_ms());
                assert_eq!(request.decide(true), !new_input);
                assert_eq!(upgrading.await, !new_input);
                let rollback_after = (clock(), now_ms());
                assert_eq!(h.shared.exec_pending(), !new_input);
                if new_input {
                    let restored = h.rt.owed_timer().expect("refusal restores the original act");
                    assert_restored_deadline(restored.not_before, carried_deadline, rollback_before, rollback_after);
                    assert_eq!(restored.rule, owed.rule);
                    assert_eq!(restored.text, owed.text);
                    // Acceptance retains native admission until the queued interrupt finishes
                    // dispatch. Rollback releases this handoff, not that accepted operation.
                    let barrier = loop {
                        match h.test.codex.prepare_upgrade_manager().await {
                            Ok(barrier) => break barrier,
                            Err(error)
                                if matches!(error.details(),
                                    codex_protocol::error::CodexErrorDetails::InvalidRequest(reason)
                                        if reason == &codex_alab_core::upgrade_barrier::Refusal::PendingAdmission.to_string()) =>
                            {
                                tokio::task::yield_now().await;
                            }
                            Err(error) => panic!("unexpected manager admission refusal: {error}"),
                        }
                    };
                    assert!(barrier.valid(), "refusal releases the manager barrier after native input drains");
                    drop(barrier);
                }
                h.test.codex.shutdown_and_wait().await.unwrap();
            }
        }).await.expect("native queued input handoff hang detector");
    });
}

#[test]
fn cancelled_upgrade_releases_the_native_inventory_before_a_late_tui_event() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut h = Harness::new().await;
            let owed = h.rt.owed_timer().expect("original owed act");
            let pilot = Arc::clone(&h.autopilot);
            let mut upgrading = Box::pin(pilot.try_upgrade());
            let request = tokio::select! {
                request = h.requests.recv() => request.expect("queued native handoff"),
                result = &mut upgrading => panic!("upgrade ended before the TUI decision: {result}"),
            };
            let carried_deadline = h.shared.owed_act().unwrap().not_before_ms;
            let rollback_before = (clock(), now_ms());
            drop(upgrading);
            let rollback_after = (clock(), now_ms());
            assert!(!request.decide(true), "a cancelled request cannot commit later");
            assert!(!h.shared.exec_pending());
            assert!(h.test.codex.prepare_upgrade_manager().await.unwrap().valid());
            let restored = h.rt.owed_timer().expect("cancelled handoff restores its act");
            assert_eq!(restored.rule, owed.rule);
            assert_eq!(restored.text, owed.text);
            assert_restored_deadline(restored.not_before, carried_deadline, rollback_before, rollback_after);
            h.test.codex.shutdown_and_wait().await.unwrap();
        }).await.expect("cancelled inventory handoff hang detector");
    });
}

#[test]
fn changing_the_primary_after_target_preparation_cannot_resume_the_previous_conversation() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut h = Harness::new().await;
            let second = h.test.thread_manager.start_thread(h.test.start_thread_options().await)
                .await.unwrap().thread;
            let next = second.exec_thread_id().to_string();
            let (entered, paused) = tokio::sync::oneshot::channel();
            let (resume, release) = tokio::sync::oneshot::channel();
            *lock(&h.autopilot.before_upgrade_take) = Some((entered, release));
            let pilot = Arc::clone(&h.autopilot);
            let mut upgrading = Box::pin(pilot.try_upgrade());
            tokio::select! {
                reached = paused => reached.expect("target prepared before the native inventory"),
                result = &mut upgrading => panic!("upgrade ended before target preparation: {result}"),
            }
            let view = h.shared.view();
            let rollout = view.primary().unwrap().rollout_path.as_deref();
            h.shared.observe(&Frame {
                primary_thread: Some(&next),
                displayed_thread: Some(&next),
                rollout_path: rollout,
                cwd: view.cwd.as_deref().unwrap(),
                model: "gpt-5.5",
                codex_home: view.codex_home.as_deref().unwrap(),
                state: ProgramState::Idle,
                embedded: view.embedded,
                draft_empty: true,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            }, now_ms());
            resume.send(()).unwrap();
            let request = tokio::select! {
                request = h.requests.recv() => request.expect("native decision for the prepared target"),
                result = &mut upgrading => panic!("upgrade ended before checking its resume: {result}"),
            };
            assert!(!request.decide(true), "the old resume must not replace the new conversation");
            assert!(!upgrading.await);
            assert!(!h.shared.exec_pending());
            assert_eq!(h.shared.view().primary_thread.as_deref(), Some(next.as_str()));
            second.shutdown_and_wait().await.unwrap();
            h.test.codex.shutdown_and_wait().await.unwrap();
        }).await.expect("changed primary resume handoff hang detector");
    });
}

/// Real wall and monotonic samples are not atomic. Bound the conversion by the
/// samples surrounding rollback, rather than imposing a scheduler latency bound.
fn assert_restored_deadline(
    restored: Instant,
    carried_epoch: u64,
    before: (Instant, u64),
    after: (Instant, u64),
) {
    let lower = before.0 + Duration::from_millis(carried_epoch.saturating_sub(after.1));
    let upper = after.0 + Duration::from_millis(carried_epoch.saturating_sub(before.1));
    assert!(
        restored >= lower && restored <= upper,
        "restored deadline {restored:?} is outside its native conversion samples {lower:?}..={upper:?}"
    );
}
