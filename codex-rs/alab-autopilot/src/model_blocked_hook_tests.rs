// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native hook cancellation cannot erase a goal stop that already committed.

use super::super::upgrade_handoff_tests::HookFixture;
use super::*;
use codex_protocol::protocol::HookEventName;

#[derive(Default)]
pub(super) struct GoalToolOutcomes(Mutex<Vec<ToolCallOutcome>>);

impl ToolLifecycleContributor for GoalToolOutcomes {
    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if input.tool_name.is_default_namespace()
                && input.tool_name.name == codex_goal_extension::UPDATE_GOAL_TOOL_NAME
            {
                lock(&self.0).push(input.outcome);
            }
        })
    }
}

pub(super) async fn hook_harness(
    event: HookEventName,
) -> (Harness, Arc<GoalService>, Arc<GoalToolOutcomes>) {
    let event_name = match event {
        HookEventName::PreToolUse => "PreToolUse",
        HookEventName::PostToolUse => "PostToolUse",
        _ => panic!("this fixture covers goal-tool hooks"),
    };
    let goals = Arc::new(GoalService::new());
    let outcomes = Arc::new(GoalToolOutcomes::default());
    let h = Harness::new_with_accounted_goals_and_hooks(
        Arc::clone(&goals),
        HookFixture {
            configuration: serde_json::json!({"hooks": {
                event_name: [{"matcher": "^update_goal$", "hooks": [{
                    "type": "command", "command": "/bin/sleep 60"
                }]}]
            }}),
            observer: Arc::clone(&outcomes) as Arc<dyn ToolLifecycleContributor>,
        },
    )
    .await;
    let (h, goals) = configured_harness(h, goals).await;
    (h, goals, outcomes)
}

pub(super) async fn start_goal_tool_until_hook(h: &Harness, status: &str, hook: HookEventName) {
    responses::mount_sse_once(
        &h._server,
        responses::sse(vec![
            responses::ev_response_created("goal-hook-cancellation"),
            responses::ev_function_call(
                "goal-hook-cancellation-call",
                codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                &serde_json::json!({"status": status}).to_string(),
            ),
            responses::ev_completed_with_tokens("goal-hook-cancellation", 10),
        ]),
    )
    .await;
    h.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Continue the original compiler goal; report the current obstacle.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("start the real native goal tool");
    core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::HookStarted(event) if event.run.event_name == hook),
        HANG,
    )
    .await;
}

pub(super) async fn interrupt_hook(h: &Harness, outcomes: &GoalToolOutcomes) {
    assert!(
        lock(&outcomes.0).is_empty(),
        "the hook precedes host completion"
    );
    h.test
        .codex
        .submit(Op::Interrupt)
        .await
        .expect("interrupt while the native hook is pending");
    core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnAborted(_)),
        HANG,
    )
    .await;
    while h.test.codex.has_active_turn().await {
        tokio::task::yield_now().await;
    }
    wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
    assert_eq!(
        *lock(&outcomes.0),
        vec![ToolCallOutcome::Aborted],
        "the host reports delivery cancellation after the committed tool handler"
    );
}

#[test]
fn interrupting_a_post_tool_hook_preserves_the_committed_model_block_and_its_recovery() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals, outcomes) = hook_harness(HookEventName::PostToolUse).await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            start_goal_tool_until_hook(&h, "blocked", HookEventName::PostToolUse).await;
            let committed = goal(&h).await;
            assert_eq!(committed.goal_id, original.goal_id);
            assert_eq!(committed.status, ThreadGoalStatus::Blocked);
            assert!(
                lock(&h.rt.model_blocked_goal).is_none(),
                "the hook is still before the native tool-finish callback"
            );
            interrupt_hook(&h, &outcomes).await;
            let binding = lock(&h.rt.model_blocked_goal)
                .clone()
                .expect("the interrupted hook cannot erase the handler's committed source fact");
            assert_eq!(binding.goal.goal_id, original.goal_id);
            assert_eq!(binding.goal.objective, original.objective);
            assert_eq!(binding.goal.token_budget, original.token_budget);
            assert_eq!(
                h.shared
                    .pace(h.rt.key())
                    .and_then(|pace| pace.model_blocked_goal),
                Some(binding.goal),
                "the committed model block must remain durable across cancellation"
            );
            let interrupted = goal(&h).await;
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            drop(evaluation);
            due(&h).await;
            evaluate(&h).await;
            wait_requests(&h, 2).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.token_budget, original.token_budget);
            assert_eq!(resumed.tokens_used, interrupted.tokens_used);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            let started = core_test_support::wait_for_event_with_timeout(
                &h.test.codex,
                |event| matches!(event, EventMsg::TurnStarted(_)),
                HANG,
            )
            .await;
            assert_eq!(
                serde_json::to_value(started).expect("native start attribution")["turn_attribution"]
                    ["turn_trigger"],
                "goal"
            );
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 2);
            assert!(requests[1].body_contains_text(OBJECTIVE));
            assert!(requests[1].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            finish(&h).await;
        })
        .await
        .expect("post-tool hook interruption recovery hang detector");
    });
}

