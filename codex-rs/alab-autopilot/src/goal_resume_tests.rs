// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An automatic usage resume cannot overwrite a goal a person moved while it prepared.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use crate::extension::test_progress;
use codex_alab_core::wake::Reading;
use codex_alab_core::wall_kind::LimitClass;
use core_test_support::responses::received_responses_requests;
use std::future::Future;
use std::task::Poll;

#[test]
fn a_usage_resume_preserves_a_goal_changed_while_it_waits_for_the_mutation_permit() {
    native_test(|| async {
        use codex_state::ThreadGoalStatus;
        let progress = test_progress::Progress::default();
        for changed in [
            "unchanged",
            "paused",
            "blocked",
            "budget-limited",
            "complete",
            "replacement",
            "objective",
            "budget",
            "native",
        ] {
            progress.case(changed);
            // Keep the existing hang bound for each independently isolated fixture.
            tokio::time::timeout(Duration::from_secs(60), async {
                let goals = Arc::new(GoalService::new());
                progress.phase("create native harness");
                let setup_progress = progress.clone();
                let h = Harness::new_with_goals_and_setup_observer(
                    Some(Arc::clone(&goals)),
                    Arc::new(move |phase| setup_progress.phase(phase)),
                )
                .await;
                progress.phase("set immediate decision point");
                h.immediate_point().await;
                progress.phase("hold model response");
                h.hold_model_response().await;
                let db = h.test.codex.state_db().expect("goal database");
                progress.phase("write original limited goal");
                let original = db
                    .thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "the original task",
                        ThreadGoalStatus::UsageLimited,
                        None,
                    )
                    .await
                    .expect("original limited goal");
                let authority = DecisionAuthority {
                    quota: None,
                    quota_identity: None,
                    policy: h.autopilot.sup.policy().expect("proven policy"),
                    offs: h.autopilot.sup.offs(),
                    point: h.rt.state().point_at(),
                    accepted_turns: h.rt.accepted_turns.load(Ordering::SeqCst),
                    native_origin: h.rt.state().native_origin,
                };
                let before = h.rt.state();

                // A real GoalService mutation holds the runtime's permit while it records
                // the person's intent. The stale automatic resume must queue behind it.
                let (held, holding) = tokio::sync::oneshot::channel();
                let (change, changing) = tokio::sync::oneshot::channel();
                let (applied, applying) = tokio::sync::oneshot::channel();
                let (retire, retiring) = tokio::sync::oneshot::channel();
                let service = Arc::clone(&goals);
                let person_db = db.clone();
                let id = h.rt.id;
                let person = tokio::spawn(async move {
                    service
                        .alab_scope_host_goal_mutation(id, async {
                            let outcome = service
                                .set_thread_goal(
                                    &person_db,
                                    GoalSetRequest {
                                        thread_id: id,
                                        objective: if changed == "objective" {
                                            GoalObjectiveUpdate::Set("the person's revised task")
                                        } else {
                                            GoalObjectiveUpdate::Keep
                                        },
                                        status: Some(match changed {
                                            "paused" => {
                                                codex_protocol::protocol::ThreadGoalStatus::Paused
                                            }
                                            "blocked" => {
                                                codex_protocol::protocol::ThreadGoalStatus::Blocked
                                            }
                                            "budget-limited" => {
                                                codex_protocol::protocol::ThreadGoalStatus::BudgetLimited
                                            }
                                            "complete" => {
                                                codex_protocol::protocol::ThreadGoalStatus::Complete
                                            }
                                            _ => codex_protocol::protocol::ThreadGoalStatus::UsageLimited,
                                        }),
                                        token_budget: if changed == "budget" {
                                            GoalTokenBudgetUpdate::Set(Some(500))
                                        } else {
                                            GoalTokenBudgetUpdate::Keep
                                        },
                                        max_goal_token_budget: None,
                                    },
                                    async {
                                        held.send(()).expect("person holds the mutation permit");
                                        changing.await.expect("release person's recorded intent");
                                        Ok(())
                                    },
                                )
                                .await
                                .expect("person's mutation");
                            outcome.apply_runtime_effects(&service).await;
                            applied.send(()).expect("person's exact effects applied");
                            // Keep the real host permit while the test stages the newer
                            // identity/input and reads its expected state. Releasing it
                            // earlier would let the queued resume overtake those controls.
                            retiring.await.expect("release person's mutation custody");
                        })
                        .await
                        .expect("person's native mutation custody")
                });
                progress.phase("wait for person's mutation permit");
                holding
                    .await
                    .expect("the real goal mutation permit is held");
                let (entered, mut entry) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((entered, blocked));
                let mut resuming =
                    Box::pin(h.autopilot.resume_goal(&h.rt, &h.test.codex, &authority));
                progress.phase("reach automatic resume submit gate");
                std::future::poll_fn(|cx| {
                    assert!(resuming.as_mut().poll(cx).is_pending());
                    match entry.try_recv() {
                        Ok(()) => Poll::Ready(()),
                        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => Poll::Pending,
                        Err(error) => panic!("initial goal read: {error}"),
                    }
                })
                .await;
                progress.phase("queue automatic resume behind mutation permit");
                release
                    .send(())
                    .expect("resume may ask for the held permit");
                std::future::poll_fn(|cx| {
                    assert!(
                        resuming.as_mut().poll(cx).is_pending(),
                        "the service's real mutation permit holds the resume"
                    );
                    Poll::Ready(())
                })
                .await;
                progress.phase("finish person's goal mutation");
                change.send(()).expect("person's mutation goes first");
                applying.await.expect("person's mutation and effects finished");
                // A replacement may retain the same objective and limit; its identity
                // still belongs to the person's new task, never the stale resume.
                if changed == "replacement" {
                    progress.phase("replace goal identity");
                    db.thread_goals()
                        .replace_thread_goal(
                            h.rt.id,
                            &original.objective,
                            ThreadGoalStatus::UsageLimited,
                            None,
                        )
                        .await
                        .expect("replacement goal");
                }
                let expected_state = if changed == "native" {
                    progress.phase("start newer native input");
                    h.test
                        .codex
                        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                            text: "the person's correction".into(),
                            text_elements: Vec::new(),
                        }]))
                        .await
                        .expect("new native input");
                    progress.phase("observe newer native turn");
                    upgrade_handoff_tests::wait_for_supervisor(|| {
                        h.shared.turn_running_on(h.rt.key())
                    })
                    .await;
                    h.rt.state()
                } else {
                    before
                };
                progress.phase("read person's goal");
                let expected = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("person's goal")
                    .expect("goal exists");
                progress.phase("release person's mutation custody");
                retire.send(()).expect("person's staged changes go first");
                person.await.expect("person's mutation custody finished");
                progress.phase("complete automatic goal resume");
                let outcome = resuming.await;
                progress.phase("read final goal");
                let after = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("read final goal")
                    .expect("goal remains");
                progress.phase("check goal and runtime invariants");
                if changed != "unchanged" {
                    assert_eq!(after, expected, "the person's goal must stand: {changed}");
                    assert_eq!(outcome, GoalResume::NotNow);
                    assert_eq!(
                        h.rt.state(),
                        expected_state,
                        "an abandoned resume spends no try and leaves newer input intact"
                    );
                } else {
                    assert_eq!(after.goal_id, original.goal_id);
                    assert_eq!(after.status, ThreadGoalStatus::Active);
                    assert_eq!(outcome, GoalResume::Resumed);
                    assert!(h.rt.state().point.is_none(), "the admitted try was booked");
                }
                assert!(
                    h.fake.acts().is_empty(),
                    "the direct goal service types no act"
                );
                progress.phase("shutdown native Core");
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
                progress.phase("drop native fixture");
            })
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "usage resume race hang detector: {error:?}; {}",
                    progress.describe()
                )
            });
        }
    });
}

