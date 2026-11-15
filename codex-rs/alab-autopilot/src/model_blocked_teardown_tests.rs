// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A forced sampling-task teardown must not erase an already committed goal stop.

use super::super::upgrade_handoff_tests::HookFixture;
use super::*;
use codex_alab_core::session::WallGoal;
use codex_protocol::protocol::TurnAbortReason;
use std::path::Path;
use std::sync::atomic::AtomicBool;

const CALL: &str = "model-block-forced-teardown-call";
const HELD_ITEM: &str = "The original model is blocked on a temporary compiler artifact.";

struct Observed {
    item_entered: AtomicBool,
    item_returned: AtomicBool,
    item_dropped: AtomicBool,
    item_release: tokio::sync::Semaphore,
    finished: Mutex<Vec<ToolCallOutcome>>,
    sources_at_finish: Mutex<Vec<Option<WallGoal>>>,
}

impl Default for Observed {
    fn default() -> Self {
        Self {
            item_entered: AtomicBool::new(false),
            item_returned: AtomicBool::new(false),
            item_dropped: AtomicBool::new(false),
            item_release: tokio::sync::Semaphore::new(0),
            finished: Mutex::new(Vec::new()),
            sources_at_finish: Mutex::new(Vec::new()),
        }
    }
}

struct ItemCallback<'a>(&'a Observed);

impl Drop for ItemCallback<'_> {
    fn drop(&mut self) {
        if !self.0.item_returned.load(Ordering::SeqCst) {
            self.0.item_dropped.store(true, Ordering::SeqCst);
        }
    }
}

impl TurnLifecycleContributor for Observed {
    fn on_item_completed<'a>(
        &'a self,
        _thread_store: &'a ExtensionData,
        _turn_store: &'a ExtensionData,
        item: &'a TurnItem,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !matches!(item, TurnItem::AgentMessage(message) if agent_text(message) == HELD_ITEM)
            {
                return;
            }
            assert!(
                !self.item_entered.swap(true, Ordering::SeqCst),
                "the actual streamed assistant item is observed once"
            );
            let _callback = ItemCallback(self);
            let _release = self.item_release.acquire().await.expect("owned item gate");
            self.item_returned.store(true, Ordering::SeqCst);
        })
    }
}

impl ToolLifecycleContributor for Observed {
    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if input.call_id == CALL
                && input.tool_name.is_default_namespace()
                && input.tool_name.name == codex_goal_extension::UPDATE_GOAL_TOOL_NAME
            {
                lock(&self.finished).push(input.outcome);
                let source = Autopilot::rt(input.thread_store).and_then(|rt| {
                    lock(&rt.model_blocked_goal)
                        .as_ref()
                        .map(|binding| binding.goal.clone())
                });
                lock(&self.sources_at_finish).push(source);
            }
        })
    }
}

struct HookGate(tempfile::TempDir);

impl HookGate {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("owned forced-teardown hook gate");
        std::fs::write(
            root.path().join("post.sh"),
            r#"#!/bin/sh
/bin/cat >/dev/null
echo "$$" > "$1/pid"
: > "$1/entered"
while [ ! -f "$1/release" ]; do /bin/sleep 0.01; done
"#,
        )
        .expect("write the actual native post-tool gate");
        Self(root)
    }

    fn configuration(&self) -> serde_json::Value {
        let quote =
            |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
        let command = format!(
            "/bin/sh {} {}",
            quote(&self.0.path().join("post.sh")),
            quote(self.0.path())
        );
        serde_json::json!({"hooks": {"PostToolUse": [{
            "matcher": "^update_goal$", "hooks": [{"type": "command", "command": command}]
        }]}})
    }

    fn release(&self) {
        std::fs::write(self.0.path().join("release"), b"release\n")
            .expect("release the owned post-tool hook");
    }

    async fn wait_for_exit(&self) {
        let Ok(pid) = std::fs::read_to_string(self.0.path().join("pid")) else {
            return;
        };
        let pid: libc::pid_t = pid.trim().parse().expect("the actual hook process id");
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                // SAFETY: signal zero only checks the recorded owned process; it sends no signal.
                if unsafe { libc::kill(pid, 0) } != 0 {
                    assert_eq!(
                        std::io::Error::last_os_error().raw_os_error(),
                        Some(libc::ESRCH),
                        "the hook exited; permission refusal is not process-exit evidence"
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the owned native hook process must settle during cleanup");
    }
}

impl Drop for HookGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.path().join("release"), b"release\n");
    }
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Forced,
    Released,
    ApiPause,
    Shutdown,
}

async fn pause_with_native_api(h: &Harness, goals: &GoalService) {
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
        .expect("a real API pause after the native model commit");
    h.test
        .codex
        .append_rollout_items(&[outcome.thread_goal_updated_item()])
        .await
        .expect("persist the actual API pause outcome");
    outcome.apply_runtime_effects(goals).await;
    h.test
        .codex
        .flush_rollout()
        .await
        .expect("flush the native API pause");
}

