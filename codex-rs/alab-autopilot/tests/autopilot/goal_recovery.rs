// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native goal stops own the pace even when the failed request names a later retry, but for
//! the stop a turn error made at the wall itself, which the wall's try resumes.

use super::*;

/// A GOAL BLOCKED BEFORE THE TURN STAYS A PERSON'S: the walled turn began under no active goal,
/// so the stop is no wall's ([`a_goal_a_turn_error_blocked_resumes_at_the_walls_try`]'s
/// negative control); it is surfaced before any provider retry wait, nothing is typed past it.
#[test]
fn a_blocked_goal_is_surfaced_before_a_provider_retry_wait() {
    big_stack(|| async {
        let h = harness_with_request_retries(
            Mode::Current,
            vec![
                wiremock::ResponseTemplate::new(503)
                    .insert_header("Retry-After", "3600")
                    .set_body_json(serde_json::json!({"error": {
                        "code": "rate_limit_exceeded", "message": "Temporary failure."
                    }})),
            ],
            &[("human_grace_s", "0")],
            Some(0),
        )
        .await;
        let db = h
            .test
            .codex
            .state_db()
            .unwrap_or_else(|| panic!("goal database"));
        let id = h.test.session_configured.thread_id;
        db.thread_goals()
            .replace_thread_goal(
                id,
                "finish the original task",
                codex_state::ThreadGoalStatus::Blocked,
                None,
            )
            .await
            .unwrap_or_else(|err| panic!("the native goal's stop: {err}"));
        h.test
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Continue the original task.".into(),
                text_elements: Vec::new(),
            }]))
            .await
            .unwrap_or_else(|err| panic!("start the failed turn: {err}"));
        assert!(
            h.fake
                .wait_for(|f| f
                    .attention()
                    .get("codex-alab")
                    .is_some_and(|text| text.starts_with("goal: the goal is blocked")))
                .await,
            "the native goal stop needs attention now: {:?}",
            h.autopilot.supervisor().journal()
        );
        assert!(
            !h.autopilot
                .thread_rt(&h.thread)
                .unwrap_or_else(|| panic!("registered thread"))
                .timer_pending()
        );
        assert_eq!(h.mock.requests().len(), 1);
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(id)
                .await
                .unwrap_or_else(|err| panic!("read goal: {err}"))
                .unwrap_or_else(|| panic!("same goal"))
                .status,
            codex_state::ThreadGoalStatus::Blocked
        );
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .unwrap_or_else(|err| panic!("shutdown: {err}"));
    });
}

/// THE GOAL A TURN ERROR BLOCKED AT A WALL IS RESUMED AT THE WALL'S TRY, ON ITS LADDER (owner
/// 2026-10-28: codex-alab "must keep continuing and making progress with retries" through
/// outages; FOREVER CRUISE): upstream's goal extension blocks the active goal at the server
/// error that ends the turn (`turn-error`), and codex-alab took that stop for a person's, so the
/// wall's try was dropped at the goal check and the thread parked for good behind `goal: the
/// goal is blocked`. Now the try resumes the goal through `GoalService` (no words typed), its
/// own turn answers the try, and when it meets the wall again the next try waits for the
/// ladder's next rung. NEGATIVE CONTROL: a goal blocked before the turn stays a person's
/// ([`a_blocked_goal_is_surfaced_before_a_provider_retry_wait`]).
#[test]
fn a_goal_a_turn_error_blocked_resumes_at_the_walls_try() {
    big_stack(|| async {
        let server = || {
            wiremock::ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": {"message": "Internal server error."}
            }))
        };
        let h = harness_goals(
            Mode::Current,
            // The person's turn, then the resumed goal's own turn, each meet the server error.
            vec![server(), server()],
            &[("human_grace_s", "0"), ("continue", "false")],
            &[],
            Some(0),
            None,
            Timing {
                min_work: Duration::ZERO,
                retry_backoff: vec![Duration::ZERO, Duration::from_secs(3600)],
                ..Timing::default()
            },
            |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new())),
        )
        .await;
        let db = h
            .test
            .codex
            .state_db()
            .unwrap_or_else(|| panic!("goal database"));
        let id = h.test.session_configured.thread_id;
        db.thread_goals()
            .replace_thread_goal(
                id,
                "finish the original task",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .unwrap_or_else(|err| panic!("the active goal: {err}"));
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Continue the original task.".into(),
                text_elements: Vec::new(),
            }]))
            .await
            .unwrap_or_else(|err| panic!("start the walled turn: {err}"));
        let rt = h
            .autopilot
            .thread_rt(&h.thread)
            .unwrap_or_else(|| panic!("registered thread"));
        let lines = || h.autopilot.supervisor().journal();
        let resumes = || {
            lines()
                .iter()
                .filter(|l| l.contains("resumed the goal a turn error blocked"))
                .count()
        };
        wait_until(
            "the goal's own turn meets the wall again, and its next try waits",
            || {
                h.mock.requests().len() == 2
                    && rt.timer_pending()
                    && rt
                        .owed_timer()
                        .is_some_and(|owed| owed.rule == "api-retry@v1")
            },
        )
        .await;
        assert_eq!(resumes(), 1, "{:?}", lines());
        assert!(
            !lines().iter().any(|l| l.contains("typed \"")),
            "no words typed past the goal: {:?}",
            lines()
        );
        assert!(h.fake.acts().iter().any(|a| a == "api-retry@v1"));
        assert!(
            !h.fake
                .attention()
                .get("codex-alab")
                .is_some_and(|text| text.starts_with("goal: the goal is blocked")),
            "{:?}",
            h.fake.attention()
        );
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(id)
                .await
                .unwrap_or_else(|err| panic!("read goal: {err}"))
                .unwrap_or_else(|| panic!("same goal"))
                .status,
            codex_state::ThreadGoalStatus::Blocked,
            "the wall stopped it again; its next try resumes it"
        );
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .unwrap_or_else(|err| panic!("shutdown: {err}"));
    });
}