#[test]
fn a_failed_goal_resume_retries_only_the_current_runtime() {
    native_test(|| async {
        const REPORT: &str = "meta set attention owner=codex-alab";
        const CLEAR: &str = "meta unset attention owner=codex-alab-wait";
        const REFUSAL: &str = "fixture-goal-write-refused";
        const WAIT: &str = "The earlier usage wait";
        for change in [
            "unchanged",
            "replacement",
            "lost-allowance",
            "clear",
            "newer-point",
        ] {
            tokio::time::timeout(upgrade_handoff_tests::NATIVE_HANG, async {
                let h = Harness::new_with_control_timeout(
                    Some(Arc::new(GoalService::new())),
                    None,
                    Duration::from_secs(30),
                )
                .await;
                h.fake.set_policy_key("limit_wait", "true");
                upgrade_handoff_tests::wait_for_supervisor(|| {
                    h.autopilot.sup.policy().is_some_and(|policy| policy.limit_wait)
                })
                .await;
                h.immediate_point().await;
                h.hold_model_response().await;
                let db = h.test.codex.state_db().expect("native goal database");
                let original = db
                    .thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "continue the original usage goal",
                        codex_state::ThreadGoalStatus::UsageLimited,
                        None,
                    )
                    .await
                    .expect("write original limited goal");
                let at = clock() - Duration::from_secs(3600);
                let lost_allowance = change == "lost-allowance";
                let reset = if lost_allowance {
                    clock() + Duration::from_secs(24 * 60 * 60)
                } else {
                    at
                };
                h.rt.with_state(|state| {
                    state.observe_turn_end(
                        decide::TurnEnd {
                            at,
                            by_autopilot: true,
                            by_person: false,
                            text: None,
                            earlier: None,
                            questions: false,
                            worked: Duration::from_secs(600),
                            wall: Some(Wall::Usage {
                                limit_class: LimitClass::Ordinary,
                                reset_at: Some(reset),
                                reset_label: None,
                            }),
                            aborted: false,
                            ran_tools: false,
                            restored: false,
                        },
                        &h.autopilot.timing,
                    )
                });
                let asleep = Arc::new(Mutex::new(Duration::ZERO));
                let quota_wall = lost_allowance.then(|| {
                    let reading = Arc::clone(&asleep);
                    h.autopilot.set_wake_read(Arc::new(move || {
                        Some(Reading {
                            asleep: *lock(&reading),
                            last: None,
                        })
                    }));
                    let quota = h.shared.quota();
                    let account = "fixture-account";
                    let identity = QuotaIdentity {
                        account: account.into(),
                        user: Some("fixture-user".into()),
                        owner: 0,
                    };
                    quota.select_owner(account, identity.user.as_deref(), identity.owner);
                    let wall = quota.wall_for(account.into(), "codex".into(), at);
                    *lock(&h.autopilot.quota_test_identity) = Some(identity.clone());
                    *lock(&h.rt.quota_wall) = Some(QuotaWall {
                        wall: wall.clone(),
                        identity,
                    });
                    quota.start_read(1, clock());
                    quota.accept(1, account, vec!["codex".into()], clock());
                    assert!(h.autopilot.quota_restored(&h.rt, &wall, clock()));
                    wall
                });
                h.autopilot.refuse_goal_set(Some(REFUSAL.to_owned()));
                let clears_wait = matches!(change, "clear" | "newer-point");
                let preparation = clears_wait.then(|| {
                    let (entered, prepared) = tokio::sync::oneshot::channel();
                    let (release, paused) = tokio::sync::oneshot::channel();
                    *lock(&h.rt.before_submit) = Some((entered, paused));
                    (prepared, release)
                });
                let gate = if clears_wait { CLEAR } else { REPORT };
                h.fake.gate(gate);
                let pilot = Arc::clone(&h.autopilot);
                let old = Arc::clone(&h.rt);
                let mut evaluating = tokio::spawn(async move { pilot.evaluate(old).await });
                if let Some((prepared, release)) = preparation {
                    tokio::select! {
                        ready = prepared => ready.expect("prepared usage resume"),
                        result = &mut evaluating => panic!("resume ended before preparation: {result:?}"),
                    }
                    assert!(h.autopilot.sup.want_wait_for(
                        h.rt.key(),
                        Some((clock() + Duration::from_secs(3600), WAIT.into())),
                    ));
                    h.autopilot.sup.show_waits().await;
                    assert_eq!(h.fake.attention().get("codex-alab-wait").map(String::as_str), Some(WAIT));
                    release.send(()).expect("allow the prepared goal resume to fail");
                }
                tokio::select! {
                    () = upgrade_handoff_tests::wait_for_supervisor(|| {
                        h.fake.gated_requests().iter().any(|line| {
                            if clears_wait { line.ends_with(CLEAR) } else { line.contains(REFUSAL) }
                        })
                    }) => {}
                    result = &mut evaluating => panic!("resume ended before its failure report: {result:?}"),
                }
                if clears_wait {
                    assert!(h.autopilot.sup.journal().iter().any(|note| note.contains(REFUSAL)));
                    assert!(!h.autopilot.sup.attention_stands(h.rt.key(), "goal"));
                }
                let mut next_evaluation = Box::pin(h.rt.hold_eval_when_free());
                std::future::poll_fn(|cx| {
                    assert!(next_evaluation.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;

                if let Some(wall) = &quota_wall {
                    h.shared.quota().invalidate();
                    assert!(!h.autopilot.quota_restored(&h.rt, wall, clock()));
                }
                let replaced = change == "replacement";
                let replacement = replaced.then(|| {
                    let next = Arc::new(ThreadRt::new(
                        h.rt.id,
                        h.rt.config.clone(),
                        h.rt.token_budget,
                        Duration::ZERO,
                    ));
                    h.test.codex.thread_extension_data().insert_arc(Arc::clone(&next));
                    h.autopilot.keep_thread(&next);
                    assert!(Arc::ptr_eq(&h.autopilot.thread_rt(h.rt.key()).unwrap(), &next));
                    next
                });
                let newer_point = change == "newer-point";
                if newer_point {
                    let before = h.rt.accepted_turns.load(Ordering::SeqCst);
                    let session = ExtensionData::new("session");
                    let successor = ExtensionData::new("turn-after-failed-resume");
                    let settings = h.test.codex.thread_settings_snapshot().await;
                    let thread_store = h.test.codex.thread_extension_data();
                    h.autopilot.on_turn_start(TurnStartInput {
                        turn_id: successor.level_id(),
                        collaboration_mode: &settings.collaboration_mode,
                        token_usage_at_turn_start: None,
                        session_store: &session,
                        thread_store,
                        turn_store: &successor,
                    }).await;
                    h.autopilot.on_turn_stop(TurnStopInput {
                        session_store: &session,
                        thread_store,
                        turn_store: &successor,
                    }).await;
                    assert!(!evaluating.is_finished(), "the wait clear still holds the old resume");
                    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), before + 1);
                    assert!(!h.shared.turn_running_on(h.rt.key()));
                    assert!(Arc::ptr_eq(&h.autopilot.thread_rt(h.rt.key()).unwrap(), &h.rt));
                }
                // Replacement is effective even while the old callback can still
                // run and the old instance has yet to receive its stop flag.
                assert!(!h.rt.stopped.load(Ordering::SeqCst));
                let timer_before = h.rt.timer_seq.load(Ordering::SeqCst);
                assert!(lock(&h.rt.timer).is_none());
                let earliest = clock() + RECHECK;
                h.fake.open_gate(gate);
                evaluating.await.expect("finish the failed evaluation");
                let next_evaluation = next_evaluation.await;
                let deadline = lock(&h.rt.timer).as_ref().map(|timer| timer.at);
                assert_eq!(
                    h.rt.timer_seq.load(Ordering::SeqCst),
                    timer_before + u64::from(!replaced && !newer_point)
                );
                if let Some(next) = replacement {
                    assert_eq!(deadline, None);
                    assert!(lock(&next.timer).is_none());
                } else if newer_point {
                    assert_eq!(deadline, None);
                } else {
                    assert!(deadline.is_some_and(|at| at >= earliest));
                }
                if clears_wait {
                    assert_eq!(h.autopilot.sup.attention_stands(h.rt.key(), "goal"), !newer_point);
                    assert_eq!(h.fake.attention().get("codex-alab").is_some_and(|text| text.contains(REFUSAL)), !newer_point);
                }
                drop(next_evaluation);
                if lost_allowance {
                    h.autopilot.refuse_goal_set(None);
                    let slept = RECHECK * 2;
                    *lock(&asleep) = slept;
                    // The retry reevaluates the original usage window and
                    // waits out its reset with no current quota permission.
                    upgrade_handoff_tests::wait_for_supervisor(|| {
                        h.rt.owed_timer().is_some_and(|owed| owed.not_before >= reset - slept)
                    })
                    .await;
                }
                Autopilot::cancel_timer(&h.rt).await;
                assert_eq!(
                    db.thread_goals().get_thread_goal(h.rt.id).await.expect("read the goal"),
                    Some(original)
                );
                assert!(received_responses_requests(&h._server).await.is_empty());
                assert!(h.fake.acts().is_empty());
                h.test.codex.shutdown_and_wait().await.expect("native shutdown");
            })
            .await
            .unwrap_or_else(|error| panic!("{change}: failed resume hang detector: {error}"));
        }
    });
}

