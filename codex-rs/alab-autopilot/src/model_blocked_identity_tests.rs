// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Provider IDs can collide with an observed host CodeMode call's actual UUID.

use super::*;
use crate::extension::test_progress;
use codex_extension_api::ToolCallSource;
use codex_protocol::protocol::EventMsg;
use std::path::Path;
use std::sync::Condvar;

const OBJECTIVE: &str = "Finish and verify the original compiler integration and land it on main";

#[derive(Clone, Copy, Debug)]
enum Case {
    DuplicateFirst,
    OriginFirst,
    DirectFailure,
    NestedAborted,
    DirectFailureNestedAborted,
}

impl Case {
    fn direct_fails(self) -> bool {
        matches!(self, Self::DirectFailure | Self::DirectFailureNestedAborted)
    }

    fn nested_aborts(self) -> bool {
        matches!(self, Self::NestedAborted | Self::DirectFailureNestedAborted)
    }
}

#[derive(Clone, Debug)]
struct Call {
    nested: bool,
    id: String,
    turn: String,
    outcome: Option<ToolCallOutcome>,
}

#[derive(Default)]
struct Observed {
    nested: Mutex<Option<Call>>,
    started: Condvar,
    calls: Mutex<Vec<Call>>,
    finished: Mutex<Vec<Call>>,
    provider: tokio::sync::Notify,
}

impl Observed {
    fn nested_call(&self) -> Call {
        // Wiremock's responder is synchronous. Wait for the actual host callback,
        // rather than predict a UUID or race the CodeMode cell's yield deadline.
        let call = lock(&self.nested);
        let (call, timeout) = self
            .started
            .wait_timeout_while(call, Duration::from_secs(60), |call| call.is_none())
            .expect("native source capture");
        assert!(!timeout.timed_out(), "the actual nested tool never started");
        call.clone().expect("the observed native UUID")
    }
}

impl ToolLifecycleContributor for Observed {
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !input.tool_name.is_default_namespace()
                || input.tool_name.name != codex_goal_extension::UPDATE_GOAL_TOOL_NAME
            {
                return;
            }
            let call = Call {
                nested: matches!(input.source, ToolCallSource::CodeMode { .. }),
                id: input.call_id.to_owned(),
                turn: input.turn_id.to_owned(),
                outcome: None,
            };
            lock(&self.calls).push(call.clone());
            if call.nested {
                let mut source = lock(&self.nested);
                assert!(source.is_none(), "exactly one nested goal invocation");
                *source = Some(call);
                self.started.notify_all();
            }
        })
    }

    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !input.tool_name.is_default_namespace()
                || input.tool_name.name != codex_goal_extension::UPDATE_GOAL_TOOL_NAME
            {
                return;
            }
            let nested = matches!(input.source, ToolCallSource::CodeMode { .. });
            lock(&self.finished).push(Call {
                nested,
                id: input.call_id.to_owned(),
                turn: input.turn_id.to_owned(),
                outcome: Some(input.outcome),
            });
            if !nested {
                // Auto's B callback already ran. Prevent a terminal provider
                // response from overtaking A's actual native hook completion.
                self.provider.notified().await;
            }
        })
    }
}

struct Hooks(tempfile::TempDir);

impl Hooks {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("owned identity hook gate");
        std::fs::write(
            root.path().join("post.sh"),
            r#"#!/bin/sh
: > "$1/hook-started"
/bin/cat >/dev/null
: > "$1/hook-stdin-drained"
if /bin/mkdir "$1/first-post" 2>/dev/null; then
    : > "$1/entered"
    while [ ! -f "$1/release" ]; do /bin/sleep 0.01; done
elif [ -f "$1/hold-second" ]; then
    : > "$1/second-entered"
    while [ ! -f "$1/release-second" ]; do /bin/sleep 0.01; done
fi
"#,
        )
        .expect("write first-only native post-hook");
        Self(root)
    }

    fn command(&self, script: &str) -> String {
        let quote =
            |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
        format!(
            "/bin/sh {} {}",
            quote(&self.0.path().join(script)),
            quote(self.0.path())
        )
    }

    fn configuration(&self) -> serde_json::Value {
        serde_json::json!({"hooks": {
            "PostToolUse": [{"matcher": "^update_goal$", "hooks": [{"type": "command", "command": self.command("post.sh")}]}]
        }})
    }

    fn mark(&self, name: &str) {
        std::fs::write(self.0.path().join(name), b"ready\n").expect("release native identity gate");
    }

    fn release(&self) {
        self.mark("allow-duplicate");
        self.mark("release");
        self.mark("release-second");
    }
}

