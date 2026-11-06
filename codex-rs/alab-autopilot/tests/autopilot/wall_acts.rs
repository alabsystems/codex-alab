// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A wall's try end to end: its pace reaches the record with the act, before its turn runs
//! (B3), and a quota wall's try goes on under a goal core stopped at that wall, its need said
//! loud (FOREVER CRUISE: NEEDS YOU is loud and never blocks).

use super::*;

use codex_alab_core::session::Pace;

/// THE TRY'S PACE IS IN THE RECORD WHEN ITS TURN STARTS (B3; aterm's books hold an act as it
/// is typed): the turn start's write-through, which every crash in the try's turn leaves as
/// the record aterm's relaunch reads, names the try (the ladder's attempt, its answer owed).
/// Synced at the evaluation's end, after the submit's await, the try reached the record only
/// after its turn began, so a crash in that turn handed the relaunch the rung before it.
/// NEGATIVE CONTROL: the person's turn before the wall starts with no try on the record.
#[test]
fn a_walls_try_is_in_the_record_when_its_turn_starts() {
    big_stack(|| async {
        let h = harness_timed(
            Mode::Current,
            vec![
                wiremock::ResponseTemplate::new(500).set_body_json(serde_json::json!({
                    "error": {"message": "Internal server error."}
                })),
                responses::sse_response(reply("r2", "Retried; stage 2 is next.")),
            ],
            &[("human_grace_s", "0"), ("continue", "false")],
            &[],
            Some(0),
            None,
            Timing {
                min_work: Duration::ZERO,
                retry_backoff: vec![Duration::ZERO],
                ..Timing::default()
            },
        )
        .await;
        let starts: Arc<std::sync::Mutex<Vec<Option<Pace>>>> = Arc::default();
        let seen = Arc::clone(&starts);
        let shared = h.shared;
        let thread = h.thread.clone();
        // The turn start's write-through (the record a crash in the turn leaves).
        assert!(shared.set_flush(Box::new(move || {
            if shared.turn_running_on(&thread) {
                seen.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(shared.pace(&thread));
            }
            true
        })));
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until("the retry's turn ends", || {
            h.mock.requests().len() == 2 && journal_has(&h, "nothing (continue is off)")
        })
        .await;
        assert!(h.fake.acts().iter().any(|a| a == "api-retry@v1"));
        let starts = starts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let attempts = |pace: &Option<Pace>| {
            pace.as_ref()
                .and_then(|p| p.wall.as_ref())
                .map(|w| (w.attempts, w.answer_owed))
        };
        assert_eq!(starts.len(), 2, "two turn starts: {starts:?}");
        // NEGATIVE CONTROL: the person's turn begins before any wall.
        assert_eq!(attempts(&starts[0]), None, "{starts:?}");
        assert_eq!(
            attempts(&starts[1]),
            Some((1, true)),
            "the try's turn starts with the try on the record: {starts:?}"
        );
    });
}