#[test]
fn a_rejected_usage_resume_redecides_a_still_limited_goal_but_leaves_stops_alone() {
    native_test(|| async {
        tokio::time::timeout(upgrade_handoff_tests::NATIVE_HANG, async {
            use codex_state::ThreadGoalStatus;
            for changed in ["objective", "budget", "unread", "paused", "blocked"] {
                let goals = Arc::new(GoalService::new());
                let h = Harness::new_with_goals(Some(Arc::clone(&goals))).await;
                h.fake.set_policy_key("limit_wait", "true");
                upgrade_handoff_tests::wait_for_supervisor(|| {
                    h.autopilot
                        .sup
                        .policy()
                        .is_some_and(|policy| policy.limit_wait)
                })
                .await;
                h.immediate_point().await;
                h.hold_model_response().await;
                let db = h.test.codex.state_db().expect("goal database");
                db.thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "the original task",
                        ThreadGoalStatus::UsageLimited,
                        None,
                    )
                    .await
                    .expect("limited goal");
                let at = clock() - Duration::from_secs(3600);
                h.rt.with_state(|state| {
                    state.observe_turn_end(
                        decide::TurnEnd {
                            at,
                            by_autopilot: true,
                            by_person: false,
                            text: None,
                            earlier: None,
                            questions: false,
                            worked: Duration::from_secs(600),
                            wall: Some(decide::Wall::Usage {
                                limit_class: LimitClass::Ordinary,
                                reset_at: Some(at),
                                reset_label: None,
                            }),
                            aborted: false,
                            ran_tools: false,
                            restored: false,
                        },
                        &h.autopilot.timing,
                    )
                });
                let policy = h.autopilot.sup.policy().expect("proven policy");
                let decision = decide::decide(
                    &h.rt.state(),
                    &policy,
                    &h.autopilot.timing,
                    clock(),
                    Duration::ZERO,
                );
                assert!(
                    matches!(decision.act, Act::ResumeLimit { .. }),
                    "{decision:?}"
                );
                assert!(
                    decision
                        .not_before
                        .is_some_and(|deadline| deadline <= clock())
                );

                // Hold the actual evaluator after it read the original goal. A person's
                // service mutation and runtime effects then finish before it may resume.
                let (entered, mut entry) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((entered, blocked));
                let mut evaluating = Box::pin(Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)));
                std::future::poll_fn(|cx| {
                    assert!(evaluating.as_mut().poll(cx).is_pending());
                    match entry.try_recv() {
                        Ok(()) => Poll::Ready(()),
                        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => Poll::Pending,
                        Err(error) => panic!("initial automatic goal read: {error}"),
                    }
                })
                .await;
                let outcome = goals
                    .set_thread_goal(
                        &db,
                        GoalSetRequest {
                            thread_id: h.rt.id,
                            objective: if matches!(changed, "objective" | "unread") {
                                GoalObjectiveUpdate::Set("the revised task")
                            } else {
                                GoalObjectiveUpdate::Keep
                            },
                            status: Some(match changed {
                                "paused" => codex_protocol::protocol::ThreadGoalStatus::Paused,
                                "blocked" => codex_protocol::protocol::ThreadGoalStatus::Blocked,
                                _ => codex_protocol::protocol::ThreadGoalStatus::UsageLimited,
                            }),
                            token_budget: if changed == "budget" {
                                GoalTokenBudgetUpdate::Set(Some(500))
                            } else {
                                GoalTokenBudgetUpdate::Keep
                            },
                            max_goal_token_budget: None,
                        },
                        async { Ok(()) },
                    )
                    .await
                    .expect("person's goal edit");
                outcome.apply_runtime_effects(&goals).await;
                let expected = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("edited goal")
                    .expect("goal exists");
                let (fresh, fresh_read) = tokio::sync::oneshot::channel();
                let (continue_fresh, fresh_blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((fresh, fresh_blocked));
                let unread = (changed == "unread").then(|| {
                    let (entered, pending_read) = tokio::sync::oneshot::channel();
                    let (release, blocked_read) = tokio::sync::oneshot::channel();
                    *lock(&h.rt.before_rejected_goal_read) = Some((entered, blocked_read));
                    (pending_read, release)
                });
                release.send(()).expect("release stale preparation");
                if let Some((pending_read, release)) = unread {
                    tokio::select! {
                        entered = pending_read => entered.expect("rejected resume reached its goal lookup"),
                        () = &mut evaluating => panic!("evaluation ended before the rejected resume lookup"),
                    }
                    // The native service already rejected the stale preparation. Only the
                    // follow-up lookup should encounter an unreadable goal row.
                    super::description_goal_read_tests::saved_status(&db, h.rt.id, "unreadable")
                        .await;
                    assert!(db.thread_goals().get_thread_goal(h.rt.id).await.is_err());
                    let asleep = Arc::new(Mutex::new(Duration::ZERO));
                    let reading = Arc::clone(&asleep);
                    h.autopilot.set_wake_read(Arc::new(move || {
                        Some(Reading {
                            asleep: *lock(&reading),
                            last: None,
                        })
                    }));
                    let timer_before = h.rt.timer_seq.load(Ordering::SeqCst);
                    let earliest = clock() + RECHECK;
                    let mut next = Box::pin(h.rt.hold_eval_when_free());
                    std::future::poll_fn(|cx| {
                        assert!(next.as_mut().poll(cx).is_pending());
                        Poll::Ready(())
                    })
                    .await;
                    release.send(()).expect("allow the unread follow-up lookup");
                    evaluating.await;
                    let next = next.await;
                    assert_eq!(h.rt.timer_seq.load(Ordering::SeqCst), timer_before + 1);
                    assert!(lock(&h.rt.timer).as_ref().is_some_and(|timer| timer.at >= earliest));
                    assert!(h.fake.acts().is_empty());
                    assert!(received_responses_requests(&h._server).await.is_empty());
                    super::description_goal_read_tests::saved_status(
                        &db,
                        h.rt.id,
                        expected.status.as_str(),
                    )
                    .await;
                    *lock(&asleep) = RECHECK * 2;
                    drop(next);
                } else {
                    evaluating.await;
                }
                if matches!(changed, "paused" | "blocked") {
                    assert_eq!(
                        db.thread_goals()
                            .get_thread_goal(h.rt.id)
                            .await
                            .expect("stopped goal"),
                        Some(expected)
                    );
                    assert!(!h.rt.timer_pending(), "an explicit stop stays quiet");
                    assert!(!h.shared.turn_running_on(h.rt.key()));
                    assert_eq!(
                        h.rt.state().wall.as_ref().expect("unspent wall").attempts,
                        0
                    );
                    assert!(h.fake.acts().is_empty());
                } else {
                    fresh_read
                        .await
                        .expect("a still-limited goal must receive a fresh automatic decision");
                    assert_eq!(
                        h.rt.state().wall.as_ref().expect("unspent wall").attempts,
                        0,
                        "rejecting the stale preparation spends no try"
                    );
                    assert_eq!(
                        db.thread_goals()
                            .get_thread_goal(h.rt.id)
                            .await
                            .expect("edited limited goal"),
                        Some(expected.clone())
                    );
                    continue_fresh.send(()).expect("allow the fresh decision");
                    upgrade_handoff_tests::wait_for_supervisor(|| {
                        h.shared.turn_running_on(h.rt.key())
                            && h.fake
                                .acts()
                                .iter()
                                .any(|act| act == words::RULE_LIMIT_RESUME)
                    })
                    .await;
                    let resumed = db
                        .thread_goals()
                        .get_thread_goal(h.rt.id)
                        .await
                        .expect("resumed goal")
                        .expect("goal remains");
                    assert_eq!(resumed.status, ThreadGoalStatus::Active);
                    assert_eq!(resumed.goal_id, expected.goal_id);
                    assert_eq!(resumed.objective, expected.objective);
                    assert_eq!(resumed.token_budget, expected.token_budget);
                    assert_eq!(
                        h.fake
                            .acts()
                            .iter()
                            .filter(|act| *act == words::RULE_LIMIT_RESUME)
                            .count(),
                        1
                    );
                    if changed == "unread" {
                        loop {
                            let requests = received_responses_requests(&h._server).await;
                            if !requests.is_empty() {
                                assert_eq!(requests.len(), 1);
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    }
                }
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            }
        })
        .await
        .expect("goal resume redecision hang detector");
    });
}