/// THE GOAL A TURN ERROR BLOCKED SAYS ITS OWN STOP WHEN ITS RESUME FAILS (review of the
/// forever-cruise lane, finding 5): the store refuses the wall's try's resume
/// (`Autopilot::refuse_goal_set`), and the person is told the goal a turn error blocked did not
/// resume at the wall's try, never that a usage-limited goal missed a reset (no usage limit and
/// no reset stopped it); the goal stays as the wall left it. NEGATIVE CONTROL: a usage-limited
/// goal's refused resume says its own (`wall_acts.rs`'s
/// `a_goal_resume_the_store_refused_gives_its_try_back`).
#[test]
fn a_wall_goals_refused_resume_names_the_walls_stop() {
    big_stack(|| async {
        let h = harness_goals(
            Mode::Current,
            vec![
                wiremock::ResponseTemplate::new(500).set_body_json(serde_json::json!({
                    "error": {"message": "Internal server error."}
                })),
            ],
            &[("human_grace_s", "0"), ("continue", "false")],
            &[],
            Some(0),
            None,
            Timing {
                min_work: Duration::ZERO,
                retry_backoff: vec![Duration::ZERO, Duration::from_secs(3600)],
                ..Timing::default()
            },
            |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new())),
        )
        .await;
        let db = h
            .test
            .codex
            .state_db()
            .unwrap_or_else(|| panic!("goal database"));
        let id = h.test.session_configured.thread_id;
        db.thread_goals()
            .replace_thread_goal(
                id,
                "finish the original task",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .unwrap_or_else(|err| panic!("the active goal: {err}"));
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        const REFUSAL: &str = "the goal store is read-only";
        h.autopilot.refuse_goal_set(Some(REFUSAL.to_string()));
        h.test
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Continue the original task.".into(),
                text_elements: Vec::new(),
            }]))
            .await
            .unwrap_or_else(|err| panic!("start the walled turn: {err}"));
        let told = codex_alab_autopilot::extension::wall_goal_resume_failed(REFUSAL);
        assert!(
            h.fake
                .wait_for(|f| f.attention().get("codex-alab") == Some(&told))
                .await,
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        assert_ne!(
            told,
            codex_alab_autopilot::extension::goal_resume_failed(REFUSAL)
        );
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(id)
                .await
                .unwrap_or_else(|err| panic!("read goal: {err}"))
                .unwrap_or_else(|| panic!("same goal"))
                .status,
            codex_state::ThreadGoalStatus::Blocked,
            "the refused resume leaves the goal as the wall left it"
        );
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .unwrap_or_else(|err| panic!("shutdown: {err}"));
    });
}

#[test]
fn a_goal_paused_while_the_compact_act_writes_its_record_stops_the_compaction() {
    for paused in [true, false] {
        big_stack(move || async move {
            let mut replies = vec![responses::sse_failed(
                "context-full",
                "context_length_exceeded",
                "The context window is full.",
            )];
            if !paused {
                replies.push(reply("compacted", "A summary of the original task."));
            }
            let h = harness(
                Mode::Current,
                replies,
                &[("human_grace_s", "0"), ("continue", "false")],
            )
            .await;
            let db = h
                .test
                .codex
                .state_db()
                .unwrap_or_else(|| panic!("goal database"));
            let id = h.test.session_configured.thread_id;
            let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let written = Arc::clone(&changed);
            let shared = h.shared;
            let runtime = tokio::runtime::Handle::current();
            let goal_db = db.clone();
            // write_through runs this on a blocking worker, after the early goal check
            // and before the compaction is submitted. Change the goal at that exact seam.
            assert!(shared.set_flush(Box::new(move || {
                if shared.acting()
                    && !written.swap(true, std::sync::atomic::Ordering::SeqCst)
                    && paused
                {
                    runtime
                        .block_on(goal_db.thread_goals().replace_thread_goal(
                            id,
                            "finish the original task",
                            codex_state::ThreadGoalStatus::Paused,
                            None,
                        ))
                        .unwrap_or_else(|err| panic!("pause at the record write: {err}"));
                }
                true
            })));
            assert!(
                h.fake
                    .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                    .await
            );
            h.test
                .submit_turn("Keep the original task while recovering its context.")
                .await
                .unwrap_or_else(|err| panic!("context-limited turn: {err}"));
            if paused {
                wait_until("the late goal check leaves the paused goal alone", || {
                    journal_has(&h, "a goal stands (paused)")
                })
                .await;
                assert_eq!(h.mock.requests().len(), 1, "no compaction was submitted");
                assert!(h.fake.acts().is_empty());
                assert_eq!(
                    db.thread_goals()
                        .get_thread_goal(id)
                        .await
                        .unwrap_or_else(|err| panic!("read paused goal: {err}"))
                        .unwrap_or_else(|| panic!("same goal"))
                        .status,
                    codex_state::ThreadGoalStatus::Paused
                );
            } else {
                // Negative control: the same context wall compacts with no paused goal.
                wait_until("the compaction was submitted", || {
                    h.fake
                        .acts()
                        .iter()
                        .any(|act| act == codex_alab_autopilot::words::RULE_COMPACT)
                })
                .await;
                wait_for_event(&h.test.codex, |event| {
                    matches!(event, EventMsg::TurnComplete(_))
                })
                .await;
                assert_eq!(h.mock.requests().len(), 2);
            }
            assert!(changed.load(std::sync::atomic::Ordering::SeqCst));
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .unwrap_or_else(|err| panic!("shutdown: {err}"));
        });
    }
}