/// A QUOTA WALL'S TRY GOES ON UNDER A GOAL CORE STOPPED AT THAT WALL (B3; FOREVER CRUISE): core
/// stops an active goal as usage-limited at a quota wall (`insufficient_quota`, which it
/// classes a usage limit), and codex-alab took that stop for a person's, so the try it decided
/// typed nothing and nothing else ever came: a silent stall for ever. The try now resumes the
/// goal, as the usage window's resume does, and the need only a person lifts is said loud at
/// once. With no goal the try is typed as a turn, the need said all the same. NEGATIVE
/// CONTROL: a goal that is blocked is still a person's, and nothing goes.
///
/// THE NEED IS NEVER HIDDEN (parity-5, N): an `api:` reason codex-alab raised before stands
/// all along, and the quota's need goes up as a need of the `owner=needs` entry, in aterm's
/// frame, beside it. Raised as codex-alab's `limit:` attention reason (RED), it sat behind
/// the `api:` one, the only one of codex-alab's reasons aterm shows, and the `needs` entry a
/// manager's `await task` wakes on never carried it.
///
/// THE GOAL'S TRY CLIMBS THE LADDER (review of CX-B, CXR-1), under the goal extension the
/// binary installs ([`super::late_goal`]): the resumed goal's own turn starts inside the
/// resume, its record names the try (B3: the rung and its owed answer, written before the
/// turn begins), and when that turn meets the quota again (the extension stops the goal
/// usage-limited once more) the next try waits for the ladder's next rung. Booked only after
/// the resume returned, the try was never counted: the turn start's record named no try, and
/// the next wall resumed the goal again at once, rung after rung at the first.
#[test]
fn a_quota_try_resumes_a_usage_limited_goal_and_says_the_need() {
    use codex_state::ThreadGoalStatus;
    for goal in [
        Some(ThreadGoalStatus::UsageLimited),
        None,
        Some(ThreadGoalStatus::Blocked),
    ] {
        big_stack(move || async move {
            let quota = || {
                responses::sse_response(responses::sse_failed(
                    "quota",
                    "insufficient_quota",
                    "The project has no remaining quota.",
                ))
            };
            let mut replies = vec![quota()];
            match goal {
                // The resumed goal's own turn, which meets the quota again.
                Some(ThreadGoalStatus::UsageLimited) => replies.push(quota()),
                None => replies.push(responses::sse_response(reply("r2", "Back at it."))),
                _ => {}
            }
            let h = harness_goals(
                Mode::Current,
                replies,
                &[
                    ("human_grace_s", "0"),
                    ("resume_limits", "true"),
                    ("continue", "false"),
                ],
                &[],
                Some(0),
                None,
                Timing {
                    min_work: Duration::ZERO,
                    entitlement_backoff: vec![Duration::ZERO, Duration::from_secs(3600)],
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
            if let Some(status) = goal {
                // Core's stop at the wall, before the walled turn (the person's turn starts
                // no goal; the extension stops only an active one).
                db.thread_goals()
                    .replace_thread_goal(id, "finish the original task", status, None)
                    .await
                    .unwrap_or_else(|err| panic!("the goal's stop: {err}"));
            }
            // The turn start's write-through (the record a crash in the turn leaves).
            let starts: Arc<std::sync::Mutex<Vec<Option<Pace>>>> = Arc::default();
            let seen = Arc::clone(&starts);
            let shared = h.shared;
            let thread = h.thread.clone();
            assert!(shared.set_flush(Box::new(move || {
                if shared.turn_running_on(&thread) {
                    seen.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(shared.pace(&thread));
                }
                true
            })));
            assert!(
                h.fake
                    .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                    .await
            );
            // A reason codex-alab left to a person before the wall, which stands meanwhile
            // (not under the blocked goal, whose own reason is the control's to read).
            const API: &str = "api: Codex reported a server error; retry_api_errors is off";
            if goal != Some(ThreadGoalStatus::Blocked) {
                wait_until("codex-alab acts", || h.autopilot.supervisor().acting()).await;
                h.autopilot
                    .supervisor()
                    .attention_for(&h.thread, "api", Some(API.to_string()))
                    .await;
                assert!(
                    h.fake
                        .wait_for(
                            |f| f.attention().get("codex-alab").map(String::as_str) == Some(API)
                        )
                        .await
                );
            }
            h.test
                .codex
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "Continue the original task.".into(),
                    text_elements: Vec::new(),
                }]))
                .await
                .unwrap_or_else(|err| panic!("start the walled turn: {err}"));
            let need = format!(
                "Codex needs you: {} \u{2014} carrying on",
                codex_alab_autopilot::decide::QUOTA_NEED
            );
            let raised = format!("meta set attention owner=needs kind=ask {need}");
            let said = || h.fake.requests().iter().any(|r| r.contains(&raised));
            // The need beside the reason that stands, never behind it.
            let beside = || {
                let shown = h.fake.attention();
                shown.get("needs") == Some(&need)
                    && shown.get("codex-alab").map(String::as_str) == Some(API)
            };
            let status = || async {
                db.thread_goals()
                    .get_thread_goal(id)
                    .await
                    .unwrap_or_else(|err| panic!("read goal: {err}"))
                    .map(|g| g.status)
            };
            match goal {
                Some(ThreadGoalStatus::UsageLimited) => {
                    let lines = || h.autopilot.supervisor().journal();
                    let resumes = || {
                        lines()
                            .iter()
                            .filter(|l| l.contains("resumed the goal on the quota's ladder"))
                            .count()
                    };
                    let waits = || {
                        lines().iter().any(|l| {
                            l.contains(": wait ")
                                && l.contains("the account's quota stops the model")
                        })
                    };
                    wait_until(
                        "the goal's own turn meets the quota again, and its next try is decided",
                        || h.mock.requests().len() == 2 && (waits() || resumes() >= 2),
                    )
                    .await;
                    assert_eq!(
                        resumes(),
                        1,
                        "the wall the goal's turn met waits for the ladder's next rung"
                    );
                    assert!(waits(), "{:?}", lines());
                    assert!(said(), "{:?}", h.fake.requests());
                    assert!(beside(), "{:?}", h.fake.attention());
                    assert!(h.fake.acts().iter().any(|a| a == "usage-resume@v1"));
                    assert!(
                        !lines().iter().any(|l| l.contains("typed \"")),
                        "no words typed past the goal: {:?}",
                        lines()
                    );
                    // The extension stopped the goal at the wall its own turn met.
                    assert_eq!(status().await, Some(ThreadGoalStatus::UsageLimited));
                    let starts = starts
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    let rung = |pace: &Option<Pace>| {
                        pace.as_ref()
                            .and_then(|p| p.wall.as_ref())
                            .map(|w| (w.attempts, w.answer_owed))
                    };
                    assert!(starts.len() >= 2, "{starts:?}");
                    // NEGATIVE CONTROL: the person's turn begins before any wall.
                    assert_eq!(rung(&starts[0]), None, "{starts:?}");
                    assert_eq!(
                        rung(&starts[1]),
                        Some((1, true)),
                        "the goal's turn starts with the try on the record: {starts:?}"
                    );
                    // Answered by the goal's turn, the ladder stands on its next rung.
                    assert_eq!(rung(&h.shared.pace(&h.thread)), Some((1, false)));
                }
                None => {
                    wait_until("the try is typed and answered", || {
                        h.mock.requests().len() == 2 && journal_has(&h, "nothing (continue is off)")
                    })
                    .await;
                    assert!(said(), "{:?}", h.fake.requests());
                    assert!(h.fake.acts().iter().any(|a| a == "usage-resume@v1"));
                    // A turn that ends with no wall takes the need down, and the reason too.
                    assert!(
                        h.fake
                            .wait_for(|f| {
                                let shown = f.attention();
                                !shown.contains_key("needs") && !shown.contains_key("codex-alab")
                            })
                            .await,
                        "{:?}",
                        h.fake.attention()
                    );
                }
                _ => {
                    // NEGATIVE CONTROL.
                    assert!(
                        h.fake
                            .wait_for(|f| f
                                .attention()
                                .get("codex-alab")
                                .is_some_and(|t| t.starts_with("goal: the goal is blocked")))
                            .await,
                        "{:?}",
                        h.autopilot.supervisor().journal()
                    );
                    assert_eq!(status().await, Some(ThreadGoalStatus::Blocked));
                    assert!(!h.fake.acts().iter().any(|a| a == "usage-resume@v1"));
                    assert_eq!(h.mock.requests().len(), 1);
                }
            }
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .unwrap_or_else(|err| panic!("shutdown: {err}"));
        });
    }
}

