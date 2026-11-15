// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real direct and CodeMode dispatch gates can finish goal commits out of order.

use super::*;
use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolCallSource;
use codex_protocol::protocol::EventMsg;
use std::path::Path;

const OBJECTIVE: &str = "Finish and verify the original compiler integration and land it on main";
const DIRECT_CALL: &str = "direct-overlap-block";
const THIRD_CALL: &str = "direct-overlap-third";

#[derive(Clone, Debug)]
struct Finished {
    nested: bool,
    call: String,
    turn: String,
    outcome: ToolCallOutcome,
}

#[derive(Default)]
struct Outcomes {
    finished: Mutex<Vec<Finished>>,
    provider_gate: tokio::sync::Notify,
}

impl ToolLifecycleContributor for Outcomes {
    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if input.tool_name.is_default_namespace()
                && input.tool_name.name == codex_goal_extension::UPDATE_GOAL_TOOL_NAME
            {
                lock(&self.finished).push(Finished {
                    nested: matches!(input.source, ToolCallSource::CodeMode { .. }),
                    call: input.call_id.to_owned(),
                    turn: input.turn_id.to_owned(),
                    outcome: input.outcome,
                });
                if input.call_id == DIRECT_CALL {
                    // Auto has already accepted B's exact native receipt. Keep
                    // its output from reaching the provider until A finishes.
                    self.provider_gate.notified().await;
                }
            }
        })
    }
}

struct Hooks {
    root: tempfile::TempDir,
}

impl Hooks {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("owned hook gate");
        // B waits before handler execution. A uses the independent nested gate
        // and must commit before its actual PostToolUse command opens B's gate.
        std::fs::write(
            root.path().join("pre.sh"),
            format!(
                r#"#!/bin/sh
input=$(/bin/cat)
case "$input" in
  *{DIRECT_CALL}*)
    while [ ! -f "$1/entered" ] || [ ! -f "$1/allow-duplicate" ]; do /bin/sleep 0.01; done
    ;;
esac
"#
            ),
        )
        .expect("write native pre-hook gate");
        std::fs::write(
            root.path().join("post.sh"),
            r#"#!/bin/sh
input=$(/bin/cat)
if [ ! -f "$1/armed" ]; then exit 0; fi
case "$input" in
  *direct-overlap-third*)
    : > "$1/third-entered"
    while [ ! -f "$1/release-third" ]; do /bin/sleep 0.01; done
    exit 0
    ;;
esac
if /bin/mkdir "$1/first" 2>/dev/null; then
    : > "$1/entered"
    while [ ! -f "$1/release" ]; do /bin/sleep 0.01; done
fi
"#,
        )
        .expect("write first-only native post-hook gate");
        Self { root }
    }

    fn command(&self, script: &str) -> String {
        let quote =
            |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
        format!(
            "/bin/sh {} {}",
            quote(&self.root.path().join(script)),
            quote(self.root.path())
        )
    }

    fn configuration(&self) -> serde_json::Value {
        serde_json::json!({"hooks": {
            "PreToolUse": [{"matcher": "^update_goal$", "hooks": [{
                "type": "command", "command": self.command("pre.sh")
            }]}],
            "PostToolUse": [{"matcher": "^update_goal$", "hooks": [{
                "type": "command", "command": self.command("post.sh")
            }]}]
        }})
    }

    fn mark(&self, name: &str) {
        std::fs::write(self.root.path().join(name), b"ready\n").expect("release native hook gate");
    }

    fn release(&self) {
        self.mark("allow-duplicate");
        self.mark("release");
        self.mark("release-third");
    }
}

impl Drop for Hooks {
    fn drop(&mut self) {
        self.release();
    }
}

async fn goal(h: &Harness) -> codex_state::ThreadGoal {
    h.test
        .codex
        .state_db()
        .expect("native database")
        .thread_goals()
        .get_thread_goal(h.rt.id)
        .await
        .expect("read native goal")
        .expect("original goal remains")
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Fresh,
    ExistingKnown,
    ApiGap,
    AccountingOnly,
    ThirdUnaccepted,
}

async fn completed(h: &Harness) {
    core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnComplete(_)),
        NATIVE_HANG,
    )
    .await;
    while h.test.codex.has_active_turn().await {
        tokio::task::yield_now().await;
    }
    wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
}

async fn host_set(
    h: &Harness,
    goals: &GoalService,
    status: codex_protocol::protocol::ThreadGoalStatus,
) {
    let outcome = goals
        .set_thread_goal(
            &h.test.codex.state_db().expect("native database"),
            GoalSetRequest {
                thread_id: h.rt.id,
                objective: GoalObjectiveUpdate::Keep,
                status: Some(status),
                token_budget: GoalTokenBudgetUpdate::Keep,
                max_goal_token_budget: None,
            },
            async { Ok(()) },
        )
        .await
        .expect("native host/API status intent");
    h.test
        .codex
        .append_rollout_items(&[outcome.thread_goal_updated_item()])
        .await
        .expect("persist the actual host/API outcome");
    outcome.apply_runtime_effects(goals).await;
    h.test
        .codex
        .flush_rollout()
        .await
        .expect("flush host/API outcome");
}