async fn scenario(
    h: &Harness,
    goals: &GoalService,
    hook: &HookGate,
    observed: &Observed,
    case: Case,
) {
    let evaluation = h.rt.hold_eval_when_free().await;
    let original = seed(h, goals).await;
    let mut bodies = vec![responses::sse(vec![
        responses::ev_response_created("model-block-forced-teardown"),
        responses::ev_function_call(
            CALL,
            codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
            r#"{"status":"blocked"}"#,
        ),
        responses::ev_assistant_message("model-block-held-item", HELD_ITEM),
        responses::ev_completed_with_tokens("model-block-forced-teardown", 10),
    ])];
    if matches!(case, Case::Released) {
        bodies.push(responses::sse(vec![
            responses::ev_assistant_message(
                "model-block-drained-final",
                "The temporary artifact is unavailable.",
            ),
            responses::ev_completed("model-block-drained-final"),
        ]));
    }
    responses::mount_sse_sequence(&h._server, bodies).await;
    h.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Continue the original compiler goal and report the obstacle.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("start the actual native model goal tool");
    wait_for_supervisor(|| observed.item_entered.load(Ordering::SeqCst)).await;
    wait_for_supervisor(|| hook.0.path().join("entered").exists()).await;
    let committed = goal(h).await;
    assert_eq!(committed.goal_id, original.goal_id);
    assert_eq!(committed.objective, original.objective);
    assert_eq!(committed.token_budget, original.token_budget);
    assert_eq!(committed.status, ThreadGoalStatus::Blocked);
    assert!(lock(&observed.finished).is_empty());
    assert!(lock(&h.rt.model_blocked_goal).is_none());
    assert_eq!(h.shared.person_goal_stops(h.rt.key()), 0);
    let accepted_before_stop = h.rt.accepted_turns.load(Ordering::SeqCst);
    if matches!(case, Case::ApiPause) {
        pause_with_native_api(h, goals).await;
        assert_eq!(goal(h).await.status, ThreadGoalStatus::Paused);
    }
    if matches!(case, Case::Released) {
        observed.item_release.add_permits(1);
        hook.release();
        completed(h).await;
        assert!(observed.item_returned.load(Ordering::SeqCst));
        assert!(!observed.item_dropped.load(Ordering::SeqCst));
        assert_eq!(
            *lock(&observed.finished),
            vec![ToolCallOutcome::Completed { success: true }]
        );
    } else {
        h.test
            .codex
            .submit(if matches!(case, Case::Shutdown) {
                Op::Shutdown
            } else {
                Op::Interrupt
            })
            .await
            .expect("stop the held native sampling task");
        wait_for_supervisor(|| observed.item_dropped.load(Ordering::SeqCst)).await;
        assert!(
            !observed.item_returned.load(Ordering::SeqCst),
            "Core dropped the held callback before the fixture released any gate"
        );
        // Keep both fixture gates closed until cancellation delivers the real
        // finish. Releasing the file gate sooner could race cancellation with
        // a legitimate normal hook result on another runtime worker.
        wait_for_supervisor(|| !lock(&observed.finished).is_empty()).await;
        assert_eq!(*lock(&observed.finished), vec![ToolCallOutcome::Aborted]);
        // Cancellation drops the native hook runner and kills its process
        // group. Release the file gate only as cleanup insurance after that
        // outcome, before waiting for the turn's final abort publication.
        hook.release();
        let aborted = core_test_support::wait_for_event_with_timeout(
            &h.test.codex,
            |event| matches!(event, EventMsg::TurnAborted(_)),
            HANG,
        )
        .await;
        assert!(
            matches!(aborted, EventMsg::TurnAborted(event) if event.reason == TurnAbortReason::Interrupted)
        );
        while h.test.codex.has_active_turn().await {
            tokio::task::yield_now().await;
        }
        wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
        assert!(
            observed.item_dropped.load(Ordering::SeqCst),
            "Core forcibly dropped the pending item callback"
        );
        assert!(
            !observed.item_returned.load(Ordering::SeqCst),
            "the fixture did not release the callback to drain tools"
        );
        eprintln!(
            "forced native teardown tool outcomes: {:?}",
            *lock(&observed.finished)
        );
    }
    let stopped = goal(h).await;
    assert_eq!(stopped.goal_id, original.goal_id);
    assert_eq!(stopped.objective, original.objective);
    assert_eq!(stopped.token_budget, original.token_budget);
    assert_eq!(h.shared.person_goal_stops(h.rt.key()), 0);
    let requests_before = responses::received_responses_requests(&h._server)
        .await
        .len();
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    if matches!(case, Case::Shutdown) {
        core_test_support::wait_for_event_with_timeout(
            &h.test.codex,
            |event| matches!(event, EventMsg::ShutdownComplete),
            HANG,
        )
        .await;
        h.test.codex.wait_until_terminated().await;
        assert_eq!(*lock(&observed.finished), vec![ToolCallOutcome::Aborted]);
        let source = lock(&observed.sources_at_finish)
            .first()
            .cloned()
            .flatten()
            .expect("native shutdown drains the committed source before thread retirement");
        assert_eq!(source.goal_id, original.goal_id);
        assert_eq!(source.objective, original.objective);
        assert_eq!(source.token_budget, original.token_budget);
        assert_eq!(stopped.status, ThreadGoalStatus::Blocked);
        assert_eq!(goal(h).await, stopped);
        assert_eq!(
            h.rt.accepted_turns.load(Ordering::SeqCst),
            accepted_before_stop
        );
        assert_eq!(
            requests_before, 1,
            "shutdown starts no replacement model turn"
        );
        assert_eq!(
            responses::received_responses_requests(&h._server)
                .await
                .len(),
            requests_before
        );
        return;
    }
    if matches!(case, Case::ApiPause) {
        assert!(
            h.autopilot
                .current_model_goal(&h.rt)
                .await
                .expect("read native model proof")
                .is_none()
        );
        drop(evaluation);
        due(h).await;
        evaluate(h).await;
        assert_eq!(goal(h).await, stopped);
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
        assert_eq!(
            responses::received_responses_requests(&h._server)
                .await
                .len(),
            requests_before
        );
        return;
    }
    let binding = h
        .autopilot
        .current_model_goal(&h.rt)
        .await
        .expect("read native model proof")
        .expect("forced teardown cannot erase the original committed model-block source");
    assert_eq!(binding.goal_id, original.goal_id);
    assert_eq!(binding.objective, original.objective);
    assert_eq!(binding.token_budget, original.token_budget);
    if matches!(case, Case::Forced) {
        assert_eq!(*lock(&observed.finished), vec![ToolCallOutcome::Aborted]);
    }
    drop(evaluation);
    due(h).await;
    evaluate(h).await;
    wait_requests(h, requests_before + 1).await;
    let resumed = goal(h).await;
    assert_eq!(resumed.goal_id, original.goal_id);
    assert_eq!(resumed.objective, original.objective);
    assert_eq!(resumed.token_budget, original.token_budget);
    assert_eq!(resumed.tokens_used, stopped.tokens_used);
    assert_eq!(resumed.status, ThreadGoalStatus::Active);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
    let started = core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnStarted(_)),
        HANG,
    )
    .await;
    assert_eq!(
        serde_json::to_value(started).expect("native start attribution")["turn_attribution"]["turn_trigger"],
        "goal"
    );
    let requests = responses::received_responses_requests(&h._server).await;
    assert!(requests[requests_before].body_contains_text(OBJECTIVE));
    assert!(requests[requests_before].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
}

