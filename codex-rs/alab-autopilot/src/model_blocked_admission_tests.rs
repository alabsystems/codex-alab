// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A real model stop retains its original goal at every scheduler admission.

use super::*;
use crate::repo::RepoRead;
use crate::repo_tests::scratch_git;

const CLEAN: &str = "Nothing left: all merged and pushed to origin/main.";

#[test]
fn compact_admission_requires_the_current_model_stop_epoch() {
    native_test(|| async {
        for stale in [false, true] {
            tokio::time::timeout(HANG, async {
                let (h, goals) = harness().await;
                let evaluation = h.rt.hold_eval_when_free().await;
                h.fake.set_policy_key("compact_on_context_wall", "true");
                wait_for_supervisor(|| {
                    h.autopilot
                        .sup
                        .policy()
                        .is_some_and(|policy| policy.compact_on_context_wall)
                })
                .await;
                seed(&h, &goals).await;
                model_stop(&h, "blocked").await;
                h.immediate_point().await;
                h.rt.with_state(|state| {
                    state.point.as_mut().expect("native decision point").wall =
                        Some(Wall::Context { auto_compact: true });
                });
                let decision = decide::decide(
                    &h.rt.state(),
                    &h.autopilot.sup.policy().expect("compaction policy"),
                    &h.autopilot.timing,
                    clock(),
                    Duration::ZERO,
                );
                assert_eq!(decision.act, Act::Compact);

                let (entered, entry) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((entered, blocked));
                let pilot = Arc::clone(&h.autopilot);
                let rt = Arc::clone(&h.rt);
                drop(evaluation);
                let evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
                entry.await.expect("compaction reached final admission");
                if stale {
                    super::source::host_set(
                        &h,
                        &goals,
                        GoalObjectiveUpdate::Keep,
                        codex_protocol::protocol::ThreadGoalStatus::Paused,
                    )
                    .await;
                    super::source::host_set(
                        &h,
                        &goals,
                        GoalObjectiveUpdate::Keep,
                        codex_protocol::protocol::ThreadGoalStatus::Blocked,
                    )
                    .await;
                }
                release.send(()).expect("release compact admission");
                evaluating.await.expect("compaction evaluator settles");
                assert_eq!(
                    h.fake.acts().iter().any(|act| act == words::RULE_COMPACT),
                    !stale,
                    "stale={stale}: only the exact live model epoch admits compaction"
                );
                finish(&h).await;
            })
            .await
            .unwrap_or_else(|error| {
                panic!("stale={stale}: model-stop compact admission hung: {error}")
            });
        }
    });
}

async fn land_policy(h: &Harness) {
    h.fake.set_status("autopilot", "land");
    h.fake.set_policy_key("autopilot", "land");
    wait_for_supervisor(|| {
        h.autopilot
            .sup
            .policy()
            .is_some_and(|policy| policy.autopilot == decide::Mode::Land && !policy.upgrade)
    })
    .await;
}

fn clean_repository(h: &Harness) -> tempfile::TempDir {
    let remote = tempfile::tempdir().expect("owned local remote");
    let cwd = h.test.cwd.path();
    scratch_git(cwd, &["init", "-q", "."]);
    scratch_git(cwd, &["add", "."]);
    scratch_git(
        cwd,
        &["commit", "--allow-empty", "-q", "-m", "landed fixture"],
    );
    scratch_git(remote.path(), &["init", "-q", "--bare", "."]);
    let remote_path = remote.path().display().to_string();
    scratch_git(cwd, &["remote", "add", "origin", &remote_path]);
    scratch_git(cwd, &["push", "-q", "-u", "origin", "HEAD:main"]);
    assert!(scratch_git(cwd, &["status", "--porcelain"]).is_empty());
    remote
}