async fn start(h: &Harness) {
    h.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Continue the original compiler goal and reassess the obstacle.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("start actual CodeMode/native dispatch");
}

async fn scenario(
    h: &Harness,
    goals: &GoalService,
    hooks: &Hooks,
    outcomes: &Outcomes,
    case: Case,
) {
    let rt = Arc::clone(&h.rt);
    let evaluation = rt.hold_eval_when_free().await;
    goals
        .set_thread_goal(
            &h.test.codex.state_db().expect("native database"),
            GoalSetRequest {
                thread_id: h.rt.id,
                objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                max_goal_token_budget: None,
            },
            async { Ok(()) },
        )
        .await
        .expect("seed the actual native goal");
    h.test
        .codex
        .thread_extension_data()
        .get::<codex_goal_extension::GoalRuntimeHandle>()
        .expect("native goal runtime")
        .restore_after_resume()
        .await
        .expect("restore original goal accounting");
    let original = goal(h).await;
    let known_source = if matches!(case, Case::ExistingKnown) {
        responses::mount_sse_sequence(
            &h._server,
            vec![
                responses::sse(vec![
                    responses::ev_response_created("known-source-call"),
                    responses::ev_function_call(
                        "known-source-block",
                        codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                        r#"{"status":"blocked"}"#,
                    ),
                    responses::ev_completed_with_tokens("known-source-call", 5),
                ]),
                responses::sse(vec![
                    responses::ev_assistant_message(
                        "known-source-final",
                        "The original model reported a temporary validation obstacle.",
                    ),
                    responses::ev_completed("known-source-done"),
                ]),
            ],
        )
        .await;
        start(h).await;
        completed(h).await;
        let source = lock(&h.rt.model_blocked_goal)
            .as_ref()
            .expect("an actual earlier native source exists")
            .goal
            .clone();
        lock(&outcomes.finished).clear();
        Some(source)
    } else {
        None
    };
    let requests_before = responses::received_responses_requests(&h._server)
        .await
        .len();
    hooks.mark("armed");
    let script =
        "// @exec: {\"yield_time_ms\": 1}\ntext(await tools.update_goal({status: \"blocked\"}));";
    let mut overlap = vec![
        responses::sse(vec![
            responses::ev_response_created("overlap-nested"),
            responses::ev_custom_tool_call("overlap-exec", "exec", script),
            responses::ev_completed_with_tokens("overlap-nested", 10),
        ]),
        responses::sse(vec![
            responses::ev_response_created("overlap-direct"),
            responses::ev_function_call(
                DIRECT_CALL,
                codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                r#"{"status":"blocked"}"#,
            ),
            responses::ev_completed_with_tokens("overlap-direct", 20),
        ]),
    ];
    if matches!(case, Case::ThirdUnaccepted) {
        overlap.push(responses::sse(vec![
            responses::ev_response_created("overlap-third"),
            responses::ev_function_call(
                THIRD_CALL,
                codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                r#"{"status":"blocked"}"#,
            ),
            responses::ev_completed_with_tokens("overlap-third", 30),
        ]));
    }
    overlap.push(responses::sse(vec![
        responses::ev_response_created("overlap-final"),
        responses::ev_assistant_message(
            "overlap-final-message",
            "The temporary build artifact is missing, so I stopped the goal.",
        ),
        responses::ev_completed("overlap-final"),
    ]));
    let overlap_requests = overlap.len();
    responses::mount_sse_sequence(&h._server, overlap).await;
    start(h).await;

    wait_for_supervisor(|| hooks.root.path().join("entered").is_file()).await;
    let a_epoch = goals
        .alab_model_goal_epoch(h.rt.id)
        .expect("A's actual committed native epoch");
    assert_eq!(goal(h).await.status, codex_state::ThreadGoalStatus::Blocked);
    if known_source.is_none() {
        assert!(
            lock(&h.rt.model_blocked_goal).is_none(),
            "the nested handler committed before its completion callback"
        );
    }
    let mut accounted = None;
    match case {
        Case::ApiGap => {
            host_set(h, goals, codex_protocol::protocol::ThreadGoalStatus::Paused).await;
            host_set(
                h,
                goals,
                codex_protocol::protocol::ThreadGoalStatus::Blocked,
            )
            .await;
            assert_eq!(
                h.shared.person_goal_stops(h.rt.key()),
                0,
                "automatic host intents do not impersonate a person"
            );
        }
        Case::AccountingOnly => {
            let before = goal(h).await;
            let epoch = goals
                .alab_model_goal_epoch(h.rt.id)
                .expect("actual native owner");
            h.test
                .codex
                .state_db()
                .expect("native database")
                .thread_goals()
                .account_thread_goal_usage(
                    h.rt.id,
                    3,
                    7,
                    codex_state::GoalAccountingMode::ActiveOrStopped,
                    Some(original.goal_id.as_str()),
                )
                .await
                .expect("additive native usage accounting");
            let after = goal(h).await;
            assert_eq!(after.tokens_used, before.tokens_used + 7);
            assert_eq!(after.time_used_seconds, before.time_used_seconds + 3);
            assert_eq!(
                goals.alab_model_goal_epoch(h.rt.id),
                Some(epoch),
                "accounting preserves the native mutation epoch"
            );
            accounted = Some(after.tokens_used);
        }
        Case::Fresh | Case::ExistingKnown | Case::ThirdUnaccepted => {}
    }
    hooks.mark("allow-duplicate");
    wait_for_supervisor(|| !lock(&outcomes.finished).is_empty()).await;
    {
        let finished = lock(&outcomes.finished);
        assert_eq!(finished.len(), 1);
        assert!(!finished[0].nested, "direct B finishes before nested A");
        assert_eq!(finished[0].call, DIRECT_CALL);
        assert_eq!(
            finished[0].outcome,
            ToolCallOutcome::Completed { success: true }
        );
    }
    // The real exec returned Yielded: the provider reached the direct call while
    // the nested code cell's native PostToolUse command is still waiting.
    assert!(!hooks.root.path().join("release").exists());
    if matches!(case, Case::ThirdUnaccepted) {
        outcomes.provider_gate.notify_one();
        wait_for_supervisor(|| hooks.root.path().join("third-entered").is_file()).await;
        hooks.mark("release");
        wait_for_supervisor(|| lock(&outcomes.finished).len() == 2).await;
        let source_turn = lock(&outcomes.finished)[1].turn.clone();
        let expected = codex_alab_core::session::WallGoal {
            goal_id: original.goal_id.clone(),
            objective: original.objective.clone(),
            token_budget: original.token_budget,
            turn: source_turn,
            person_stops: h.shared.person_goal_stops(h.rt.key()),
        };
        assert!(
            h.autopilot
                .current_model_binding(&h.rt)
                .await
                .expect("source while C awaits acceptance")
                .is_none()
        );
        let retained = h
            .shared
            .model_goal_stops(h.rt.key())
            .source(&a_epoch)
            .expect("A's accepted callback evidence remains in the native receipt ledger");
        assert_eq!(retained.origin.goal, expected);
        assert_eq!(retained.origin.epoch, a_epoch);
        assert!(
            !a_epoch.is_current(),
            "accepted evidence is passive while C's edge is unaccepted"
        );
        hooks.mark("release-third");
        wait_for_supervisor(|| lock(&outcomes.finished).len() == 3).await;
        assert_eq!(
            lock(&h.rt.model_goal_import).goal.as_ref(),
            Some(&expected),
            "C's accepted receipt stages the exact original source for history validation"
        );
        assert_eq!(
            h.autopilot
                .current_model_goal(&h.rt)
                .await
                .expect("validate the original source after C acceptance"),
            Some(expected)
        );
    } else {
        hooks.mark("release");
        wait_for_supervisor(|| lock(&outcomes.finished).len() == 2).await;
        outcomes.provider_gate.notify_one();
    }
    completed(h).await;
    let finished = lock(&outcomes.finished).clone();
    assert!(
        finished[1].nested,
        "the real CodeMode source completes second"
    );
    assert_eq!(
        finished[1].outcome,
        ToolCallOutcome::Completed { success: true }
    );
    assert_eq!(
        finished[0].turn, finished[1].turn,
        "both native calls belong to the same Core turn"
    );
    assert_ne!(finished[0].call, finished[1].call);
    if matches!(case, Case::ThirdUnaccepted) {
        assert_eq!(finished.len(), 3);
        assert_eq!(finished[2].call, THIRD_CALL);
        assert!(!finished[2].nested);
        assert_eq!(finished[2].turn, finished[1].turn);
        assert_eq!(
            finished[2].outcome,
            ToolCallOutcome::Completed { success: true }
        );
    }
    assert_eq!(
        responses::received_responses_requests(&h._server)
            .await
            .len(),
        requests_before + overlap_requests
    );
    let blocked = goal(h).await;
    assert_eq!(blocked.goal_id, original.goal_id);
    assert_eq!(blocked.objective, original.objective);
    assert_eq!(blocked.token_budget, original.token_budget);
    assert_eq!(blocked.status, codex_state::ThreadGoalStatus::Blocked);
    if let Some(accounted) = accounted {
        assert!(
            blocked.tokens_used >= accounted,
            "overlap cannot lose acknowledged usage"
        );
    }
    let binding = h
        .autopilot
        .current_model_binding(&h.rt)
        .await
        .expect("read actual native recovery proof");
    if matches!(case, Case::ApiGap) {
        assert!(
            binding.is_none(),
            "an intervening native API intent must break duplicate receipt continuity"
        );
    } else {
        let source = binding.expect(
            "out-of-order successful native completions must retain the fresh model stop's source",
        );
        assert_eq!(source.goal.goal_id, original.goal_id);
        assert_eq!(source.goal.objective, original.objective);
        assert_eq!(source.goal.token_budget, original.token_budget);
        if let Some(known) = known_source {
            assert_eq!(
                source.goal, known,
                "duplicates preserve the entire original source"
            );
        } else {
            assert_eq!(source.goal.turn, finished[1].turn);
        }
    }
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    drop(evaluation);
    h.rt.with_state(|state| {
        state.point.as_mut().expect("completed native point").at =
            clock() - Duration::from_secs(3601)
    });
    h.immediate_point().await;
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    if matches!(case, Case::ApiGap) {
        assert_eq!(goal(h).await, blocked);
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
        assert_eq!(
            responses::received_responses_requests(&h._server)
                .await
                .len(),
            requests_before + overlap_requests
        );
        return;
    }
    while responses::received_responses_requests(&h._server)
        .await
        .len()
        < requests_before + overlap_requests + 1
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let resumed = goal(h).await;
    assert_eq!(resumed.goal_id, original.goal_id);
    assert_eq!(resumed.objective, original.objective);
    assert_eq!(resumed.token_budget, original.token_budget);
    assert_eq!(resumed.tokens_used, blocked.tokens_used);
    assert_eq!(resumed.status, codex_state::ThreadGoalStatus::Active);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
    let started = core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnStarted(_)),
        NATIVE_HANG,
    )
    .await;
    assert_eq!(
        serde_json::to_value(started).expect("native goal attribution")["turn_attribution"]["turn_trigger"],
        "goal"
    );
    let requests = responses::received_responses_requests(&h._server).await;
    assert!(
        requests[requests_before + overlap_requests]
            .body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE)
    );
}