#[derive(Clone, Copy, Debug)]
enum StoppedCase {
    BeforeCommit,
    ModelPause,
    PersonPause,
}

async fn stopped_hook(case: StoppedCase) {
    tokio::time::timeout(HANG, async {
        let hook = if matches!(case, StoppedCase::BeforeCommit) {
            HookEventName::PreToolUse
        } else {
            HookEventName::PostToolUse
        };
        let (h, goals, outcomes) = hook_harness(hook).await;
        let evaluation = h.rt.hold_eval_when_free().await;
        let original = seed(&h, &goals).await;
        if matches!(case, StoppedCase::BeforeCommit) {
            h.test
                .codex
                .state_db()
                .expect("native goal database")
                .thread_goals()
                .update_thread_goal(
                    h.rt.id,
                    codex_state::GoalUpdate {
                        objective: None,
                        status: Some(ThreadGoalStatus::Blocked),
                        token_budget: None,
                        expected_goal_id: Some(original.goal_id.clone()),
                    },
                )
                .await
                .expect("a blocked row without model-stop provenance");
            h.test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("native goal runtime")
                .restore_after_resume()
                .await
                .expect("restore the stopped accounting boundary");
        }
        let status = if matches!(case, StoppedCase::ModelPause) {
            "paused"
        } else {
            "blocked"
        };
        start_goal_tool_until_hook(&h, status, hook).await;
        if matches!(case, StoppedCase::PersonPause) {
            h.shared.person_goal_set(h.rt.key(), true);
            let outcome = goals
                .set_thread_goal(
                    &h.test.codex.state_db().expect("native goal database"),
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
                .expect("persist the person's explicit pause while the hook waits");
            outcome.apply_runtime_effects(&goals).await;
        }
        interrupt_hook(&h, &outcomes).await;
        let stopped = goal(&h).await;
        assert_eq!(stopped.goal_id, original.goal_id);
        assert_eq!(stopped.objective, original.objective);
        assert_eq!(
            stopped.status,
            if matches!(case, StoppedCase::BeforeCommit) {
                ThreadGoalStatus::Blocked
            } else {
                ThreadGoalStatus::Paused
            }
        );
        assert!(lock(&h.rt.model_blocked_goal).is_none());
        assert!(
            h.shared
                .pace(h.rt.key())
                .is_none_or(|pace| pace.model_blocked_goal.is_none())
        );
        let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
        drop(evaluation);
        due(&h).await;
        evaluate(&h).await;
        assert_eq!(
            goal(&h).await,
            stopped,
            "{case:?}: interruption grants no new goal authority"
        );
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
        assert_eq!(
            responses::received_responses_requests(&h._server)
                .await
                .len(),
            1
        );
        finish(&h).await;
    })
    .await
    .unwrap_or_else(|error| panic!("{case:?}: hook stop control hang detector: {error}"));
}

#[test]
fn cancellation_before_the_goal_handler_commits_creates_no_model_stop_authority() {
    native_test(|| stopped_hook(StoppedCase::BeforeCommit));
}

#[test]
fn an_interrupted_post_tool_hook_does_not_turn_a_model_pause_into_a_recovery() {
    native_test(|| stopped_hook(StoppedCase::ModelPause));
}

#[test]
fn a_persons_pause_while_the_post_tool_hook_waits_defeats_the_committed_model_stop() {
    native_test(|| stopped_hook(StoppedCase::PersonPause));
}