async fn land_reply(h: &Harness, status: &str) {
    let calls = [
        (
            codex_goal_extension::CREATE_GOAL_TOOL_NAME,
            serde_json::json!({"objective": OBJECTIVE, "token_budget": 1000}),
        ),
        (
            codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
            serde_json::json!({"status": status}),
        ),
    ];
    let mut replies = Vec::new();
    for (index, (name, arguments)) in calls.into_iter().enumerate() {
        let response = format!("land-goal-{status}-{index}");
        replies.push(responses::sse(vec![
            responses::ev_response_created(&response),
            responses::ev_function_call(
                &format!("land-call-{status}-{index}"),
                name,
                &arguments.to_string(),
            ),
            responses::ev_completed_with_tokens(&response, 10),
        ]));
    }
    replies.push(responses::sse(vec![
        responses::ev_response_created("land-clean-final"),
        responses::ev_assistant_message("land-clean-message", CLEAN),
        responses::ev_completed("land-clean-final"),
    ]));
    responses::mount_sse_sequence(&h._server, replies).await;
}

struct LandResult {
    goal: codex_state::ThreadGoal,
    landed: bool,
    acts: Vec<String>,
    requests: Vec<responses::ResponsesRequest>,
    original_id: String,
}

async fn land_yield(status: &str) -> LandResult {
    let (h, _goals) = harness().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    land_policy(&h).await;
    let _remote = clean_repository(&h);
    h.immediate_point().await;
    h.rt.with_state(|state| state.observe_mode(decide::Mode::Land));
    let policy = h.autopilot.sup.policy().expect("native Land policy");
    let state = h.rt.state();
    let decision = decide::decide_with_usage(
        &state,
        &policy,
        &h.autopilot.timing,
        clock(),
        h.rt.jitter,
        false,
    );
    let Act::Type { rule, ref text } = decision.act else {
        panic!("the original task owes a native Land instruction: {decision:?}");
    };
    assert_eq!(rule, words::RULE_LAND);
    let authority = DecisionAuthority {
        quota: None,
        quota_identity: None,
        policy,
        offs: h.autopilot.sup.offs(),
        point: state.point_at(),
        accepted_turns: h.rt.accepted_turns.load(Ordering::SeqCst),
        native_origin: state.native_origin,
    };
    land_reply(&h, status).await;
    // Production submit runs under the evaluator's permit too. Holding it here
    // owns the later look while the actual Land instruction and goal tools run.
    assert!(
        h.autopilot
            .submit(
                &h.rt,
                &h.test.codex,
                &authority,
                &decision,
                rule,
                text.clone()
            )
            .await,
        "native Land instruction was accepted"
    );
    completed(&h).await;
    let original = goal(&h).await;
    let state = h.rt.state();
    assert!(state.landing && state.point.as_ref().unwrap().by_autopilot);
    assert_eq!(state.point.as_ref().unwrap().text.as_deref(), Some(CLEAN));
    assert_eq!(
        responses::received_responses_requests(&h._server)
            .await
            .len(),
        3,
        "the Land turn executed create_goal, its terminal update and final defense"
    );
    if status == "blocked" {
        assert!(lock(&h.rt.model_blocked_goal).is_some());
    } else {
        assert!(lock(&h.rt.model_blocked_goal).is_none());
    }
    due(&h).await;
    // This is the real repository probe, not a fabricated clean read. Its
    // result is attached to the same completed point the evaluator judges.
    let read = h
        .autopilot
        .probe_repo(
            &h.rt,
            "model-blocked landing regression",
            crate::repo::PROBE_LIMIT,
        )
        .await;
    assert_eq!(read, RepoRead::Read(None));
    h.rt.with_state(|state| state.probed_for(state.point_at(), read));
    drop(evaluation);
    evaluate(&h).await;
    let after = goal(&h).await;
    if after.status == ThreadGoalStatus::Active {
        wait_requests(&h, 4).await;
    }
    let result = LandResult {
        goal: after,
        landed: h.rt.state().landed,
        acts: h.fake.acts(),
        requests: responses::received_responses_requests(&h._server).await,
        original_id: original.goal_id,
    };
    finish(&h).await;
    result
}

