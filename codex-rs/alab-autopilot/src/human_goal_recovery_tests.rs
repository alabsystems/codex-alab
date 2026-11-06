// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Successful human status replies recover stale usage stops through native goals.
//!
//! Harness::new_with_goals installs the real GoalExtension through LateGoal.
//! The positive case runs a real human input and mock model answer first.
//! Supplementary cases seed only the completed TurnEnd decision fixture.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use codex_alab_core::session::Frame;
use codex_state::ThreadGoalStatus;
use core_test_support::responses;

const HANG: Duration = Duration::from_secs(60);
const OBJECTIVE: &str = "Finish the original compiler integration and publish it";

async fn harness() -> (Harness, Arc<GoalService>) {
    let goals = Arc::new(GoalService::new());
    let h = Harness::new_with_goals(Some(Arc::clone(&goals))).await;
    h.fake.set_policy_key("upgrade", "false");
    h.fake.set_policy_key("autopilot", "cruise");
    h.fake.set_policy_key("limit_wait", "true");
    wait_for_supervisor(|| {
        h.autopilot.sup.policy().is_some_and(|policy| {
            !policy.upgrade
                && policy.autopilot == decide::Mode::Cruise
                && policy.limit_wait
                && policy.resume_limits
        })
    })
    .await;
    h.immediate_point().await;
    h.hold_model_response().await;
    (h, goals)
}

async fn goal(h: &Harness) -> codex_state::ThreadGoal {
    h.test
        .codex
        .state_db()
        .expect("real goal database")
        .thread_goals()
        .get_thread_goal(h.rt.id)
        .await
        .expect("read actual goal")
        .expect("goal remains")
}

async fn seed_goal(
    h: &Harness,
    status: ThreadGoalStatus,
    budget: Option<i64>,
) -> codex_state::ThreadGoal {
    h.test
        .codex
        .state_db()
        .expect("real goal database")
        .thread_goals()
        .replace_thread_goal(h.rt.id, OBJECTIVE, status, budget)
        .await
        .expect("persist original goal")
}

fn status_answer(at: Instant) -> TurnEnd {
    TurnEnd {
        at,
        by_autopilot: false,
        by_person: true,
        text: Some("The compiler integration is progressing. The remaining work is the executable caller boundary.".into()),
        earlier: None,
        questions: false,
        worked: Duration::from_secs(1),
        wall: None,
        aborted: false,
        ran_tools: false,
        restored: false,
    }
}

/// Fixture-only elapsed time. Do not reduce the production backoff or clear
/// the short-work streak: advancing just this point's timestamp makes its
/// existing normal continuation deadline due without a two-minute test sleep.
fn mature_point(h: &Harness) {
    let at = clock() - Duration::from_secs(3601);
    h.rt.with_state(|state| {
        state.point.as_mut().expect("completed status point").at = at;
    });
    if let Some((point, _)) = lock(&h.rt.human_goal_recovery).as_mut() {
        *point = at;
    }
}

async fn seed_status_point(h: &Harness, mut end: TurnEnd) {
    end.at = clock() - Duration::from_secs(3601);
    // Supplementary fixtures provide the same start-bound snapshot as the real
    // lifecycle exercised by the first test; failed ends must still be refused.
    *lock(&h.rt.human_goal_recovery) = Some((end.at, goal(h).await));
    h.rt.with_state(|state| {
        state.task = true;
        state.mode = Some(decide::Mode::Cruise);
        state.observe_turn_end(end, &h.autopilot.timing);
    });
    h.immediate_point().await;
}

async fn evaluate(h: &Harness) {
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
}

async fn assert_no_submission(h: &Harness, accepted: u64) {
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
    assert!(
        responses::received_responses_requests(&h._server)
            .await
            .is_empty()
    );
    assert!(!h.shared.turn_running_on(h.rt.key()));
}

async fn wait_requests(h: &Harness, count: usize) {
    // Native admission precedes provider I/O. Wait for the actual wire observation
    // rather than assuming TurnStarted means the HTTP request has already arrived.
    while responses::received_responses_requests(&h._server)
        .await
        .len()
        < count
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn assert_native_goal_started_once(h: &Harness, accepted_before: u64) {
    let started = core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnStarted(_)),
        HANG,
    )
    .await;
    let started = serde_json::to_value(started).expect("native start provenance");
    assert_eq!(
        started["turn_attribution"]["turn_trigger"], "goal",
        "GoalService must start a native goal continuation, not type a plain prompt"
    );
    wait_for_supervisor(|| h.shared.turn_running_on(h.rt.key())).await;
    // The model response is deliberately held. A second evaluation of this
    // now-active goal must not create a second native turn or consume a try.
    evaluate(h).await;
    assert_eq!(
        h.rt.accepted_turns.load(Ordering::SeqCst),
        accepted_before + 1
    );
    wait_requests(h, 1).await;
    let requests = responses::received_responses_requests(&h._server).await;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].body_contains_text(OBJECTIVE));
    assert!(
        h.fake
            .acts()
            .iter()
            .all(|rule| rule != words::RULE_CONTINUE)
    );
}