/// A rejected preparation owes a fresh goal decision even when its original quota
/// allowance is invalidated. That redecision waits for the original future reset until
/// another accepted allowance wakes the edited goal through the real quota waker.
#[test]
fn a_rejected_usage_resume_rechecks_the_edited_goal_after_quota_invalidation() {
    native_test(|| async {
        tokio::time::timeout(upgrade_handoff_tests::NATIVE_HANG, async {
            use codex_state::ThreadGoalStatus;
            let goals = Arc::new(GoalService::new());
            let h = Harness::new_with_goals(Some(Arc::clone(&goals))).await;
            h.fake.set_policy_key("upgrade", "false");
            h.fake.set_policy_key("limit_wait", "true");
            upgrade_handoff_tests::wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| !policy.upgrade && policy.limit_wait)
            })
            .await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let db = h.test.codex.state_db().expect("native goal database");
            let original = db
                .thread_goals()
                .replace_thread_goal(h.rt.id, "the original quota task", ThreadGoalStatus::UsageLimited, None)
                .await
                .expect("original limited goal");
            let at = clock();
            let reset = at + Duration::from_secs(24 * 60 * 60);
            h.rt.with_state(|state| {
                state.observe_turn_end(
                    decide::TurnEnd {
                        at,
                        by_autopilot: true,
                        by_person: false,
                        text: None,
                        earlier: None,
                        questions: false,
                        worked: Duration::from_secs(600),
                        wall: Some(Wall::Usage {
                            limit_class: LimitClass::Ordinary,
                            reset_at: Some(reset),
                            reset_label: None,
                        }),
                        aborted: false,
                        ran_tools: false,
                        restored: false,
                    },
                    &h.autopilot.timing,
                )
            });
            let before = h.rt.state();
            let policy = h.autopilot.sup.policy().expect("proven policy");
            let ordinary = decide::decide(&before, &policy, &h.autopilot.timing, clock(), h.rt.jitter);
            assert!(matches!(ordinary.act, Act::ResumeLimit { .. }));
            assert!(ordinary.not_before.is_some_and(|deadline| deadline >= reset));
            assert!(lock(&h.rt.human_goal_recovery).is_none());
            let quota = h.shared.quota();
            let identity = QuotaIdentity {
                account: "fixture-account".into(),
                user: Some("fixture-user".into()),
                owner: 0,
            };
            let generation = quota.select_owner(&identity.account, identity.user.as_deref(), identity.owner);
            let wall = quota.wall_for(identity.account.clone(), "codex".into(), at);
            *lock(&h.autopilot.quota_test_identity) = Some(identity.clone());
            *lock(&h.rt.quota_wall) = Some(QuotaWall {
                wall: wall.clone(),
                identity: identity.clone(),
            });
            quota.start_read(1, clock());
            quota.accept(1, &identity.account, vec!["codex".into()], clock());
            assert!(h.autopilot.quota_restored(&h.rt, &wall, clock()));
            let restored = decide::decide_with_usage(
                &before,
                &policy,
                &h.autopilot.timing,
                clock(),
                h.rt.jitter,
                true,
            );
            assert!(matches!(restored.act, Act::ResumeLimit { .. }));
            assert!(restored.not_before.is_some_and(|deadline| deadline <= clock()));

            let (entered, prepared) = tokio::sync::oneshot::channel();
            let (release, blocked) = tokio::sync::oneshot::channel();
            *lock(&h.rt.before_submit) = Some((entered, blocked));
            let pilot = Arc::clone(&h.autopilot);
            let runtime = Arc::clone(&h.rt);
            let mut evaluating = tokio::spawn(async move { pilot.evaluate(runtime).await });
            tokio::select! {
                ready = prepared => ready.expect("original goal preparation"),
                result = &mut evaluating => panic!("evaluation ended before preparation: {result:?}"),
            }
            let edit = goals
                .set_thread_goal(
                    &db,
                    GoalSetRequest {
                        thread_id: h.rt.id,
                        objective: GoalObjectiveUpdate::Set("the revised quota task"),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::UsageLimited),
                        token_budget: GoalTokenBudgetUpdate::Keep,
                        max_goal_token_budget: None,
                    },
                    async { Ok(()) },
                )
                .await
                .expect("real GoalService objective edit");
            edit.apply_runtime_effects(&goals).await;
            let edited = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
            assert_eq!(edited.goal_id, original.goal_id);
            assert_ne!(edited.objective, original.objective);
            assert_eq!(edited.status, ThreadGoalStatus::UsageLimited);
            let (changed, rejected) = tokio::sync::oneshot::channel();
            let (recheck, blocked_recheck) = tokio::sync::oneshot::channel();
            *lock(&h.rt.before_changed_goal_recheck) = Some((changed, blocked_recheck));
            release.send(()).expect("allow the stale goal preparation");
            tokio::select! {
                ready = rejected => ready.expect("actual NotNow reached its healthy goal recheck"),
                result = &mut evaluating => panic!("evaluation ended before rejected-goal recheck: {result:?}"),
            }
            let timer_before = h.rt.timer_seq.load(Ordering::SeqCst);
            let accepted_before = h.rt.accepted_turns.load(Ordering::SeqCst);
            assert!(!h.rt.timer_pending());
            assert_eq!(h.rt.state().point_at(), before.point_at());
            assert_eq!(h.rt.state().wall.as_ref().expect("unspent wall").attempts, 0);
            assert!(h.autopilot.quota_restored(&h.rt, &wall, clock()));
            quota.invalidate();
            assert_eq!(quota.generation(), generation);
            assert_eq!(h.autopilot.quota_identity(), Some(identity.clone()));
            assert!(!h.autopilot.quota_restored(&h.rt, &wall, clock()));
            let mut next = Box::pin(h.rt.hold_eval_when_free());
            std::future::poll_fn(|cx| {
                assert!(next.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            recheck.send(()).expect("release the rejected goal's recheck");
            let next = next.await;
            evaluating.await.expect("finish the rejected preparation");
            assert_eq!(h.rt.timer_seq.load(Ordering::SeqCst), timer_before + 1);
            assert!(h.fake.acts().is_empty());
            assert!(received_responses_requests(&h._server).await.is_empty());

            // Signal after the real immediate timer acquires evaluation, then
            // inspect its completed future-reset decision under the same permit.
            let (started, fresh) = tokio::sync::oneshot::channel();
            *lock(&h.rt.before_park) = Some(Box::new(move || {
                started.send(()).expect("observe the immediate timer's fresh evaluation");
            }));
            drop(next);
            fresh.await.expect("the rejected goal receives its fresh decision");
            let fresh = h.rt.hold_eval_when_free().await;
            let owed = h.rt.owed_timer().expect("the original usage reset remains future");
            assert_eq!(owed.rule, words::RULE_LIMIT_RESUME);
            assert!(owed.not_before >= reset);
            assert_eq!(db.thread_goals().get_thread_goal(h.rt.id).await.unwrap(), Some(edited.clone()));
            assert_eq!(h.rt.state().point_at(), before.point_at());
            assert_eq!(h.rt.state().wall.as_ref().expect("unspent wall").attempts, 0);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted_before);
            assert!(!h.autopilot.quota_restored(&h.rt, &wall, clock()));
            assert!(h.fake.acts().is_empty());
            assert!(received_responses_requests(&h._server).await.is_empty());

            h.autopilot.watching.store(false, Ordering::SeqCst);
            h.autopilot.watch_claims();
            quota.start_read(2, clock());
            quota.accept(2, &identity.account, vec!["codex".into()], clock());
            assert!(h.autopilot.quota_restored(&h.rt, &wall, clock()));
            assert!(clock() < reset);
            drop(fresh);
            upgrade_handoff_tests::wait_for_supervisor(|| {
                h.fake.acts().contains(&words::RULE_LIMIT_RESUME.into())
            })
            .await;
            loop {
                let requests = received_responses_requests(&h._server).await;
                if !requests.is_empty() {
                    assert_eq!(requests.len(), 1);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let resumed = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
            assert_eq!(resumed.goal_id, edited.goal_id);
            assert_eq!(resumed.objective, edited.objective);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted_before + 1);
            Autopilot::cancel_timer(&h.rt).await;
            h.test.codex.shutdown_and_wait().await.expect("native Core shutdown");
        })
        .await
        .expect("edited goal quota recheck hang detector");
    });
}

#[test]
fn a_due_usage_resume_respects_native_goal_ownership_before_its_initial_read() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            use codex_state::ThreadGoalStatus;
            for status in [
                Some(ThreadGoalStatus::Paused),
                Some(ThreadGoalStatus::Blocked),
                Some(ThreadGoalStatus::BudgetLimited),
                Some(ThreadGoalStatus::Active),
                Some(ThreadGoalStatus::Complete),
                None,
            ] {
                let goals = Arc::new(GoalService::new());
                let h = Harness::new_with_goals(Some(goals)).await;
                h.fake.set_policy_key("limit_wait", "true");
                upgrade_handoff_tests::wait_for_supervisor(|| {
                    h.autopilot
                        .sup
                        .policy()
                        .is_some_and(|policy| policy.limit_wait)
                })
                .await;
                h.immediate_point().await;
                h.hold_model_response().await;
                let db = h.test.codex.state_db().expect("goal database");
                if let Some(status) = status {
                    db.thread_goals()
                        .replace_thread_goal(h.rt.id, "the person's native goal", status, None)
                        .await
                        .expect("native goal");
                }
                let at = clock() - Duration::from_secs(3600);
                h.rt.with_state(|state| {
                    state.observe_turn_end(
                        decide::TurnEnd {
                            at,
                            by_autopilot: true,
                            by_person: false,
                            text: None,
                            earlier: None,
                            questions: false,
                            worked: Duration::from_secs(600),
                            wall: Some(decide::Wall::Usage {
                                limit_class: LimitClass::Ordinary,
                                reset_at: Some(at),
                                reset_label: None,
                            }),
                            aborted: false,
                            ran_tools: false,
                            restored: false,
                        },
                        &h.autopilot.timing,
                    )
                });
                Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
                if matches!(status, None | Some(ThreadGoalStatus::Complete)) {
                    assert!(
                        h.shared.turn_running_on(h.rt.key()),
                        "ordinary task continuation remains available: {status:?}"
                    );
                    assert_eq!(h.fake.acts(), vec![words::RULE_LIMIT_RESUME.to_string()]);
                } else {
                    assert!(
                        !h.shared.turn_running_on(h.rt.key()),
                        "the native goal retains its pace: {status:?}"
                    );
                    assert!(
                        h.fake.acts().is_empty(),
                        "no fallback input past {status:?}"
                    );
                    assert!(!h.rt.timer_pending());
                    assert_eq!(
                        h.rt.state().wall.as_ref().expect("unspent wall").attempts,
                        0
                    );
                    assert_eq!(
                        db.thread_goals()
                            .get_thread_goal(h.rt.id)
                            .await
                            .expect("unchanged goal")
                            .map(|goal| goal.status),
                        status
                    );
                }
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            }
        })
        .await
        .expect("native goal ownership hang detector");
    });
}