/// A GOAL RESUME THE GOAL STORE REFUSED GIVES ITS TRY BACK (review of CX-B, CXR-1; parity-5,
/// G): a quota wall's try under a goal core stopped there is booked, its pace written through,
/// before the goal moves (in the instruction record `GoalService::set_thread_goal` runs under
/// the goal runtime's permit, past the resume's recheck), since the goal's turn can start inside
/// that call. A set that fails starts no turn, so the booking is given back: the record names
/// the rung as it stood, and the person is told the goal did not resume. The goal store fails
/// through the seam (`Autopilot::refuse_goal_set`: that record ends in an error past its booked
/// try, as a goal write that fails after it does, and the set returns it, the goal unchanged). Kept
/// booked (RED), the record named a try nothing answered (its rung and its answer owed), and
/// the next try waited the ladder's second rung for a try that never went. NEGATIVE CONTROL:
/// with the store answering, the same try resumes the goal and its turn's start names the try
/// ([`a_quota_try_resumes_a_usage_limited_goal_and_says_the_need`]).
#[test]
fn a_goal_resume_the_store_refused_gives_its_try_back() {
    use codex_state::ThreadGoalStatus;
    big_stack(|| async {
        let h = harness_goals(
            Mode::Current,
            vec![responses::sse_response(responses::sse_failed(
                "quota",
                "insufficient_quota",
                "The project has no remaining quota.",
            ))],
            &[
                ("human_grace_s", "0"),
                ("resume_limits", "true"),
                ("continue", "false"),
            ],
            &[],
            Some(0),
            None,
            Timing {
                min_work: Duration::ZERO,
                entitlement_backoff: vec![Duration::ZERO, Duration::from_secs(3600)],
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
                ThreadGoalStatus::UsageLimited,
                None,
            )
            .await
            .unwrap_or_else(|err| panic!("the goal's stop: {err}"));
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
        let told = codex_alab_autopilot::extension::goal_resume_failed(REFUSAL);
        assert!(
            h.fake
                .wait_for(|f| f.attention().get("codex-alab") == Some(&told))
                .await,
            "{:?}",
            h.autopilot.supervisor().journal()
        );
        assert!(
            journal_has(&h, &format!("goal resume failed: {REFUSAL}")),
            "{:?}",
            h.autopilot.supervisor().journal()
        );
        let rung = |pace: Option<Pace>| {
            pace.and_then(|p| p.wall)
                .map(|w| (w.attempts, w.answer_owed))
        };
        assert_eq!(
            rung(h.shared.pace(&h.thread)),
            Some((0, false)),
            "the refused resume's try is given back on the record"
        );
        let status = db
            .thread_goals()
            .get_thread_goal(id)
            .await
            .unwrap_or_else(|err| panic!("read goal: {err}"))
            .map(|g| g.status);
        assert_eq!(status, Some(ThreadGoalStatus::UsageLimited), "unchanged");
        assert!(!h.fake.acts().iter().any(|a| a == "usage-resume@v1"));
        assert_eq!(h.mock.requests().len(), 1, "no turn came of it");
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .unwrap_or_else(|err| panic!("shutdown: {err}"));
    });
}