#[test]
fn a_model_blocked_goal_cannot_land_on_a_clean_defense_and_clean_repository() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            // Negative control: the same real Land turn completing its native
            // goal remains free to land. Only a positively bound block recovers.
            let complete = land_yield("complete").await;
            assert_eq!(complete.goal.status, ThreadGoalStatus::Complete);
            assert!(complete.landed);
            assert!(complete.acts.iter().any(|rule| rule == words::RULE_LANDED));
            let blocked = land_yield("blocked").await;
            assert!(
                !blocked.landed && blocked.acts.iter().all(|rule| rule != words::RULE_LANDED),
                "a model's blocked original goal cannot be recorded landed: {:?}",
                blocked.acts
            );
            assert_eq!(blocked.goal.goal_id, blocked.original_id);
            assert_eq!(blocked.goal.objective, OBJECTIVE);
            assert_eq!(blocked.goal.status, ThreadGoalStatus::Active);
            assert_eq!(blocked.requests.len(), 4);
            assert!(blocked.requests[3].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
        })
        .await
        .expect("model-blocked Land admission hang detector");
    });
}

#[test]
fn a_usage_reset_resumes_the_original_goal_the_model_had_already_blocked() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            h.fake.set_policy_key("limit_wait", "true");
            h.fake.set_policy_key("resume_limits", "true");
            h.fake.set_policy_key("probe_api", "false");
            wait_for_supervisor(|| {
                h.autopilot.sup.policy().is_some_and(|policy| {
                    policy.limit_wait && policy.resume_limits && !policy.probe_api
                })
            })
            .await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let binding = lock(&h.rt.model_blocked_goal).clone();
            assert!(
                binding.is_some(),
                "the successful native tool owns the original goal"
            );
            let at = clock() - Duration::from_secs(3600);
            h.rt.with_state(|state| {
                // The typed usage-wall seam is the same one existing native
                // goal_resume_tests exercise. The goal and stop origin above
                // remain the genuine tool's persisted state.
                state.observe_turn_end(
                    TurnEnd {
                        at,
                        by_autopilot: true,
                        by_person: false,
                        text: None,
                        earlier: None,
                        questions: false,
                        worked: Duration::from_secs(600),
                        wall: Some(Wall::Usage {
                            reset_at: Some(at),
                            reset_label: None,
                        }),
                        aborted: false,
                        ran_tools: false,
                        restored: false,
                    },
                    &h.autopilot.timing,
                );
            });
            h.immediate_point().await;
            let policy = h.autopilot.sup.policy().expect("reset recovery policy");
            let decision = decide::decide_with_usage(
                &h.rt.state(),
                &policy,
                &h.autopilot.timing,
                clock(),
                h.rt.jitter,
                false,
            );
            assert!(
                matches!(decision.act, Act::ResumeLimit { .. }),
                "{decision:?}"
            );
            drop(evaluation);
            evaluate(&h).await;
            let after = goal(&h).await;
            if after.status == ThreadGoalStatus::Active {
                wait_requests(&h, 3).await;
            }
            let requests = responses::received_responses_requests(&h._server).await;
            let acts = h.fake.acts();
            finish(&h).await;
            assert_eq!(after.goal_id, original.goal_id);
            assert_eq!(after.objective, original.objective);
            assert_eq!(after.token_budget, original.token_budget);
            assert_eq!(
                after.status,
                ThreadGoalStatus::Active,
                "the reset must resume a positively bound model block: {acts:?}"
            );
            assert_eq!(requests.len(), 3);
            assert!(requests[2].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            assert!(acts.iter().any(|rule| rule == words::RULE_LIMIT_RESUME));
        })
        .await
        .expect("usage reset after native model block hang detector");
    });
}