/// THE GOAL A TURN ERROR BLOCKED AT A WALL IS THE WALL'S STOP (`wall_goal_after`; owner
/// 2026-10-28: "must keep continuing and making progress with retries"; FOREVER CRUISE and
/// FOREVER LAND): bound where the walled turn began with the goal active, ended on a wall
/// codex-alab retries, unaborted, with no goal tool called; carried over the autopilot's own
/// compaction turn; resumed only while it stands as that wall left it (`same_wall_goal`).
/// NEGATIVE CONTROLS: a goal blocked before the turn, a goal tool's turn (the model blocks a
/// goal at a person's request), a person's stop while the turn ran, an aborted turn, a usage
/// limit's, a lost login's and a fatal wall's stops, and a turn with no wall bind
/// nothing; a paused, replaced, edited or spent goal, or one a person stopped since, is no
/// longer the bound one.
#[test]
fn a_goal_a_turn_error_blocked_at_a_retried_wall_is_that_walls_stop() {
    use codex_state::ThreadGoalStatus;
    let at = chrono::Utc::now();
    let goal = |status| codex_state::ThreadGoal {
        thread_id: ThreadId::new(),
        goal_id: "goal-1".to_string(),
        objective: "finish the original task".to_string(),
        status,
        token_budget: Some(1_000),
        tokens_used: 10,
        time_used_seconds: 0,
        created_at: at,
        updated_at: at,
    };
    let active = goal(ThreadGoalStatus::Active);
    let bound = codex_alab_core::session::WallGoal {
        goal_id: active.goal_id.clone(),
        objective: active.objective.clone(),
        token_budget: active.token_budget,
        chronology_fence_generation: None,
        turn: "turn-walled".to_string(),
        person_stops: 0,
    };
    let turn = |at_start: Option<codex_state::ThreadGoal>, tool: bool| TurnGoal {
        active_at_start: at_start,
        chronology_generation: Some(None),
        goal_tool: tool,
        person_stops: (0, 0),
        turn: "turn-walled".to_string(),
    };
    let server = Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    let retried = [
        server.clone(),
        Wall::Network {
            what: "the connection was refused".to_string(),
        },
        Wall::Overlong {
            what: "max_output_tokens".to_string(),
        },
        Wall::Context { auto_compact: true },
        Wall::Policy,
    ];
    for wall in &retried {
        assert_eq!(
            wall_goal_after(
                None,
                false,
                Some(wall),
                false,
                turn(Some(active.clone()), false)
            ),
            Some(bound.clone()),
            "{wall:?}"
        );
    }
    let generation =
        codex_alab_core::goal_chronology_fence::FenceGeneration::from_hex(&"11".repeat(32))
            .unwrap();
    assert_eq!(
        wall_goal_after(
            None,
            false,
            Some(&server),
            false,
            TurnGoal {
                chronology_generation: Some(Some(generation)),
                ..turn(Some(active.clone()), false)
            },
        )
        .unwrap()
        .chronology_fence_generation,
        Some(generation),
    );
    assert!(
        wall_goal_after(
            None,
            false,
            Some(&server),
            false,
            TurnGoal {
                chronology_generation: None,
                ..turn(Some(active.clone()), false)
            },
        )
        .is_none()
    );
    // The autopilot's own compaction carries the binding, whatever its turn met.
    assert_eq!(
        wall_goal_after(Some(bound.clone()), true, None, false, turn(None, false)),
        Some(bound.clone())
    );
    // NEGATIVE CONTROLS: binds nothing.
    let none = |carried, wall: Option<&Wall>, aborted, at_start, tool| {
        assert_eq!(
            wall_goal_after(carried, false, wall, aborted, turn(at_start, tool)),
            None,
            "{wall:?} aborted={aborted} tool={tool}"
        );
    };
    none(
        None,
        Some(&server),
        false,
        Some(goal(ThreadGoalStatus::Blocked)),
        false,
    );
    none(None, Some(&server), false, None, false);
    none(None, Some(&server), false, Some(active.clone()), true);
    none(None, Some(&server), true, Some(active.clone()), false);
    for wall in [
        Wall::Usage {
            limit_class: LimitClass::Ordinary,
            reset_at: None,
            reset_label: None,
        },
        Wall::Entitlement,
        Wall::Login,
        Wall::Fatal {
            why: "an invalid request".to_string(),
        },
    ] {
        none(None, Some(&wall), false, Some(active.clone()), false);
    }
    none(
        Some(bound.clone()),
        None,
        false,
        Some(active.clone()),
        false,
    );
    // A PERSON'S STOP WHILE THE TURN RAN (the app-server's `thread/goal/set` with a `user`
    // origin, counted through `goal_stops`; the forever-persist lane, item c) binds nothing: the goal the
    // turn error met was the person's to have stopped, and upstream's `turn-error` stop leaves
    // a goal no longer active as it is. One before the turn began is no stop of this turn's:
    // the goal is bound with the count the turn began with.
    let stops = |at_start, now| TurnGoal {
        person_stops: (at_start, now),
        ..turn(Some(active.clone()), false)
    };
    assert_eq!(
        wall_goal_after(None, false, Some(&server), false, stops(2, 3)),
        None
    );
    assert_eq!(
        wall_goal_after(None, false, Some(&server), false, stops(3, 3)),
        Some(codex_alab_core::session::WallGoal {
            person_stops: 3,
            ..bound.clone()
        })
    );
    // Resumed only while it stands as the wall left it.
    let blocked = goal(ThreadGoalStatus::Blocked);
    assert!(same_wall_goal(&bound, &blocked, 0));
    // A person's stop since the binding (after the turn's end, before the try): the goal is
    // the person's, never the wall's to resume.
    assert!(!same_wall_goal(&bound, &blocked, 1));
    for moved in [
        goal(ThreadGoalStatus::Paused),
        goal(ThreadGoalStatus::Active),
        goal(ThreadGoalStatus::BudgetLimited),
        codex_state::ThreadGoal {
            goal_id: "goal-2".to_string(),
            ..blocked.clone()
        },
        codex_state::ThreadGoal {
            objective: "another task".to_string(),
            ..blocked.clone()
        },
        codex_state::ThreadGoal {
            token_budget: Some(2_000),
            ..blocked.clone()
        },
        codex_state::ThreadGoal {
            tokens_used: 1_000,
            ..blocked
        },
    ] {
        assert!(!same_wall_goal(&bound, &moved, 0), "{moved:?}");
    }
}

/// A FULL CONTEXT'S COMPACTION GOES PAST THE GOAL A TURN ERROR BLOCKED AT IT (CONTRACT §5 goals
/// row; review of the forever-cruise lane, finding 4, whose mutant put the goal check back in
/// front of the compaction and passed every test): the goal bound to the context wall
/// ([`wall_goal_after`]) is that wall's stop, so the compaction is admitted and submitted past it
/// (its continuation resumes the goal), and the goal stays as the wall left it. NEGATIVE
/// CONTROL: the same blocked goal bound to no wall (blocked before the turn, by the model or by
/// a person) owns the pace: nothing is compacted, and the person is told.
#[test]
fn a_full_contexts_compaction_goes_past_the_goal_its_turn_error_blocked() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for case in [WallCompaction::Bound, WallCompaction::Unbound] {
                wall_compaction_case(case).await;
            }
        })
        .await
        .expect("compaction past a wall's goal hang detector");
    });
}

#[test]
fn a_busy_goal_chronology_rechecks_the_same_wall_compaction() {
    native_test(|| async {
        tokio::time::timeout(
            Duration::from_secs(60),
            wall_compaction_case(WallCompaction::Contended),
        )
        .await
        .expect("wall compaction chronology contention hang detector");
    });
}

enum WallCompaction {
    Unbound,
    Bound,
    Contended,
}

async fn wall_compaction_case(case: WallCompaction) {
    use codex_state::ThreadGoalStatus;
    let h = Harness::new_with_goals(Some(Arc::new(GoalService::new()))).await;
    h.fake.set_policy_key("upgrade", "false");
    h.fake.set_policy_key("compact_on_context_wall", "true");
    upgrade_handoff_tests::wait_for_supervisor(|| {
        h.autopilot
            .sup
            .policy()
            .is_some_and(|policy| !policy.upgrade && policy.compact_on_context_wall)
    })
    .await;
    h.immediate_point().await;
    h.hold_model_response().await;
    let db = h.test.codex.state_db().expect("goal database");
    let active = db
        .thread_goals()
        .replace_thread_goal(
            h.rt.id,
            "finish the original task",
            ThreadGoalStatus::Active,
            None,
        )
        .await
        .expect("the active goal");
    // Upstream's `turn-error` stop at the full context.
    let blocked = db
        .thread_goals()
        .update_thread_goal(
            h.rt.id,
            codex_state::GoalUpdate {
                objective: None,
                status: Some(ThreadGoalStatus::Blocked),
                token_budget: None,
                expected_goal_id: Some(active.goal_id.clone()),
            },
        )
        .await
        .expect("the turn error's stop")
        .expect("the same goal");
    let bound = !matches!(case, WallCompaction::Unbound);
    if bound {
        *lock(&h.rt.wall_goal) = Some(wall_goal_of(h.shared, &active));
    }
    h.rt.with_state(|state| {
        state.observe_turn_end(
            TurnEnd {
                at: clock(),
                by_autopilot: true,
                by_person: false,
                text: None,
                earlier: None,
                questions: false,
                worked: Duration::from_secs(600),
                wall: Some(Wall::Context { auto_compact: true }),
                aborted: false,
                ran_tools: false,
                restored: false,
            },
            &h.autopilot.timing,
        )
    });
    if matches!(case, WallCompaction::Contended) {
        let lock_chronology = || {
            codex_alab_core::goal_chronology_fence::lock_current(
                h.test.codex_home_path(),
                h.rt.key(),
            )
            .expect("hold the journal without changing its generation")
        };
        let check_retry = || {
            assert!(h.rt.timer_pending(), "an unread goal owes a recheck");
            assert!(!h.rt.state().compacting);
            assert!(h.fake.acts().is_empty());
            assert!(
                !h.fake.attention().contains_key("codex-alab"),
                "contention does not turn into goal attention: {:?}",
                h.fake.attention(),
            );
        };

        let journal = lock_chronology();
        Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
        check_retry();
        drop(journal);
        h.immediate_point().await;

        // This pass reaches final admission after the wall binding was read.
        let (entered, entry) = tokio::sync::oneshot::channel();
        let (release, blocked_at_submit) = tokio::sync::oneshot::channel();
        *lock(&h.rt.before_submit) = Some((entered, blocked_at_submit));
        let mut evaluating = Box::pin(Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)));
        tokio::select! {
            result = entry => result.expect("reached final compaction admission"),
            () = &mut evaluating => panic!("evaluation did not reach compaction admission"),
        }
        let journal = lock_chronology();
        release.send(()).expect("resume final compaction admission");
        evaluating.await;
        check_retry();
        assert_eq!(
            db.thread_goals().get_thread_goal(h.rt.id).await.unwrap(),
            Some(blocked.clone()),
        );
        drop(journal);
        h.immediate_point().await;
    }

    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    let compactions = h
        .fake
        .acts()
        .iter()
        .filter(|act| *act == words::RULE_COMPACT)
        .count();
    assert_eq!(
        compactions,
        usize::from(bound),
        "bound={bound}: {:?}",
        h.autopilot.sup.journal()
    );
    assert_eq!(h.rt.state().compacting, bound, "bound={bound}");
    assert_eq!(
        h.fake
            .attention()
            .get("codex-alab")
            .is_some_and(|text| text.starts_with("goal: the goal is blocked")),
        !bound,
        "bound={bound}: {:?}",
        h.fake.attention()
    );
    assert_eq!(
        db.thread_goals()
            .get_thread_goal(h.rt.id)
            .await
            .expect("read the goal"),
        Some(blocked),
        "bound={bound}: the compaction leaves the goal as the wall left it"
    );
    h.test
        .codex
        .shutdown_and_wait()
        .await
        .expect("native shutdown");
}