impl Drop for Hooks {
    fn drop(&mut self) {
        self.release();
    }
}

fn wait_for_duplicate_release(root: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !root.join("entered").is_file() || !root.join("allow-duplicate").is_file() {
        assert!(
            std::time::Instant::now() < deadline,
            "nested PostToolUse or duplicate release marker never arrived"
        );
        std::thread::sleep(Duration::from_millis(10));
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

async fn scenario(
    h: &Harness,
    goals: &GoalService,
    hooks: &Hooks,
    observed: &Observed,
    case: Case,
    progress: &test_progress::Progress,
) {
    let rt = Arc::clone(&h.rt);
    progress.phase("hold automatic evaluation");
    let evaluation = rt.hold_eval_when_free().await;
    assert!(
        h.test
            .codex
            .thread_extension_data()
            .get::<codex_core::HostToolInvocationLookup>()
            .is_none(),
        "this native configuration has no opt-in V2 active-ID reservation"
    );
    progress.phase("seed original goal");
    let outcome = goals
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
        .expect("seed the actual original goal");
    let db = h.test.codex.state_db().expect("native database");
    progress.phase("read setup goal");
    let setup_goal = db
        .thread_goals()
        .get_thread_goal(h.rt.id)
        .await
        .expect("read setup goal")
        .expect("setup goal exists");
    progress.phase("persist setup continuation deferral");
    db.thread_goals()
        .replace_thread_goal_snapshot(&setup_goal)
        .await
        .expect("persist setup-only continuation deferral");
    progress.phase("apply setup exact effect");
    outcome.apply_runtime_effects(goals).await;
    progress.phase("clear setup continuation deferral");
    db.thread_goals()
        .clear_thread_goal_continuation_deferral(h.rt.id)
        .await
        .expect("clear setup-only continuation deferral");
    let runtime = h
        .test
        .codex
        .thread_extension_data()
        .get::<codex_goal_extension::GoalRuntimeHandle>()
        .expect("native goal runtime");
    assert!(
        !runtime.alab_goal_persistence_held(),
        "the setup-only continuation deferral must settle mutation custody"
    );
    progress.phase("restore native accounting");
    runtime
        .restore_after_resume()
        .await
        .expect("restore native accounting");
    progress.phase("read original goal");
    let original = goal(h).await;
    if matches!(case, Case::OriginFirst) {
        hooks.mark("hold-second");
    }
    progress.phase("start mixed native turn");
    h.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Continue the original compiler goal and reassess its obstacle.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("start actual mixed CodeMode/native turn");
    progress.phase("await nested PostToolUse entry");
    wait_for_supervisor(|| {
        hooks.0.path().join("entered").is_file()
            || lock(&observed.finished).iter().any(|call| call.nested)
    })
    .await;
    if !hooks.0.path().join("entered").is_file() {
        let early = lock(&observed.finished)
            .iter()
            .find(|call| call.nested)
            .cloned()
            .expect("nested completion was observed");
        panic!(
            "nested goal finished before PostToolUse entry: {:?}",
            early.outcome
        );
    }
    progress.phase("check committed nested goal");
    assert_eq!(goal(h).await.status, codex_state::ThreadGoalStatus::Blocked);
    assert!(
        lock(&h.rt.model_blocked_goal).is_none(),
        "A committed before its post-hook callback"
    );
    let a_epoch = goals
        .alab_model_goal_epoch(h.rt.id)
        .expect("A's actual native commit epoch");
    let before_b = goal(h).await;
    progress.phase("release provider duplicate");
    hooks.mark("allow-duplicate");
    if matches!(case, Case::OriginFirst) {
        progress.phase("await direct PostToolUse entry");
        wait_for_supervisor(|| {
            hooks.0.path().join("second-entered").is_file()
                || lock(&observed.finished).iter().any(|call| !call.nested)
        })
        .await;
        if !hooks.0.path().join("second-entered").is_file() {
            let failed = lock(&observed.finished)
                .iter()
                .find(|call| !call.nested)
                .cloned()
                .expect("direct B finished before its post-hook");
            observed.provider.notify_one();
            progress.phase("await direct failure provider output");
            while responses::received_responses_requests(&h._server)
                .await
                .len()
                < 3
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let requests = responses::received_responses_requests(&h._server).await;
            panic!(
                "direct B did not reach PostToolUse: {:?}; provider output: {:?}",
                failed.outcome,
                requests
                    .last()
                    .and_then(|request| request.function_call_output_text(&failed.id))
            );
        }
        progress.phase("release nested hook before direct callback");
        hooks.mark("release");
        wait_for_supervisor(|| lock(&observed.finished).len() == 1).await;
        assert!(
            lock(&observed.finished)[0].nested,
            "A's callback precedes B's acceptance"
        );
        progress.phase("release direct hook");
        hooks.mark("release-second");
        wait_for_supervisor(|| lock(&observed.finished).len() == 2).await;
        observed.provider.notify_one();
    } else {
        progress.phase("await direct completion before nested hook");
        wait_for_supervisor(|| lock(&observed.finished).len() == 1).await;
        {
            let finished = lock(&observed.finished);
            assert!(!finished[0].nested, "direct B finishes before nested A");
            let expected = if case.direct_fails() {
                ToolCallOutcome::Failed {
                    handler_executed: true,
                }
            } else {
                ToolCallOutcome::Completed { success: true }
            };
            assert_eq!(finished[0].outcome, Some(expected));
        }
        if case.direct_fails() {
            progress.phase("check direct validation failure custody");
            assert_eq!(
                goals.alab_model_goal_epoch(h.rt.id),
                Some(a_epoch),
                "invalid status never reaches the native mutation boundary"
            );
            assert_eq!(
                goal(h).await,
                before_b,
                "B's validation failure cannot change A's committed goal"
            );
        }
        if case.nested_aborts() {
            progress.phase("await provider request before native interrupt");
            observed.provider.notify_one();
            while responses::received_responses_requests(&h._server)
                .await
                .len()
                < 3
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            progress.phase("interrupt nested native hook");
            h.test
                .codex
                .submit(codex_protocol::protocol::Op::Interrupt)
                .await
                .expect("interrupt A's actual pending CodeMode post-hook");
            progress.phase("await nested aborted callback");
            wait_for_supervisor(|| lock(&observed.finished).len() == 2).await;
        } else {
            progress.phase("release nested hook before provider completion");
            hooks.mark("release");
            wait_for_supervisor(|| lock(&observed.finished).len() == 2).await;
            observed.provider.notify_one();
        }
    }
    progress.phase("await native terminal event");
    core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| {
            if case.nested_aborts() {
                matches!(event, EventMsg::TurnAborted(_))
            } else {
                matches!(event, EventMsg::TurnComplete(_))
            }
        },
        NATIVE_HANG,
    )
    .await;
    progress.phase("await native idle state");
    while h.test.codex.has_active_turn().await {
        tokio::task::yield_now().await;
    }
    wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
    progress.phase("check native callback identities");
    let starts = lock(&observed.calls).clone();
    let finished = lock(&observed.finished).clone();
    assert_eq!(starts.len(), 2, "both actual native handlers ran");
    assert!(starts[0].nested);
    assert!(!starts[1].nested);
    assert!(
        starts[0].id.starts_with("exec-"),
        "the nested UUID is host-generated"
    );
    assert_eq!(
        starts[0].id, starts[1].id,
        "the provider reused the observed native UUID"
    );
    assert_eq!(starts[0].turn, starts[1].turn);
    assert_eq!(finished[0].nested, matches!(case, Case::OriginFirst));
    assert_ne!(finished[0].nested, finished[1].nested);
    assert_eq!(finished[0].id, finished[1].id);
    assert_eq!(finished[0].turn, finished[1].turn);
    for call in &finished {
        let expected = match (case, call.nested) {
            (case, false) if case.direct_fails() => ToolCallOutcome::Failed {
                handler_executed: true,
            },
            (case, true) if case.nested_aborts() => ToolCallOutcome::Aborted,
            _ => ToolCallOutcome::Completed { success: true },
        };
        assert_eq!(
            call.outcome,
            Some(expected),
            "{case:?}: actual native completion"
        );
    }
    progress.phase("check settled blocked goal");
    let blocked = goal(h).await;
    assert_eq!(blocked.goal_id, original.goal_id);
    assert_eq!(blocked.objective, original.objective);
    assert_eq!(blocked.token_budget, original.token_budget);
    assert_eq!(blocked.status, codex_state::ThreadGoalStatus::Blocked);
    assert!(
        !runtime.alab_goal_persistence_held(),
        "turn end must settle same-goal tokens that arrived after the terminal dispatch"
    );
    if case.direct_fails() {
        let accounted = blocked.tokens_used - original.tokens_used;
        assert!(
            matches!(accounted, 10 | 30),
            "the first callback is charged before dispatch or both callbacks are retained after dispatch; accounted={accounted}",
        );
    }
    assert_eq!(
        responses::received_responses_requests(&h._server)
            .await
            .len(),
        3
    );
    progress.phase("read original native model proof");
    let source = h
        .autopilot
        .current_model_binding(&h.rt)
        .await
        .expect("read actual native proof")
        .expect("a provider call-ID collision must not overwrite the original committed source");
    assert_eq!(source.goal.goal_id, original.goal_id);
    assert_eq!(source.goal.objective, original.objective);
    assert_eq!(source.goal.token_budget, original.token_budget);
    assert_eq!(source.goal.turn, starts[0].turn);
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    drop(evaluation);
    h.rt.with_state(|state| {
        state.point.as_mut().expect("completed native point").at =
            clock() - Duration::from_secs(3601)
    });
    progress.phase("stage recovery decision");
    h.immediate_point().await;
    progress.phase("evaluate native model-goal recovery");
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    progress.phase("await recovered provider request");
    while responses::received_responses_requests(&h._server)
        .await
        .len()
        < 4
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    progress.phase("check recovered goal");
    let resumed = goal(h).await;
    assert_eq!(resumed.goal_id, original.goal_id);
    assert_eq!(resumed.objective, original.objective);
    assert_eq!(resumed.token_budget, original.token_budget);
    assert_eq!(resumed.tokens_used, blocked.tokens_used);
    assert_eq!(resumed.status, codex_state::ThreadGoalStatus::Active);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
    progress.phase("await recovered native turn start");
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
    assert!(
        responses::received_responses_requests(&h._server).await[3]
            .body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE)
    );
    progress.phase("scenario complete");
}