async fn run(case: Case) {
    let hook = HookGate::new();
    let goals = Arc::new(GoalService::new());
    let observed = Arc::new(Observed::default());
    let h = Harness::new_with_accounted_goals_hooks_and_observer(
        Arc::clone(&goals),
        HookFixture {
            configuration: hook.configuration(),
            observer: Arc::clone(&observed) as Arc<dyn ToolLifecycleContributor>,
        },
        Arc::clone(&observed) as Arc<dyn TurnLifecycleContributor>,
    )
    .await;
    let (mut h, goals) = configured_harness(h, goals).await;
    let result = {
        let mut pending = Box::pin(tokio::time::timeout(
            HANG,
            scenario(&h, &goals, &hook, &observed, case),
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
    observed.item_release.add_permits(1);
    hook.release();
    if !matches!(&result, Ok(Ok(()))) {
        h.retain_artifacts();
        h.retain_artifact_root("hook", hook.0.path());
    }
    Autopilot::cancel_timer(&h.rt).await;
    if h.test.codex.has_active_turn().await {
        h.test
            .codex
            .submit(Op::Interrupt)
            .await
            .expect("interrupt owned fixture cleanup");
    }
    h.test
        .codex
        .shutdown_and_wait()
        .await
        .expect("owned native fixture shutdown");
    hook.wait_for_exit().await;
    h.export_retained_artifacts(
        codex_alab_core::test_support::DiagnosticProducerState::ShutdownReturned,
    );
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("{case:?}: native forced-teardown hang detector: {error}"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[test]
fn a_forced_sampling_teardown_preserves_the_committed_model_block_and_its_recovery() {
    native_test(|| run(Case::Forced));
}

#[test]
fn releasing_the_sampling_observer_drains_the_native_goal_tool_and_keeps_its_source() {
    native_test(|| run(Case::Released));
}

#[test]
fn an_api_pause_after_commit_defeats_model_recovery_across_forced_sampling_teardown() {
    native_test(|| run(Case::ApiPause));
}

#[test]
fn shutdown_drains_the_committed_model_stop_before_retiring_its_native_owner() {
    native_test(|| run(Case::Shutdown));
}