async fn wall_goal_resume_chronology_case(settled: bool) {
    tokio::time::timeout(Duration::from_secs(60), async {
        use codex_state::ThreadGoalStatus;
        let h = Harness::new_with_goals(Some(Arc::new(GoalService::new()))).await;
        h.fake.set_policy_key("upgrade", "false");
        h.fake.set_status("autopilot", "land");
        h.fake.set_policy_key("autopilot", "land");
        upgrade_handoff_tests::wait_for_supervisor(|| {
            h.autopilot
                .sup
                .policy()
                .is_some_and(|policy| !policy.upgrade && policy.autopilot == decide::Mode::Land)
        })
        .await;
        h.immediate_point().await;
        h.hold_model_response().await;
        let db = h.test.codex.state_db().expect("goal database");
        let active = db
            .thread_goals()
            .replace_thread_goal(
                h.rt.id,
                "finish the original task",
                ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("the active goal");
        let blocked = db
            .thread_goals()
            .update_thread_goal(
                h.rt.id,
                codex_state::GoalUpdate {
                    objective: None,
                    status: Some(ThreadGoalStatus::Blocked),
                    token_budget: None,
                    expected_goal_id: Some(active.goal_id.clone()),
                },
            )
            .await
            .expect("the turn error's stop")
            .expect("the same goal");
        *lock(&h.rt.wall_goal) = Some(wall_goal_of(h.shared, &active));
        let at = clock() - Duration::from_secs(3600);
        h.rt.with_state(|state| {
            state.observe_mode(decide::Mode::Land);
            state.observe_turn_end(
                TurnEnd {
                    at,
                    by_autopilot: true,
                    by_person: false,
                    text: None,
                    earlier: None,
                    questions: false,
                    worked: Duration::from_secs(600),
                    wall: Some(Wall::Server {
                        what: "an API error (500)".to_string(),
                        cut_off: false,
                        retry_at: None,
                    }),
                    aborted: false,
                    ran_tools: false,
                    restored: false,
                },
                &h.autopilot.timing,
            )
        });

        let (entered, mut entry) = tokio::sync::oneshot::channel();
        let (release, blocked_at_submit) = tokio::sync::oneshot::channel();
        *lock(&h.rt.before_submit) = Some((entered, blocked_at_submit));
        let mut evaluating = Box::pin(Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)));
        std::future::poll_fn(|cx| {
            assert!(evaluating.as_mut().poll(cx).is_pending());
            match entry.try_recv() {
                Ok(()) => Poll::Ready(()),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => Poll::Pending,
                Err(error) => panic!("wall resume preparation: {error}"),
            }
        })
        .await;
        let generation = codex_alab_core::goal_chronology_fence::arm(
            h.test.codex_home_path(),
            &h.rt.id.to_string(),
        )
        .expect("arm unresolved host chronology after the preparatory read");
        if settled {
            assert!(
                codex_alab_core::goal_chronology_fence::clear_if_current(
                    h.test.codex_home_path(),
                    &h.rt.id.to_string(),
                    generation,
                )
                .expect("settle the foreign host chronology generation")
            );
        }
        release.send(()).expect("release the prepared wall resume");
        evaluating.await;
        let evaluation = h.rt.hold_eval_when_free().await;

        assert_eq!(
            db.thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .expect("read the fenced goal"),
            Some(blocked),
            "a later host chronology generation must keep the wall's goal blocked"
        );
        assert!(!h.shared.turn_running_on(h.rt.key()));
        assert!(h.fake.acts().is_empty());
        assert_eq!(
            codex_alab_core::goal_chronology_fence::read_active(
                h.test.codex_home_path(),
                &h.rt.id.to_string(),
            )
            .expect("read the retained chronology fence"),
            (!settled).then_some(generation)
        );
        assert_eq!(
            codex_alab_core::goal_chronology_fence::read_current(
                h.test.codex_home_path(),
                &h.rt.id.to_string(),
            )
            .expect("read the durable chronology serial"),
            Some(generation)
        );
        codex_alab_core::goal_chronology_fence::clear(
            h.test.codex_home_path(),
            &h.rt.id.to_string(),
        )
        .expect("clear the test chronology fence");
        drop(evaluation);
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .expect("native shutdown");
    })
    .await
    .expect("wall chronology recheck hang detector");
}

#[test]
fn a_wall_goal_resume_rechecks_unresolved_host_chronology_under_the_native_permit() {
    native_test(|| wall_goal_resume_chronology_case(false));
}

#[test]
fn a_wall_goal_resume_rechecks_settled_host_chronology_under_the_native_permit() {
    native_test(|| wall_goal_resume_chronology_case(true));
}

/// THE WALL'S GOAL RESUMED IS BOOKED AS THE WALL'S OWN TRY (review of the forever-cruise lane,
/// finding 3, whose mutant booked it as a usage resume and passed every test): under Land, a
/// server wall's due try whose goal a turn error blocked resumes the goal through `GoalService`
/// and books the try it is ([`decide::ThreadState::wall_goal_resumed`]): the retry's rule the
/// last, one try on the wall's ladder, and Land's episode as it was (no words went, so no
/// instruction was given). NEGATIVE CONTROL: the goal was resumed (active again, no words typed).
#[test]
fn a_wall_goals_resume_is_booked_as_the_walls_own_try() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            use codex_state::ThreadGoalStatus;
            let h = Harness::new_with_goals(Some(Arc::new(GoalService::new()))).await;
            h.fake.set_policy_key("upgrade", "false");
            h.fake.set_status("autopilot", "land");
            h.fake.set_policy_key("autopilot", "land");
            upgrade_handoff_tests::wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| !policy.upgrade && policy.autopilot == decide::Mode::Land)
            })
            .await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let db = h.test.codex.state_db().expect("goal database");
            let active = db
                .thread_goals()
                .replace_thread_goal(
                    h.rt.id,
                    "finish the original task",
                    ThreadGoalStatus::Active,
                    None,
                )
                .await
                .expect("the active goal");
            db.thread_goals()
                .update_thread_goal(
                    h.rt.id,
                    codex_state::GoalUpdate {
                        objective: None,
                        status: Some(ThreadGoalStatus::Blocked),
                        token_budget: None,
                        expected_goal_id: Some(active.goal_id.clone()),
                    },
                )
                .await
                .expect("the turn error's stop")
                .expect("the same goal");
            *lock(&h.rt.wall_goal) = Some(wall_goal_of(h.shared, &active));
            // The goal's turn met a server's wall an hour ago: its first rung is past.
            let at = clock() - Duration::from_secs(3600);
            h.rt.with_state(|state| {
                state.observe_mode(decide::Mode::Land);
                state.observe_turn_end(
                    TurnEnd {
                        at,
                        by_autopilot: true,
                        by_person: false,
                        text: None,
                        earlier: None,
                        questions: false,
                        worked: Duration::from_secs(600),
                        wall: Some(Wall::Server {
                            what: "an API error (500)".to_string(),
                            cut_off: false,
                            retry_at: None,
                        }),
                        aborted: false,
                        ran_tools: false,
                        restored: false,
                    },
                    &h.autopilot.timing,
                )
            });
            assert!(!h.rt.state().land.typed);
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            let state = h.rt.state();
            assert_eq!(
                state.last_rule,
                Some(words::RULE_API_RETRY),
                "{:?}",
                h.autopilot.sup.journal()
            );
            assert_eq!(state.wall.as_ref().map(|wall| wall.attempts), Some(1));
            assert!(!state.land.typed, "no words went: no instruction was given");
            assert!(!state.landing);
            assert_eq!(
                db.thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("read the goal")
                    .map(|goal| (goal.goal_id, goal.status)),
                Some((active.goal_id, ThreadGoalStatus::Active)),
                "the goal was resumed"
            );
            assert!(
                h.fake.acts().iter().all(|act| act == words::RULE_API_RETRY),
                "{:?}",
                h.fake.acts()
            );
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("wall goal booking hang detector");
    });
}

/// A cybersecurity refusal blocks the goal, not the goal's independent future: the same goal is
/// active again, and its native continuation carries the original objective and the existing
/// instruction to abandon the refused action. The one-continuation boundary is already spent.
#[test]
fn a_policy_wall_resumes_its_goal_with_the_policy_boundary() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            use codex_state::ThreadGoalStatus;
            let h = Harness::new_with_goals(Some(Arc::new(GoalService::new()))).await;
            h.fake.set_policy_key("upgrade", "false");
            upgrade_handoff_tests::wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| !policy.upgrade)
            })
            .await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let db = h.test.codex.state_db().expect("goal database");
            let active = db
                .thread_goals()
                .replace_thread_goal(
                    h.rt.id,
                    "finish the original task",
                    ThreadGoalStatus::Active,
                    None,
                )
                .await
                .expect("the active goal");
            db.thread_goals()
                .update_thread_goal(
                    h.rt.id,
                    codex_state::GoalUpdate {
                        objective: None,
                        status: Some(ThreadGoalStatus::Blocked),
                        token_budget: None,
                        expected_goal_id: Some(active.goal_id.clone()),
                    },
                )
                .await
                .expect("the policy refusal's stop")
                .expect("the same goal");
            *lock(&h.rt.wall_goal) = Some(wall_goal_of(h.shared, &active));
            let at = clock() - Duration::from_secs(3600);
            h.rt.with_state(|state| {
                state.observe_turn_end(
                    TurnEnd {
                        at,
                        by_autopilot: true,
                        by_person: false,
                        text: None,
                        earlier: None,
                        questions: false,
                        worked: Duration::from_secs(600),
                        wall: Some(Wall::Policy),
                        aborted: false,
                        ran_tools: false,
                        restored: false,
                    },
                    &h.autopilot.timing,
                )
            });
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;

            let resumed = db
                .thread_goals()
                .get_thread_goal(h.rt.id)
                .await
                .expect("read the resumed goal")
                .expect("the goal remains");
            assert_eq!(resumed.goal_id, active.goal_id);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            let state = h.rt.state();
            assert!(state.policy_continued);
            assert_eq!(state.last_rule, Some(words::RULE_POLICY_CONTINUE));
            assert!(
                h.fake
                    .acts()
                    .iter()
                    .any(|act| act == words::RULE_POLICY_CONTINUE),
                "{:?}",
                h.fake.acts()
            );
            let requests = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if let Some(requests) = h._server.received_requests().await
                        && !requests.is_empty()
                    {
                        break requests;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("the native continuation request");
            let continuation = requests
                .last()
                .expect("a request")
                .body_json::<serde_json::Value>()
                .expect("continuation JSON")
                .to_string();
            assert!(
                continuation.contains("abandon the refused action"),
                "policy exclusion missing from native goal continuation: {continuation}"
            );
            assert!(
                continuation.contains("finish the original task"),
                "original objective missing from native goal continuation: {continuation}"
            );
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("policy goal resume hang detector");
    });
}