async fn finish(h: &Harness) {
    Autopilot::cancel_timer(&h.rt).await;
    h.test
        .codex
        .shutdown_and_wait()
        .await
        .expect("owned native shutdown");
}

#[test]
fn a_real_human_status_answer_recovers_the_same_cruise_goal_through_native_goal_start() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, _goals) = harness().await;
            let original = seed_goal(&h, ThreadGoalStatus::UsageLimited, None).await;
            // Keep automatic evaluation behind this actual human turn while
            // inspecting the native end it produces. This is not a fake end.
            let eval_hold = h.rt.hold_eval_when_free().await;
            let response = responses::sse(vec![
                responses::ev_response_created("human-status-answer"),
                responses::ev_assistant_message("human-status-text", "The compiler integration is progressing. The remaining work is the executable caller boundary."),
                responses::ev_completed("human-status-answer"),
            ]);
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .respond_with(wiremock::ResponseTemplate::new(200)
                    .set_body_raw(response, "text/event-stream"))
                .with_priority(1).up_to_n_times(1).mount(&h._server).await;
            h.test.codex.start_or_steer_turn(TurnInputRequest::user_input(vec![
                UserInput::Text { text: "What progress have you made?".into(), text_elements: vec![] },
            ])).await.expect("actual human question accepted");
            core_test_support::wait_for_event_with_timeout(
                &h.test.codex, |event| matches!(event, EventMsg::TurnComplete(_)), HANG,
            ).await;
            wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
            let ended = h.rt.state().point.expect("actual human status answer point");
            assert!(ended.by_person && !ended.by_autopilot);
            assert!(!ended.ran_tools && !ended.aborted && ended.wall.is_none());
            assert_eq!(goal(&h).await.goal_id, original.goal_id);
            assert_eq!(goal(&h).await.status, ThreadGoalStatus::UsageLimited);
            drop(eval_hold);
            h.immediate_point().await;
            evaluate(&h).await;
            assert_eq!(goal(&h).await.status, ThreadGoalStatus::UsageLimited,
                "a short status reply retains ordinary pacing before it is due");
            assert!(h.rt.owed_timer().is_some(), "the due continuation stays owed: {:?}", h.autopilot.sup.journal());
            assert_eq!(responses::received_responses_requests(&h._server).await.len(), 1);
            mature_point(&h);
            h.immediate_point().await;
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            evaluate(&h).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.token_budget, original.token_budget);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            let started = core_test_support::wait_for_event_with_timeout(
                &h.test.codex, |event| matches!(event, EventMsg::TurnStarted(_)), HANG,
            ).await;
            let started = serde_json::to_value(started).expect("goal turn provenance");
            assert_eq!(started["turn_attribution"]["turn_trigger"], "goal");
            wait_for_supervisor(|| h.shared.turn_running_on(h.rt.key())).await;
            evaluate(&h).await;
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            wait_requests(&h, 2).await;
            assert_eq!(responses::received_responses_requests(&h._server).await.len(), 2,
                "one human request plus exactly one native goal continuation");
            assert!(h.fake.acts().iter().all(|rule| rule != words::RULE_CONTINUE));
            finish(&h).await;
        }).await.expect("real human-status recovery hang detector");
    });
}

#[test]
fn a_status_answer_preserves_explicit_stops_and_an_exhausted_goal_budget() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            // Three native stop authorities, rather than a broad policy matrix.
            for (status, exhausted) in [
                (ThreadGoalStatus::Paused, false),
                (ThreadGoalStatus::Blocked, false),
                (ThreadGoalStatus::UsageLimited, true),
            ] {
                let (h, _goals) = harness().await;
                let original = seed_goal(&h, status, exhausted.then_some(10)).await;
                if exhausted {
                    h.test
                        .codex
                        .state_db()
                        .expect("goal database")
                        .thread_goals()
                        .account_thread_goal_usage(
                            h.rt.id,
                            0,
                            10,
                            codex_state::GoalAccountingMode::ActiveOrStopped,
                            Some(&original.goal_id),
                        )
                        .await
                        .expect("actual native budget consumption");
                }
                let before = goal(&h).await;
                seed_status_point(&h, status_answer(clock())).await;
                let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
                evaluate(&h).await;
                let after = goal(&h).await;
                assert_eq!(after.goal_id, before.goal_id);
                assert_eq!(after.objective, before.objective);
                assert_eq!(after.token_budget, before.token_budget);
                assert_eq!(after.tokens_used, before.tokens_used);
                if exhausted {
                    assert_ne!(after.status, ThreadGoalStatus::Active);
                } else {
                    assert_eq!(after, before, "a person's stop must remain authoritative");
                }
                assert_no_submission(&h, accepted).await;
                finish(&h).await;
            }
        })
        .await
        .expect("goal-stop and budget hang detector");
    });
}