async fn run(case: Case) {
    let progress = test_progress::Progress::default();
    progress.case(format!("{case:?}"));
    progress.phase("create native harness");
    let hooks = Hooks::new();
    let observed = Arc::new(Observed::default());
    let goals = Arc::new(GoalService::new());
    let mut h = Harness::new_with_accounted_goals_code_mode_and_hooks(
        Arc::clone(&goals),
        HookFixture {
            configuration: hooks.configuration(),
            observer: Arc::clone(&observed) as Arc<dyn ToolLifecycleContributor>,
        },
    )
    .await;
    progress.phase("observe native policy");
    h.fake.set_policy_key("upgrade", "false");
    h.fake.set_policy_key("autopilot", "cruise");
    wait_for_supervisor(|| {
        h.autopilot
            .sup
            .policy()
            .is_some_and(|policy| !policy.upgrade && policy.autopilot == decide::Mode::Cruise)
    })
    .await;
    let response_count = Arc::new(AtomicUsize::new(0));
    let response_source = Arc::clone(&observed);
    let response_gate = hooks.0.path().to_path_buf();
    progress.phase("install controlled provider responses");
    responses::mount_response_handler(&h._server, move |_| {
            match response_count.fetch_add(1, Ordering::SeqCst) {
                0 => responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("identity-nested"),
                    responses::ev_custom_tool_call("identity-exec", "exec", "// @exec: {\"yield_time_ms\": 1}\ntext(await tools.update_goal({status: \"blocked\"}));"),
                    responses::ev_completed_with_tokens("identity-nested", 10),
                ])),
                1 => {
                    let actual = response_source.nested_call();
                    wait_for_duplicate_release(&response_gate);
                    responses::sse_response(responses::sse(vec![
                        responses::ev_response_created("identity-direct"),
                        responses::ev_function_call(&actual.id, codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                            if case.direct_fails() { r#"{"status":"active"}"# } else { r#"{"status":"blocked"}"# }),
                        responses::ev_completed_with_tokens("identity-direct", 20),
                    ]))
                }
                2 if case.nested_aborts() => wiremock::ResponseTemplate::new(200).set_delay(Duration::from_secs(60)),
                2 => responses::sse_response(responses::sse(vec![
                    responses::ev_assistant_message("identity-final", "The temporary validation file is missing, so I stopped the goal."),
                    responses::ev_completed("identity-done"),
                ])),
                _ => wiremock::ResponseTemplate::new(200).set_delay(Duration::from_secs(60)),
            }
        }).await;
    let result = {
        let mut pending = Box::pin(tokio::time::timeout(
            NATIVE_HANG,
            scenario(&h, &goals, &hooks, &observed, case, &progress),
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
    let failed_progress = progress.describe();
    if !matches!(&result, Ok(Ok(()))) {
        let markers = [
            "hook-started",
            "hook-stdin-drained",
            "first-post",
            "entered",
            "allow-duplicate",
            "hold-second",
            "second-entered",
            "release",
            "release-second",
        ]
        .into_iter()
        .map(|name| (name, hooks.0.path().join(name).exists()))
        .collect::<std::collections::BTreeMap<_, _>>();
        let calls = |items: &Mutex<Vec<Call>>| {
            lock(items)
                .iter()
                .map(|call| {
                    serde_json::json!({
                        "nested": call.nested, "id": call.id, "turn": call.turn,
                        "outcome": format!("{:?}", call.outcome),
                    })
                })
                .collect::<Vec<_>>()
        };
        let evidence = serde_json::json!({
            "case": format!("{case:?}"), "phase": failed_progress,
            "hook_markers_before_cleanup": markers,
            "calls": calls(&observed.calls), "finished": calls(&observed.finished),
        });
        if let Err(error) = std::fs::write(
            hooks.0.path().join("failure-before-cleanup.json"),
            evidence.to_string(),
        ) {
            eprintln!("native identity failure evidence could not be retained: {error}");
        }
        h.retain_artifacts();
        h.retain_artifact_root("hook", hooks.0.path());
    }
    hooks.release();
    observed.provider.notify_one();
    progress.phase("cancel fixture timer");
    Autopilot::cancel_timer(&h.rt).await;
    progress.phase("shutdown native fixture");
    h.shutdown_and_export()
        .await
        .expect("owned native shutdown");
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            panic!("native marker identity collision hang detector: {error}; {failed_progress}")
        }
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[test]
fn model_goal_observed_native_call_id_collision_preserves_the_original_source() {
    native_test(|| run(Case::DuplicateFirst));
}

#[test]
fn model_goal_observed_native_call_id_collision_preserves_an_origin_callback_arriving_first() {
    native_test(|| run(Case::OriginFirst));
}

#[test]
fn model_goal_observed_native_call_id_collision_failure_before_sql_cannot_consume_the_source() {
    native_test(|| run(Case::DirectFailure));
}

#[test]
fn model_goal_observed_native_call_id_collision_preserves_a_committed_aborted_source() {
    native_test(|| run(Case::NestedAborted));
}

#[test]
fn model_goal_observed_native_call_id_collision_failed_duplicate_cannot_strand_aborted_usage() {
    native_test(|| run(Case::DirectFailureNestedAborted));
}
