// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A real model stop retains its original goal at every scheduler admission.

use super::*;
use crate::repo::RepoRead;
use crate::repo_tests::scratch_git;
use codex_alab_core::wall_kind::LimitClass;

const CLEAN: &str = "Nothing left: all merged and pushed to origin/main.";

struct ModelQuotaRecordGate(Arc<(Mutex<bool>, std::sync::Condvar)>);

impl ModelQuotaRecordGate {
    fn release(&self) {
        *lock(&self.0.0) = true;
        self.0.1.notify_all();
    }
}

impl Drop for ModelQuotaRecordGate {
    fn drop(&mut self) {
        self.release();
    }
}

/// The model's committed block and following provider quota wall belong to one real
/// native turn. A later unread chronology must retain its readiness retry without
/// treating that turn's lost quota allowance as permission for another dispatch.
#[test]
fn a_model_blocked_goal_rechecks_unread_chronology_after_quota_invalidation() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            use codex_alab_core::session::RecoveryWall;
            use codex_alab_core::wake::Reading;
            use std::future::Future;
            use std::task::Poll;

            const CALL: &str = "model-block-before-quota-wall";
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
            let reset_epoch = i64::try_from(now_ms() / 1000).expect("current epoch") + 24 * 3600;
            responses::mount_response_handler(&h._server, move |request| {
                if request.body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE) {
                    return wiremock::ResponseTemplate::new(200).set_delay(Duration::from_secs(60));
                }
                if !request.has_function_call(CALL) {
                    return responses::sse_response(responses::sse(vec![
                        responses::ev_response_created("native-model-block-before-quota"),
                        responses::ev_function_call(
                            CALL,
                            codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                            &serde_json::json!({ "status": "blocked" }).to_string(),
                        ),
                        responses::ev_completed_with_tokens("native-model-block-before-quota", 10),
                    ]));
                }
                wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
                    "error": {
                        "type": "usage_limit_reached",
                        "message": "the model stopped its goal before this quota wall",
                        "resets_at": reset_epoch,
                        "plan_type": "pro"
                    }
                }))
            })
            .await;
            let evaluations_before_turn = h.rt.evaluations();
            start_and_complete(&h).await;
            wait_for_supervisor(|| h.rt.evaluations() > evaluations_before_turn).await;
            let bound = lock(&h.rt.model_blocked_goal)
                .clone()
                .expect("the actual successful model tool owns recovery");
            assert!(lock(&h.rt.model_goal_import).goal.is_none());
            let stopped = goal(&h).await;
            assert_eq!(stopped.goal_id, original.goal_id);
            assert_eq!(stopped.status, ThreadGoalStatus::Blocked);
            let before = h.rt.state();
            let turn_id = before
                .point_turn_id
                .clone()
                .expect("the native failed turn");
            assert_eq!(turn_id, bound.goal.turn);
            let reset = match before.point.as_ref().and_then(|point| point.wall.as_ref()) {
                Some(Wall::Usage {
                    reset_at: Some(reset),
                    ..
                }) => *reset,
                wall => {
                    panic!("the same native turn must end on its provider quota wall: {wall:?}")
                }
            };
            assert!(reset > clock() + Duration::from_secs(3600));
            assert_eq!(
                responses::received_responses_requests(&h._server)
                    .await
                    .len(),
                2
            );
            h.immediate_point().await;
            let asleep = Arc::new(Mutex::new(Duration::ZERO));
            let reading = Arc::clone(&asleep);
            h.autopilot.set_wake_read(Arc::new(move || {
                Some(Reading {
                    asleep: *lock(&reading),
                    last: None,
                })
            }));
            let quota = h.shared.quota();
            let identity = QuotaIdentity {
                account: "fixture-account".into(),
                user: Some("fixture-user".into()),
                owner: 0,
            };
            let generation =
                quota.select_owner(&identity.account, identity.user.as_deref(), identity.owner);
            let wall = quota.wall_for(identity.account.clone(), "codex".into(), clock());
            *lock(&h.autopilot.quota_test_identity) = Some(identity.clone());
            *lock(&h.rt.quota_wall) = Some(QuotaWall {
                wall: wall.clone(),
                identity: identity.clone(),
            });
            quota.start_read(1, clock());
            quota.accept(1, &identity.account, vec!["codex".into()], clock());
            assert!(h.autopilot.quota_restored(&h.rt, &wall, clock()));

            // Gate the real recovery write after authority capture. The writer
            // releases its source and record locks before waiting for the test.
            let writer = Arc::new(codex_alab_tui::session::Writer::new(
                h.shared,
                codex_alab_tui::record::sessions_dir(h.test.codex_home_path()),
                Duration::ZERO,
                None,
            ));
            writer
                .write_now()
                .expect("write the actual initial session record");
            let gate =
                ModelQuotaRecordGate(Arc::new((Mutex::new(false), std::sync::Condvar::new())));
            let waiting = Arc::clone(&gate.0);
            let (entered, recorded) = tokio::sync::oneshot::channel();
            let entered = Mutex::new(Some(entered));
            let shared = h.shared;
            let recorded_turn = turn_id.clone();
            assert!(shared.set_flush(Box::new(move || {
                let written = writer.write_now().is_ok();
                let target = shared.recovery().is_some_and(|recovery| {
                    recovery.turn_id == recorded_turn
                        && matches!(recovery.wall, RecoveryWall::Usage { .. })
                });
                if target && let Some(entered) = lock(&entered).take() {
                    entered
                        .send(())
                        .expect("the recovery write observer remains");
                    let (released, changed) = &*waiting;
                    let ready = lock(released);
                    let (ready, _) = changed
                        .wait_timeout_while(ready, Duration::from_secs(60), |released| !*released)
                        .expect("record gate lock");
                    assert!(*ready, "the recovery write gate must be released");
                }
                written
            })));
            h.autopilot
                .set_recovery_write_budget(Duration::from_secs(60));
            drop(evaluation);
            recorded
                .await
                .expect("the native idle evaluation captures recovery after quota authority");
            assert!(h.autopilot.quota_restored(&h.rt, &wall, clock()));
            let marker = codex_alab_core::goal_chronology_fence::marker_path(
                h.test.codex_home_path(),
                h.rt.key(),
            )
            .expect("original chronology marker");
            let journal = match std::fs::read(&marker) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("read the original chronology marker: {error}"),
            };
            let held = codex_alab_core::goal_chronology_fence::lock_current(
                h.test.codex_home_path(),
                h.rt.key(),
            )
            .expect("hold the real stable chronology lock");
            let chronology = held.generation();
            assert_eq!(chronology, bound.goal.chronology_fence_generation);
            assert_eq!(
                codex_alab_core::goal_chronology_fence::read_current(
                    h.test.codex_home_path(),
                    h.rt.key()
                )
                .unwrap_err()
                .kind(),
                std::io::ErrorKind::WouldBlock,
            );
            let timer_before = h.rt.timer_seq.load(Ordering::SeqCst);
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            let custody = h.shared.model_goal_custody(h.rt.key());
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
            let earliest_retry = clock() + RECHECK;
            gate.release();
            let next = next.await;
            assert!(h.autopilot.sup.journal().iter().any(|note| {
                note.contains("could not read the model-blocked goal; recovery waits")
            }));
            assert_eq!(h.rt.timer_seq.load(Ordering::SeqCst), timer_before + 1);
            assert!(h.rt.timer_due().is_some_and(|at| at >= earliest_retry));
            assert_eq!(lock(&h.rt.model_blocked_goal).as_ref(), Some(&bound));
            assert_eq!(h.shared.model_goal_custody(h.rt.key()), custody);
            assert_eq!(goal(&h).await, stopped);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
            assert!(h.fake.acts().is_empty());
            assert_eq!(
                responses::received_responses_requests(&h._server)
                    .await
                    .len(),
                2
            );
            drop(held);
            assert_eq!(std::fs::read(&marker).ok(), journal);
            assert_eq!(
                codex_alab_core::goal_chronology_fence::read_current(
                    h.test.codex_home_path(),
                    h.rt.key()
                )
                .unwrap(),
                chronology,
            );

            let (started, fresh) = tokio::sync::oneshot::channel();
            *lock(&h.rt.before_park) = Some(Box::new(move || {
                started
                    .send(())
                    .expect("the delayed timer's fresh evaluation");
            }));
            *lock(&asleep) = RECHECK * 2;
            drop(next);
            fresh
                .await
                .expect("the delayed timer rechecks after journal release");
            let fresh = h.rt.hold_eval_when_free().await;
            let owed = h.rt.owed_timer().expect("ordinary future usage reset");
            assert_eq!(owed.rule, words::RULE_LIMIT_RESUME);
            assert!(owed.not_before >= reset.checked_sub(RECHECK * 2).expect("future reset"));
            assert_eq!(goal(&h).await, stopped);
            assert_eq!(lock(&h.rt.model_blocked_goal).as_ref(), Some(&bound));
            assert_eq!(h.shared.model_goal_custody(h.rt.key()), custody);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
            assert!(h.fake.acts().is_empty());
            assert_eq!(
                responses::received_responses_requests(&h._server)
                    .await
                    .len(),
                2
            );

            h.autopilot.watching.store(false, Ordering::SeqCst);
            h.autopilot.watch_claims();
            quota.start_read(2, clock());
            quota.accept(2, &identity.account, vec!["codex".into()], clock());
            assert!(h.autopilot.quota_restored(&h.rt, &wall, clock()));
            assert!(owed.not_before > clock() + RECHECK);
            drop(fresh);
            wait_for_supervisor(|| h.fake.acts().contains(&words::RULE_LIMIT_RESUME.into())).await;
            wait_requests(&h, 3).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.token_budget, original.token_budget);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 3);
            assert!(requests[2].body_contains_text(OBJECTIVE));
            assert!(requests[2].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            finish(&h).await;
        })
        .await
        .expect("model chronology and quota retry hang detector");
    });
}

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
                            limit_class: LimitClass::Ordinary,
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