async fn run(case: Case) {
    let hooks = Hooks::new();
    let goals = Arc::new(GoalService::new());
    let outcomes = Arc::new(Outcomes::default());
    let mut h = Harness::new_with_accounted_goals_code_mode_and_hooks(
        Arc::clone(&goals),
        HookFixture {
            configuration: hooks.configuration(),
            observer: Arc::clone(&outcomes) as Arc<dyn ToolLifecycleContributor>,
        },
    )
    .await;
    h.fake.set_policy_key("upgrade", "false");
    h.fake.set_policy_key("autopilot", "cruise");
    wait_for_supervisor(|| {
        h.autopilot
            .sup
            .policy()
            .is_some_and(|policy| !policy.upgrade && policy.autopilot == decide::Mode::Cruise)
    })
    .await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_delay(Duration::from_secs(60)))
        .with_priority(10)
        .mount(&h._server)
        .await;
    let result = {
        let mut pending = Box::pin(tokio::time::timeout(
            NATIVE_HANG,
            scenario(&h, &goals, &hooks, &outcomes, case),
        ));
        std::future::poll_fn(|context| {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                std::future::Future::poll(pending.as_mut(), context)
            })) {
                Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
                Ok(std::task::Poll::Ready(result)) => std::task::Poll::Ready(Ok(result)),
                Err(panic) => std::task::Poll::Ready(Err(panic)),
            }
        })
        .await
    };
    hooks.release();
    outcomes.provider_gate.notify_one();
    if !matches!(&result, Ok(Ok(()))) {
        h.retain_artifacts();
        h.retain_artifact_root("hook", hooks.root.path());
    }
    Autopilot::cancel_timer(&h.rt).await;
    h.shutdown_and_export()
        .await
        .expect("owned native shutdown");
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("native direct/CodeMode overlap hang detector: {error}"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[test]
fn model_goal_code_mode_dispatch_overlap_preserves_the_original_source() {
    native_test(|| run(Case::Fresh));
}

#[test]
fn model_goal_code_mode_dispatch_overlap_preserves_an_existing_known_source() {
    native_test(|| run(Case::ExistingKnown));
}

#[test]
fn model_goal_code_mode_dispatch_overlap_cannot_cross_a_native_api_hold() {
    native_test(|| run(Case::ApiGap));
}

#[test]
fn model_goal_code_mode_dispatch_overlap_preserves_accounted_usage() {
    native_test(|| run(Case::AccountingOnly));
}

#[test]
fn model_goal_code_mode_dispatch_overlap_retains_source_until_a_third_commit_is_accepted() {
    native_test(|| run(Case::ThirdUnaccepted));
}