/// A policy continuation whose first native admission is declined remains the goal runtime's
/// custody: the ordinary idle retry carries its exclusion. A replacement goal revokes that
/// custody instead of inheriting a refusal from the old task.
#[test]
fn a_policy_wall_survives_generic_goal_admission_retries_but_not_a_goal_change() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            use codex_protocol::config_types::CollaborationMode;
            use codex_protocol::config_types::ModeKind;
            use codex_protocol::config_types::Settings;
            use codex_protocol::protocol::ThreadSettingsOverrides;
            use codex_state::ThreadGoalStatus;

            for replacement in [false, true] {
                let goals = Arc::new(GoalService::new());
                let h = Harness::new_with_goals(Some(Arc::clone(&goals))).await;
                h.hold_model_response().await;
                let db = h.test.codex.state_db().expect("goal database");
                db.thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "finish the original task",
                        ThreadGoalStatus::Blocked,
                        None,
                    )
                    .await
                    .expect("the policy-blocked goal");
                let outcome = goals
                    .set_thread_goal(
                        &db,
                        GoalSetRequest {
                            thread_id: h.rt.id,
                            objective: GoalObjectiveUpdate::Keep,
                            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                            token_budget: GoalTokenBudgetUpdate::Keep,
                            max_goal_token_budget: None,
                        },
                        async { Ok(()) },
                    )
                    .await
                    .expect("reactivate the policy-blocked goal");
                let mode = |mode| ThreadSettingsOverrides {
                    collaboration_mode: Some(CollaborationMode {
                        mode,
                        settings: Settings {
                            model: h.test.session_configured.model.clone(),
                            reasoning_effort: None,
                            developer_instructions: None,
                        },
                    }),
                    ..Default::default()
                };
                core_test_support::submit_thread_settings(&h.test.codex, mode(ModeKind::Plan))
                    .await
                    .expect("decline the first automatic admission in Plan mode");
                outcome
                    .apply_runtime_effects_with_policy(
                        &goals,
                        Some(words::POLICY_CONTINUE.to_string()),
                    )
                    .await;
                assert!(
                    h._server
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .is_empty(),
                    "Plan mode declines the policy continuation before a provider request"
                );

                if replacement {
                    let replacement = goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id: h.rt.id,
                                objective: GoalObjectiveUpdate::Set("the replacement task"),
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async { Ok(()) },
                        )
                        .await
                        .expect("replace the goal before the generic retry");
                    replacement.apply_runtime_effects(&goals).await;
                }
                assert!(
                    h._server
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .is_empty(),
                    "Plan mode also defers the replacement until the real idle retry"
                );
                core_test_support::submit_thread_settings(&h.test.codex, mode(ModeKind::Default))
                    .await
                    .expect("permit the generic idle retry");
                // Exact mutation effects are consumed once. A generic retry enters
                // the real idle lifecycle instead of replaying the old outcome.
                h.retry_idle_goal().await;

                let requests = tokio::time::timeout(Duration::from_secs(10), async {
                    loop {
                        if let Some(requests) = h._server.received_requests().await
                            && !requests.is_empty()
                        {
                            break requests;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                })
                .await
                .expect("the generic native goal retry");
                let continuation = requests
                    .last()
                    .expect("a request")
                    .body_json::<serde_json::Value>()
                    .expect("continuation JSON")
                    .to_string();
                assert_eq!(
                    continuation.contains("abandon the refused action"),
                    !replacement,
                    "replacement={replacement}: policy custody was wrong: {continuation}"
                );
                assert!(
                    continuation.contains(if replacement {
                        "the replacement task"
                    } else {
                        "finish the original task"
                    }),
                    "replacement={replacement}: wrong objective: {continuation}"
                );
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            }
        })
        .await
        .expect("generic policy admission retry hang detector");
    });
}

/// A GOAL A PERSON STOPPED IS NEVER A WALL'S TO RESUME (the forever-persist lane, item c; owner
/// 2026-10-19: "a person's hand wins"): a server wall's due try whose goal a turn error blocked
/// resumes it through `GoalService`, but where a person stopped the thread's goal since it was
/// bound (the app-server's `thread/goal/set` with a `user` origin, from a client that blocks or
/// pauses it: the TUI only pauses, which is no block either; the `ALAB(autopilot)` hook counting
/// it through `goal_stops`, whose own test drives the request) the blocked goal is theirs: nothing is resumed or typed, the goal stays
/// as they left it, and they are told. NEGATIVE CONTROL: with no person's stop the try resumes
/// the goal.
#[test]
fn a_goal_a_person_stopped_is_never_a_walls_to_resume() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            use codex_state::ThreadGoalStatus;
            for person in [false, true] {
                let h = Harness::new_with_goals(Some(Arc::new(GoalService::new()))).await;
                h.fake.set_policy_key("upgrade", "false");
                h.fake.set_status("autopilot", "cruise");
                h.fake.set_policy_key("autopilot", "cruise");
                upgrade_handoff_tests::wait_for_supervisor(|| {
                    h.autopilot.sup.policy().is_some_and(|policy| {
                        !policy.upgrade && policy.autopilot == decide::Mode::Cruise
                    })
                })
                .await;
                h.immediate_point().await;
                h.hold_model_response().await;
                let db = h.test.codex.state_db().expect("goal database");
                let active = db
                    .thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "finish the original task",
                        ThreadGoalStatus::Active,
                        None,
                    )
                    .await
                    .expect("the active goal");
                // The goal is blocked (upstream's `turn-error` stop, or the person's own set).
                let blocked = db
                    .thread_goals()
                    .update_thread_goal(
                        h.rt.id,
                        codex_state::GoalUpdate {
                            objective: None,
                            status: Some(ThreadGoalStatus::Blocked),
                            token_budget: None,
                            expected_goal_id: Some(active.goal_id.clone()),
                        },
                    )
                    .await
                    .expect("the goal's stop")
                    .expect("the same goal");
                *lock(&h.rt.wall_goal) = Some(wall_goal_of(h.shared, &active));
                if person {
                    // What the app-server's hook notes as the person's set passes
                    // (`goal_stops::person_set`, this process's shared state).
                    h.shared.person_goal_set(&h.rt.key, true);
                }
                let at = clock() - Duration::from_secs(3600);
                h.rt.with_state(|state| {
                    state.observe_turn_end(
                        TurnEnd {
                            at,
                            by_autopilot: true,
                            by_person: false,
                            text: None,
                            earlier: None,
                            questions: false,
                            worked: Duration::from_secs(600),
                            wall: Some(Wall::Server {
                                what: "an API error (500)".to_string(),
                                cut_off: false,
                                retry_at: None,
                            }),
                            aborted: false,
                            ran_tools: false,
                            restored: false,
                        },
                        &h.autopilot.timing,
                    )
                });
                Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
                let goal = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("read the goal");
                if person {
                    assert_eq!(goal, Some(blocked), "the person's stop stands");
                    assert!(
                        h.fake.acts().is_empty(),
                        "nothing typed: {:?}",
                        h.fake.acts()
                    );
                    assert!(
                        h.fake
                            .attention()
                            .get("codex-alab")
                            .is_some_and(|text| text.starts_with("goal: the goal is blocked")),
                        "{:?}",
                        h.fake.attention()
                    );
                } else {
                    assert_eq!(
                        goal.map(|goal| (goal.goal_id, goal.status)),
                        Some((active.goal_id, ThreadGoalStatus::Active)),
                        "the wall's goal was resumed: {:?}",
                        h.autopilot.sup.journal()
                    );
                }
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            }
        })
        .await
        .expect("person-stopped goal hang detector");
    });
}

/// The walled turn the bindings these tests make name.
const WALLED_TURN: &str = "turn-walled";

/// A history that holds the walled turn's start, then `after`.
fn walled_history(after: Vec<codex_history::RolloutItem>) -> Vec<codex_history::RolloutItem> {
    let started: codex_protocol::protocol::EventMsg = serde_json::from_value(serde_json::json!({
        "type": "task_started",
        "turn_id": WALLED_TURN,
        "model_context_window": 258_400,
        "collaboration_mode_kind": "default",
        "turn_attribution": {"turn_id": WALLED_TURN, "turn_trigger": "alab_autopilot"},
    }))
    .expect("native start");
    let mut items = vec![codex_history::RolloutItem::EventMsg(started)];
    items.extend(after);
    items
}

/// A person's goal set as upstream records it in the thread's history (its own renderer).
fn person_goal_set(update: codex_core::context::UserGoalUpdate) -> codex_history::RolloutItem {
    codex_history::RolloutItem::ResponseItem(
        codex_core::context::ContextualUserFragment::into(update).into(),
    )
}