#[test]
fn an_aborted_error_or_current_quota_wall_is_not_a_successful_human_recovery() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            for failure in ["aborted", "error", "quota"] {
                let (h, _goals) = harness().await;
                let original = seed_goal(&h, ThreadGoalStatus::UsageLimited, None).await;
                let mut end = status_answer(clock());
                match failure {
                    "aborted" => end.aborted = true,
                    "error" => {
                        end.wall = Some(Wall::Fatal {
                            why: "invalid model request".into(),
                        })
                    }
                    "quota" => {
                        end.wall = Some(Wall::Usage {
                            reset_at: Some(clock() + Duration::from_secs(3600)),
                            reset_label: Some("a future usage reset".into()),
                        })
                    }
                    _ => unreachable!(),
                }
                seed_status_point(&h, end).await;
                let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
                evaluate(&h).await;
                assert_eq!(
                    goal(&h).await,
                    original,
                    "failure is not recovery: {failure}"
                );
                assert_no_submission(&h, accepted).await;
                finish(&h).await;
            }
        })
        .await
        .expect("failed-status recovery hang detector");
    });
}

fn show_input(h: &Harness, draft: bool, queued: bool) {
    let view = h.shared.view();
    h.shared.observe(
        &Frame {
            primary_thread: view.primary_thread.as_deref(),
            displayed_thread: view.displayed_thread.as_deref(),
            rollout_path: view
                .primary()
                .and_then(|facts| facts.rollout_path.as_deref()),
            cwd: view.cwd.as_deref().expect("fixture cwd"),
            model: view.model.as_deref().expect("fixture model"),
            codex_home: view.codex_home.as_deref().expect("fixture home"),
            embedded: view.embedded,
            state: view.state,
            draft_empty: !draft,
            queued_input: queued,
            pending_user_turn: false,
            popup_open: false,
            editor_open: false,
        },
        now_ms(),
    );
}

#[test]
fn a_due_status_recovery_waits_for_draft_or_queued_input_and_resumes_after_release() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            for draft in [true, false] {
                let (h, _goals) = harness().await;
                let original = seed_goal(&h, ThreadGoalStatus::UsageLimited, None).await;
                seed_status_point(&h, status_answer(clock())).await;
                show_input(&h, draft, !draft);
                let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
                evaluate(&h).await;
                assert_eq!(goal(&h).await, original);
                assert_no_submission(&h, accepted).await;
                show_input(&h, false, false);
                h.immediate_point().await;
                evaluate(&h).await;
                assert_eq!(goal(&h).await.goal_id, original.goal_id);
                assert_eq!(goal(&h).await.status, ThreadGoalStatus::Active);
                assert_native_goal_started_once(&h, accepted).await;
                finish(&h).await;
            }
        })
        .await
        .expect("status recovery input-readiness hang detector");
    });
}

#[test]
fn a_prepared_status_recovery_cannot_overwrite_a_same_point_pause_or_new_goal_identity() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            for replacement in [false, true] {
                let (h, goals) = harness().await;
                let original = seed_goal(&h, ThreadGoalStatus::UsageLimited, None).await;
                seed_status_point(&h, status_answer(clock())).await;
                let point = h.rt.state().point_at();
                let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
                let (entered, entry) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((entered, blocked));
                let pilot = Arc::clone(&h.autopilot);
                let rt = Arc::clone(&h.rt);
                let evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
                entry
                    .await
                    .expect("actual resume reached its pre-mutation gate");
                let db = h.test.codex.state_db().expect("actual goal database");
                if replacement {
                    db.thread_goals()
                        .replace_thread_goal(
                            h.rt.id,
                            &original.objective,
                            ThreadGoalStatus::UsageLimited,
                            original.token_budget,
                        )
                        .await
                        .expect("new goal with the same words and limit");
                } else {
                    let stopped = goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id: h.rt.id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async { Ok(()) },
                        )
                        .await
                        .expect("person's native pause");
                    stopped.apply_runtime_effects(&goals).await;
                }
                let newer = goal(&h).await;
                assert_eq!(h.rt.state().point_at(), point, "no new turn hides the race");
                release
                    .send(())
                    .expect("resume rechecks under the real goal-state permit");
                evaluating.await.expect("prepared evaluation completes");
                assert_eq!(goal(&h).await, newer, "newer native goal state must win");
                assert_no_submission(&h, accepted).await;
                finish(&h).await;
            }
        })
        .await
        .expect("same-point goal-mutation hang detector");
    });
}