/// An image of this tab's session: its own shared state and autopilot, after a re-exec
/// (`relaunch` false) or aterm's relaunch from a dead record (`relaunch` true) whose pace for
/// thread `key` was `carried`, as the record writer's start takes it up.
fn image_after(
    relaunch: bool,
    key: &str,
    carried: Option<codex_alab_core::session::Pace>,
) -> (&'static codex_alab_core::session::Shared, Arc<Autopilot>) {
    use codex_alab_core::session::ATERM_SESSION_ENV;
    use codex_alab_core::session::Inherited;
    use codex_alab_core::session::Launch;
    use codex_alab_core::session::Predecessor;
    use codex_alab_core::session::now_ms;
    codex_alab_core::activate();
    let shared: &'static codex_alab_core::session::Shared =
        Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    let autopilot = Autopilot::new(
        Supervisor::new(crate::SupervisorOptions {
            shared,
            endpoint: Arc::new(|| None),
            client: codex_alab_aterm::Options::default(),
        }),
        None,
        None,
        decide::Timing::default(),
        |_| true,
    );
    if relaunch {
        assert!(shared.set_launch(Launch {
            vars: vec![(ATERM_SESSION_ENV.into(), "s-ab12".into())],
            pid: std::process::id(),
            started_at_ms: now_ms(),
            ..Launch::default()
        }));
        shared.note_predecessor(Predecessor {
            pid: 41,
            session_id: key.to_string(),
            updated_at_ms: now_ms() - 60_000,
            pace: carried.clone(),
            aterm_session: Some("s-ab12".to_string()),
            ..Predecessor::default()
        });
    } else {
        assert!(shared.set_inherited(Inherited {
            exec_generation: 1,
            ..Inherited::default()
        }));
    }
    if let Some(pace) = carried {
        shared.set_pace(key, pace);
    }
    (shared, autopilot)
}

/// What a record carries of `pace`: a person's stops are never written, so a carried goal is
/// bound at none (alab-tui's `record::Pace`, whose round trip its own tests pin).
fn as_recorded(
    pace: Option<codex_alab_core::session::Pace>,
) -> Option<codex_alab_core::session::Pace> {
    pace.map(|pace| codex_alab_core::session::Pace {
        wall_goal: pace
            .wall_goal
            .map(|goal| codex_alab_core::session::WallGoal {
                person_stops: 0,
                ..goal
            }),
        ..pace
    })
}

/// The goal the bindings these tests make stand for, blocked in the store of thread `id`.
fn blocked_goal(id: ThreadId) -> codex_state::ThreadGoal {
    let at = chrono::Utc::now();
    codex_state::ThreadGoal {
        thread_id: id,
        goal_id: "goal-1".to_string(),
        objective: "finish the original task".to_string(),
        status: codex_state::ThreadGoalStatus::Blocked,
        token_budget: Some(1_000),
        tokens_used: 10,
        time_used_seconds: 0,
        created_at: at,
        updated_at: at,
    }
}

/// THE GOAL A TURN ERROR BLOCKED AT THE POINT'S WALL OUTLIVES A RE-EXEC AND A RELAUNCH (the
/// forever-persist lane, item a): the record's pace carries it, and the image after a live
/// upgrade, or aterm's relaunch from a dead record of this tab, binds it again
/// ([`Autopilot::seed_pace`]) where the thread's history holds its walled turn's start and no
/// person's goal stop since, so the wall's try resumes it there as the image before would have
/// (the try itself: `a_wall_goals_resume_is_booked_as_the_walls_own_try`), and writes it back.
/// NEGATIVE CONTROLS: a pace that carries none binds none (the try then meets the blocked goal
/// as a stop it may not lift, and parks); a history that holds a person's goal stop since the
/// walled turn began (one the dead process's record never wrote: review of the forever-persist
/// lane, F2), names no start of that turn, or is unread binds none; and a person's stop in the
/// image after makes the bound goal no wall's.
#[test]
fn a_walls_goal_outlives_a_re_exec_and_a_relaunch() {
    use codex_alab_core::session::Pace;
    use codex_alab_core::session::WallGoal;
    use codex_core::context::UserGoalUpdate;
    use codex_protocol::protocol::ThreadGoalStatus;
    let carried_goal = WallGoal {
        goal_id: "goal-1".to_string(),
        objective: "finish the original task".to_string(),
        token_budget: Some(1_000),
        chronology_fence_generation: None,
        turn: WALLED_TURN.to_string(),
        person_stops: 0,
    };
    let clean = walled_history(Vec::new());
    let stopped = walled_history(vec![person_goal_set(UserGoalUpdate::Set {
        objective: None,
        status: Some(ThreadGoalStatus::Blocked),
    })]);
    let elsewhere = {
        let mut items = walled_history(Vec::new());
        if let codex_history::RolloutItem::EventMsg(
            codex_protocol::protocol::EventMsg::TurnStarted(started),
        ) = &mut items[0]
        {
            started.turn_id = "another-turn".to_string();
        }
        items
    };
    for relaunch in [false, true] {
        for (history, carried, binds) in [
            (Some(&clean), true, true),
            (Some(&clean), false, false),
            (Some(&stopped), true, false),
            (Some(&elsewhere), true, false),
            (None, true, false),
        ] {
            let case = format!(
                "relaunch={relaunch} carried={carried} history={:?}",
                history.map(Vec::len)
            );
            let id = ThreadId::new();
            let rt = Arc::new(ThreadRt::new(id, None, false, Duration::ZERO));
            let pace = Pace {
                short_streak: 1,
                wall_goal: carried.then(|| carried_goal.clone()),
                ..Pace::default()
            };
            let (shared, autopilot) = image_after(relaunch, &rt.key, Some(pace));
            autopilot.keep_thread(&rt);
            autopilot.seed_pace(&rt, history.map(Vec::as_slice));
            let bound = lock(&rt.wall_goal).clone();
            if !binds {
                assert_eq!(bound, None, "{case}");
                continue;
            }
            let bound = bound.unwrap_or_else(|| panic!("{case}: bound again"));
            assert_eq!(bound, carried_goal, "{case}");
            let blocked = blocked_goal(id);
            assert!(
                same_wall_goal(&bound, &blocked, shared.person_goal_stops(&rt.key)),
                "{case}: the wall's to resume"
            );
            autopilot.sync_pace(&rt);
            assert_eq!(
                shared.pace(&rt.key).and_then(|pace| pace.wall_goal),
                Some(carried_goal.clone()),
                "{case}: written back"
            );
            // NEGATIVE CONTROL: a person's stop in this image.
            shared.person_goal_set(&rt.key, true);
            assert!(
                !same_wall_goal(&bound, &blocked, shared.person_goal_stops(&rt.key)),
                "{case}: the person's"
            );
        }
    }
}

/// A PERSON'S STOP BEFORE A RESTART KEEPS THE WALL'S GOAL FROM THE IMAGE AFTER (review of the
/// forever-persist lane, F2): a goal a turn error blocked at the wall, bound and written to the
/// pace, and then stopped by a person (the app-server's `thread/goal/set` with a `user` origin),
/// left the pace the record carries as it was, and the image after a re-exec or a relaunch, or a
/// `/resume` back in the same process, re-counted it from the person's stops it had then, so it
/// resumed the goal the person had stopped. Now a person's stop takes the goal out of the pace
/// at once, a pace written with the old binding keeps none, and the seed keeps the binding's own
/// count. So no image binds it: after a re-exec, after a relaunch, at a `/resume` back, and where
/// the person's stop comes in the image after before the thread is resumed there. NEGATIVE
/// CONTROL: with no person's stop each path binds the goal again, and the wall's try may resume
/// it.
#[test]
fn a_persons_stop_before_a_restart_keeps_the_walls_goal_from_the_image_after() {
    use codex_alab_core::session::WallGoal;
    let history = walled_history(Vec::new());
    for path in [
        "re-exec",
        "relaunch",
        "resume back",
        "stop in the image after",
    ] {
        for person in [false, true] {
            let case = format!("{path} person={person}");
            let id = ThreadId::new();
            // The image before: the wall's goal is bound and written to the pace.
            let (before, old) = image_after(false, &id.to_string(), None);
            let rt = Arc::new(ThreadRt::new(id, None, false, Duration::ZERO));
            old.keep_thread(&rt);
            lock(&rt.state).seed_pace(1, None);
            *lock(&rt.wall_goal) = Some(WallGoal {
                goal_id: "goal-1".to_string(),
                objective: "finish the original task".to_string(),
                token_budget: Some(1_000),
                chronology_fence_generation: None,
                turn: WALLED_TURN.to_string(),
                person_stops: before.person_goal_stops(&rt.key),
            });
            old.sync_pace(&rt);
            assert!(
                before
                    .pace(&rt.key)
                    .and_then(|pace| pace.wall_goal)
                    .is_some(),
                "{case}: written"
            );
            if person && path != "stop in the image after" {
                before.person_goal_set(&rt.key, true);
                // A later write of the image before (any decision's) writes none back.
                old.sync_pace(&rt);
                assert_eq!(
                    before.pace(&rt.key).and_then(|pace| pace.wall_goal),
                    None,
                    "{case}: out of the pace"
                );
            }
            let carried = before.pace(&rt.key);
            let (shared, autopilot) = match path {
                "resume back" => (before, Arc::clone(&old)),
                "relaunch" => image_after(true, &rt.key, as_recorded(carried)),
                _ => image_after(false, &rt.key, as_recorded(carried)),
            };
            if person && path == "stop in the image after" {
                shared.person_goal_set(&rt.key, true);
            }
            let resumed = Arc::new(ThreadRt::new(id, None, false, Duration::ZERO));
            autopilot.keep_thread(&resumed);
            autopilot.seed_pace(&resumed, Some(&history));
            let bound = lock(&resumed.wall_goal).clone();
            let resumes = bound.as_ref().is_some_and(|bound| {
                same_wall_goal(bound, &blocked_goal(id), shared.person_goal_stops(&rt.key))
            });
            assert_eq!(resumes, !person, "{case}: {bound:?}");
        }
    }
}

/// The binding [`wall_goal_after`] makes of `goal`, active as its walled turn began, in a process
/// whose shared state is `shared`.
pub(super) fn wall_goal_of(
    shared: &codex_alab_core::session::Shared,
    goal: &codex_state::ThreadGoal,
) -> codex_alab_core::session::WallGoal {
    codex_alab_core::session::WallGoal {
        goal_id: goal.goal_id.clone(),
        objective: goal.objective.clone(),
        token_budget: goal.token_budget,
        chronology_fence_generation: None,
        turn: WALLED_TURN.to_string(),
        person_stops: shared.person_goal_stops(&goal.thread_id.to_string()),
    }
}
