// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Claimed native completion must finish its existing observer delivery after waiter teardown.

use super::*;
use codex_alab_core::goal_persistence::MutationEpoch;
use codex_alab_core::model_goal_invocation;
use codex_alab_core::model_goal_invocation::Invocation;
use codex_extension_api::ToolName;
use codex_protocol::protocol::TurnAbortReason;
use codex_thread_store::ThreadStoreError;
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;

const CALL: &str = "partial-finish-model-block";
const HELD_ITEM: &str = "The original model stopped at a temporary compiler obstacle.";
type Binding = model_blocked_goal::ModelGoalBinding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Started,
    FinishEntered,
    ItemEntered,
    ItemDropped,
    ItemReturned,
    FinishDropped,
    FinishReturned,
    PostAuto,
    TurnAbort,
    TurnStop,
    ThreadStop,
    SuspensionCallerDropped,
}

#[derive(Clone, Debug)]
struct Start {
    turn: String,
    invocation: Invocation,
    epoch: MutationEpoch,
}

#[derive(Clone, Debug)]
struct Finish {
    outcome: ToolCallOutcome,
    invocation: Invocation,
    epoch: MutationEpoch,
}

#[derive(Clone, Debug)]
struct Delivered {
    finish: Finish,
    binding: Option<Binding>,
}

struct Observed {
    log: Mutex<Vec<Step>>,
    starts: Mutex<Vec<Start>>,
    entered: Mutex<Vec<Finish>>,
    returned: Mutex<Vec<Finish>>,
    delivered: Mutex<Vec<Delivered>>,
    stopped: Mutex<Vec<Option<Binding>>>,
    capture_shared: OnceLock<&'static codex_alab_core::session::Shared>,
    saved_source: Mutex<Option<codex_alab_core::session::WallGoal>>,
    item_entries: AtomicUsize,
    item_release: tokio::sync::Semaphore,
    finish_release: tokio::sync::Semaphore,
}

impl Default for Observed {
    fn default() -> Self {
        Self {
            log: Mutex::new(Vec::new()),
            starts: Mutex::new(Vec::new()),
            entered: Mutex::new(Vec::new()),
            returned: Mutex::new(Vec::new()),
            delivered: Mutex::new(Vec::new()),
            stopped: Mutex::new(Vec::new()),
            capture_shared: OnceLock::new(),
            saved_source: Mutex::new(None),
            item_entries: AtomicUsize::new(0),
            item_release: tokio::sync::Semaphore::new(0),
            finish_release: tokio::sync::Semaphore::new(0),
        }
    }
}

impl Observed {
    fn saw(&self, step: Step) -> bool {
        lock(&self.log).contains(&step)
    }

    fn release(&self) {
        self.item_release.add_permits(1);
        self.finish_release.add_permits(1);
    }
}

/// Each callback owns its return flag. A later invocation cannot hide an earlier drop.
struct Callback<'a> {
    observed: &'a Observed,
    dropped: Step,
    returned: bool,
}

impl Drop for Callback<'_> {
    fn drop(&mut self) {
        if !self.returned {
            lock(&self.observed.log).push(self.dropped);
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
            assert_eq!(self.item_entries.fetch_add(1, Ordering::SeqCst), 0);
            let mut callback = Callback {
                observed: self,
                dropped: Step::ItemDropped,
                returned: false,
            };
            lock(&self.log).push(Step::ItemEntered);
            let _release = self.item_release.acquire().await.expect("owned item gate");
            callback.returned = true;
            lock(&self.log).push(Step::ItemReturned);
        })
    }

    fn on_turn_abort<'a>(&'a self, _input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move { lock(&self.log).push(Step::TurnAbort) })
    }

    fn on_turn_stop<'a>(&'a self, _input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move { lock(&self.log).push(Step::TurnStop) })
    }
}

impl ThreadLifecycleContributor<Config> for Observed {
    fn on_thread_stop<'a>(&'a self, input: ThreadStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let rt = Autopilot::rt(input.thread_store).expect("the retiring native thread");
            lock(&self.stopped).push(lock(&rt.model_blocked_goal).clone());
            lock(&self.log).push(Step::ThreadStop);
        })
    }
}

struct BeforeAuto {
    observed: Arc<Observed>,
    goals: Arc<GoalService>,
}

fn is_goal_call(call: &str, name: &ToolName) -> bool {
    call == CALL
        && name.is_default_namespace()
        && name.name == codex_goal_extension::UPDATE_GOAL_TOOL_NAME
}

fn finish_fact(goals: &GoalService, input: &ToolFinishInput<'_>) -> Finish {
    let rt = Autopilot::rt(input.thread_store).expect("the native registered thread");
    Finish {
        outcome: input.outcome,
        invocation: model_goal_invocation::current().expect("the actual native dispatch scope"),
        epoch: goals
            .alab_model_goal_epoch(rt.id)
            .expect("the native goal owner"),
    }
}

impl ToolLifecycleContributor for BeforeAuto {
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !is_goal_call(input.call_id, input.tool_name) {
                return;
            }
            let rt = Autopilot::rt(input.thread_store).expect("the native registered thread");
            lock(&self.observed.starts).push(Start {
                turn: input.turn_id.to_owned(),
                invocation: model_goal_invocation::current()
                    .expect("the actual native start scope"),
                epoch: self
                    .goals
                    .alab_model_goal_epoch(rt.id)
                    .expect("the native goal owner"),
            });
            lock(&self.observed.log).push(Step::Started);
        })
    }

    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !is_goal_call(input.call_id, input.tool_name) {
                return;
            }
            let mut callback = Callback {
                observed: &self.observed,
                dropped: Step::FinishDropped,
                returned: false,
            };
            lock(&self.observed.entered).push(finish_fact(&self.goals, &input));
            lock(&self.observed.log).push(Step::FinishEntered);
            let _release = self
                .observed
                .finish_release
                .acquire()
                .await
                .expect("owned finish gate");
            lock(&self.observed.returned).push(finish_fact(&self.goals, &input));
            callback.returned = true;
            lock(&self.observed.log).push(Step::FinishReturned);
        })
    }
}

struct AfterAuto {
    observed: Arc<Observed>,
    goals: Arc<GoalService>,
}

impl ToolLifecycleContributor for AfterAuto {
    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !is_goal_call(input.call_id, input.tool_name) {
                return;
            }
            let rt = Autopilot::rt(input.thread_store).expect("the native registered thread");
            if let Some(shared) = self.observed.capture_shared.get() {
                let pace = shared
                    .pace(rt.key())
                    .expect("the actual published source pace");
                let encoded = serde_json::to_vec(&codex_alab_tui::record::Pace::from(pace))
                    .expect("encode the published record pace");
                let decoded: codex_alab_tui::record::Pace =
                    serde_json::from_slice(&encoded).expect("read the published record pace");
                *lock(&self.observed.saved_source) = Some(
                    codex_alab_core::session::Pace::from(decoded)
                        .model_blocked_goal
                        .expect("the actual published source survives its record codec"),
                );
            }
            lock(&self.observed.delivered).push(Delivered {
                finish: finish_fact(&self.goals, &input),
                binding: lock(&rt.model_blocked_goal).clone(),
            });
            lock(&self.observed.log).push(Step::PostAuto);
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Interrupted,
    Normal,
    ApiPause,
    Failed,
    Suspend,
    SuspendApiPause,
    SuspendCallerDrop,
    SuspendCallerDropApiPause,
}

fn ordered(log: &[Step], earlier: Step, later: Step) {
    let position = |step| {
        log.iter()
            .position(|value| *value == step)
            .expect("native lifecycle step")
    };
    assert!(
        position(earlier) < position(later),
        "native lifecycle: {log:?}"
    );
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
        .expect("actual host API pause");
    h.test
        .codex
        .append_rollout_items(&[outcome.thread_goal_updated_item()])
        .await
        .expect("persist the actual API outcome");
    outcome.apply_runtime_effects(goals).await;
    h.test
        .codex
        .flush_rollout()
        .await
        .expect("flush the actual API pause");
}

async fn suspend_claimed(
    h: &Harness,
    goals: &GoalService,
    observed: &Observed,
    start: &Start,
    entered: &Finish,
    drop_caller: bool,
) {
    let before = goal(h).await;
    let epoch = goals
        .alab_model_goal_epoch(h.rt.id)
        .expect("the native owner before suspension");
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    let requests = responses::received_responses_requests(&h._server)
        .await
        .len();
    assert_eq!(requests, 1, "only the original provider request was sent");
    let rollout = h
        .test
        .codex
        .rollout_path()
        .expect("the actual native rollout");
    let mut suspension = Box::pin(h.test.codex.suspend_turn_and_shutdown());
    tokio::select! {
        outcome = suspension.as_mut() => panic!("suspension finished before the held delivery: {outcome:?}"),
        () = wait_for_supervisor(|| observed.saw(Step::ItemDropped)) => {}
    }
    // Poll the one public request again after native forced teardown. Its accepted
    // operation remains session-owned if an assertion drops this caller.
    std::future::poll_fn(
        |context| match std::future::Future::poll(suspension.as_mut(), context) {
            std::task::Poll::Pending => std::task::Poll::Ready(()),
            std::task::Poll::Ready(outcome) => {
                panic!("suspension skipped the held claimed delivery: {outcome:?}")
            }
        },
    )
    .await;
    assert!(!observed.saw(Step::ItemReturned));
    assert!(!observed.saw(Step::FinishDropped));
    assert!(!observed.saw(Step::FinishReturned));
    assert!(!observed.saw(Step::PostAuto));
    assert!(!observed.saw(Step::ThreadStop));
    assert!(!observed.saw(Step::TurnAbort));
    assert!(!observed.saw(Step::TurnStop));
    assert!(lock(&h.rt.model_blocked_goal).is_none());
    assert_eq!(goal(h).await.goal_id, before.goal_id);
    assert_eq!(goals.alab_model_goal_epoch(h.rt.id), Some(epoch.clone()));
    if drop_caller {
        drop(suspension);
        lock(&observed.log).push(Step::SuspensionCallerDropped);
        let mut terminated = Box::pin(h.test.codex.wait_until_terminated());
        std::future::poll_fn(|context| {
            match std::future::Future::poll(terminated.as_mut(), context) {
                std::task::Poll::Pending => std::task::Poll::Ready(()),
                std::task::Poll::Ready(()) => {
                    panic!("caller drop terminated the owner before its held delivery")
                }
            }
        })
        .await;
        assert!(!observed.saw(Step::FinishDropped));
        assert!(!observed.saw(Step::FinishReturned));
        assert!(!observed.saw(Step::PostAuto));
        assert!(!observed.saw(Step::ThreadStop));
        observed.finish_release.add_permits(1);
        terminated.await;
        // The original reply receiver is gone. Observe native successful cleanup
        // without sending another operation that could complete it for the caller.
        tokio::time::timeout(HANG, async {
            loop {
                let event = h
                    .test
                    .codex
                    .next_event()
                    .await
                    .expect("native suspension event");
                match &event.msg {
                    EventMsg::ShutdownComplete => break,
                    EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_) => {
                        panic!("nonterminal suspension emitted a terminal turn event: {event:?}")
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("accepted caller-drop suspension must announce native ShutdownComplete");
    } else {
        observed.finish_release.add_permits(1);
        let outcome = suspension
            .await
            .expect("suspend the actual unfinished root turn");
        assert_eq!(
            outcome,
            codex_core::SuspendTurnOutcome::Suspended {
                turn_id: start.turn.clone()
            }
        );
    }
    let returned = lock(&observed.returned).clone();
    let delivered = lock(&observed.delivered).clone();
    let stopped = lock(&observed.stopped).clone();
    let log = lock(&observed.log).clone();
    assert_eq!(lock(&observed.starts).len(), 1);
    assert_eq!(lock(&observed.entered).len(), 1);
    assert_eq!(returned.len(), 1);
    assert_eq!(delivered.len(), 1);
    assert_eq!(stopped.len(), 1);
    assert_eq!(
        entered.outcome,
        ToolCallOutcome::Completed { success: true }
    );
    assert_eq!(returned[0].outcome, entered.outcome);
    assert_eq!(delivered[0].finish.outcome, entered.outcome);
    assert_eq!(entered.invocation, start.invocation);
    assert_eq!(returned[0].invocation, start.invocation);
    assert_eq!(delivered[0].finish.invocation, start.invocation);
    assert_eq!(returned[0].epoch, epoch);
    assert_eq!(delivered[0].finish.epoch, epoch);
    assert_eq!(stopped[0], delivered[0].binding);
    if before.status == ThreadGoalStatus::Blocked {
        let binding = delivered[0]
            .binding
            .as_ref()
            .expect("the original completed source was delivered before owner retirement");
        assert_eq!(binding.goal.goal_id, before.goal_id);
        assert_eq!(binding.goal.objective, before.objective);
        assert_eq!(binding.goal.token_budget, before.token_budget);
        assert_eq!(binding.goal.turn, start.turn);
        assert_eq!(binding.epoch, entered.epoch);
        assert_eq!(binding.epoch, epoch);
    } else {
        assert_eq!(before.status, ThreadGoalStatus::Paused);
        assert!(delivered[0].binding.is_none());
        assert!(
            epoch != entered.epoch,
            "the actual API pause revoked the native source"
        );
    }
    assert!(!log.contains(&Step::ItemReturned));
    assert!(!log.contains(&Step::FinishDropped));
    assert!(!log.contains(&Step::TurnAbort));
    assert!(!log.contains(&Step::TurnStop));
    ordered(&log, Step::Started, Step::FinishEntered);
    ordered(&log, Step::FinishEntered, Step::ItemDropped);
    ordered(&log, Step::ItemDropped, Step::FinishReturned);
    if drop_caller {
        ordered(&log, Step::ItemDropped, Step::SuspensionCallerDropped);
        ordered(&log, Step::SuspensionCallerDropped, Step::FinishReturned);
    }
    ordered(&log, Step::FinishReturned, Step::PostAuto);
    ordered(&log, Step::PostAuto, Step::ThreadStop);
    assert_eq!(goals.alab_model_goal_epoch(h.rt.id), None);
    let after = goal(h).await;
    assert_eq!(after.goal_id, before.goal_id);
    assert_eq!(after.objective, before.objective);
    assert_eq!(after.status, before.status);
    assert_eq!(after.token_budget, before.token_budget);
    assert_eq!(after.tokens_used, before.tokens_used);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
    assert_eq!(
        responses::received_responses_requests(&h._server)
            .await
            .len(),
        requests
    );
    let (items, thread_id, parse_errors) =
        codex_core::RolloutRecorder::load_rollout_items(&rollout)
            .await
            .expect("read the closed actual native rollout");
    assert_eq!(thread_id, Some(h.rt.id));
    assert_eq!(parse_errors, 0);
    let mut original_starts = items
        .iter()
        .filter(|item| matches!(item, codex_history::RolloutItem::EventMsg(EventMsg::TurnStarted(turn)) if turn.turn_id == start.turn));
    let started = original_starts
        .next()
        .expect("the exact unfinished turn is durable");
    assert!(
        original_starts.next().is_none(),
        "the original turn is recorded as started exactly once"
    );
    assert!(
        items.iter().all(|item| !matches!(
            item,
            codex_history::RolloutItem::EventMsg(
                EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_)
            )
        )),
        "suspension closes this fresh root rollout without a terminal turn event"
    );
    let error = h
        .test
        .codex
        .append_rollout_items(std::slice::from_ref(started))
        .await
        .expect_err("a nonempty append must reach the closed native writer");
    assert!(
        matches!(error, ThreadStoreError::ThreadNotFound { thread_id } if thread_id == h.rt.id)
    );
}

async fn scenario(h: &Harness, goals: &GoalService, observed: &Observed, case: Case) {
    let evaluation = h.rt.hold_eval_when_free().await;
    let original = seed(h, goals).await;
    if matches!(case, Case::Failed) {
        pause_with_native_api(h, goals).await;
    }
    let baseline = goal(h).await;
    let original_epoch = goals
        .alab_model_goal_epoch(h.rt.id)
        .expect("the original native owner");
    let arguments = if matches!(case, Case::Failed) {
        r#"{"status":"active"}"#
    } else {
        r#"{"status":"blocked"}"#
    };
    let mut bodies = vec![responses::sse(vec![
        responses::ev_response_created("partial-native-finish"),
        responses::ev_function_call(CALL, codex_goal_extension::UPDATE_GOAL_TOOL_NAME, arguments),
        responses::ev_assistant_message("partial-finish-held-item", HELD_ITEM),
        responses::ev_completed_with_tokens("partial-native-finish", 10),
    ])];
    if matches!(case, Case::Normal) {
        bodies.push(responses::sse(vec![
            responses::ev_assistant_message(
                "partial-finish-final",
                "The temporary compiler artifact is absent.",
            ),
            responses::ev_completed("partial-finish-final"),
        ]));
    }
    responses::mount_sse_sequence(&h._server, bodies).await;
    h.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Continue the original compiler goal and report the current obstacle.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("start the real native goal tool");
    wait_for_supervisor(|| observed.saw(Step::FinishEntered) && observed.saw(Step::ItemEntered))
        .await;
    let starts = lock(&observed.starts).clone();
    let entered = lock(&observed.entered).clone();
    assert_eq!(starts.len(), 1, "one actual native goal invocation");
    assert_eq!(
        entered.len(),
        1,
        "registry claimed the normal terminal callback once"
    );
    assert_eq!(starts[0].epoch, original_epoch);
    assert_eq!(starts[0].invocation, entered[0].invocation);
    if matches!(case, Case::Failed) {
        assert_eq!(
            entered[0].outcome,
            ToolCallOutcome::Failed {
                handler_executed: true
            }
        );
        assert_eq!(
            entered[0].epoch, starts[0].epoch,
            "the rejected native status never reaches a mutation"
        );
    } else {
        assert_eq!(
            entered[0].outcome,
            ToolCallOutcome::Completed { success: true }
        );
        assert!(
            entered[0].epoch != starts[0].epoch,
            "the native model mutation committed"
        );
    }
    assert_eq!(
        goals.alab_model_goal_epoch(h.rt.id),
        Some(entered[0].epoch.clone())
    );
    let committed = goal(h).await;
    assert_eq!(committed.goal_id, original.goal_id);
    assert_eq!(committed.objective, original.objective);
    assert_eq!(committed.token_budget, original.token_budget);
    if matches!(case, Case::Failed) {
        assert_eq!(committed.status, ThreadGoalStatus::Paused);
        assert_eq!(
            committed, baseline,
            "UserInput and the invalid native tool preserve the paused row"
        );
    } else {
        assert_eq!(committed.status, ThreadGoalStatus::Blocked);
    }
    assert!(
        lock(&h.rt.model_blocked_goal).is_none(),
        "Auto is after the held native observer"
    );
    assert!(lock(&observed.delivered).is_empty());
    assert_eq!(h.shared.person_goal_stops(h.rt.key()), 0);
    if matches!(
        case,
        Case::ApiPause | Case::SuspendApiPause | Case::SuspendCallerDropApiPause
    ) {
        pause_with_native_api(h, goals).await;
        assert_eq!(goal(h).await.status, ThreadGoalStatus::Paused);
        assert!(goals.alab_model_goal_epoch(h.rt.id).as_ref() != Some(&entered[0].epoch));
    }
    if matches!(
        case,
        Case::Suspend
            | Case::SuspendApiPause
            | Case::SuspendCallerDrop
            | Case::SuspendCallerDropApiPause
    ) {
        suspend_claimed(
            h,
            goals,
            observed,
            &starts[0],
            &entered[0],
            matches!(
                case,
                Case::SuspendCallerDrop | Case::SuspendCallerDropApiPause
            ),
        )
        .await;
        return;
    }
    if matches!(case, Case::Normal) {
        observed.release();
        core_test_support::wait_for_event_with_timeout(
            &h.test.codex,
            |event| matches!(event, EventMsg::TurnComplete(_)),
            HANG,
        )
        .await;
        wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
    } else {
        h.test
            .codex
            .submit(Op::Interrupt)
            .await
            .expect("request real native interruption without awaiting terminal delivery");
        wait_for_supervisor(|| observed.saw(Step::ItemDropped)).await;
        assert!(!observed.saw(Step::ItemReturned));
        assert!(
            !observed.saw(Step::FinishDropped),
            "the claimed finish remains owned through waiter Drop"
        );
        assert!(!observed.saw(Step::FinishReturned));
        assert!(!observed.saw(Step::PostAuto));
        assert!(
            !observed.saw(Step::TurnAbort),
            "native abort lifecycle waits for the claimed delivery"
        );
        if !matches!(case, Case::ApiPause) {
            assert_eq!(goal(h).await, committed);
            assert_eq!(
                goals.alab_model_goal_epoch(h.rt.id),
                Some(entered[0].epoch.clone())
            );
        }
        observed.finish_release.add_permits(1);
        let aborted = core_test_support::wait_for_event_with_timeout(
            &h.test.codex,
            |event| matches!(event, EventMsg::TurnAborted(_)),
            HANG,
        )
        .await;
        assert!(
            matches!(aborted, EventMsg::TurnAborted(event) if event.reason == TurnAbortReason::Interrupted)
        );
        wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
    }
    let returned = lock(&observed.returned).clone();
    let delivered = lock(&observed.delivered).clone();
    let log = lock(&observed.log).clone();
    assert_eq!(lock(&observed.starts).len(), 1);
    assert_eq!(lock(&observed.entered).len(), 1);
    assert_eq!(
        returned.len(),
        1,
        "no observer replay after transferring the original future"
    );
    assert_eq!(delivered.len(), 1, "one completed native delivery to Auto");
    assert_eq!(returned[0].outcome, entered[0].outcome);
    assert_eq!(delivered[0].finish.outcome, entered[0].outcome);
    assert_eq!(returned[0].invocation, starts[0].invocation);
    assert_eq!(delivered[0].finish.invocation, starts[0].invocation);
    assert_eq!(returned[0].epoch, delivered[0].finish.epoch);
    assert_eq!(
        goals.alab_model_goal_epoch(h.rt.id),
        Some(delivered[0].finish.epoch.clone())
    );
    if matches!(case, Case::Failed) {
        assert_eq!(returned[0].epoch, original_epoch);
    }
    assert!(
        !log.contains(&Step::FinishDropped),
        "the original observer future survived: {log:?}"
    );
    ordered(&log, Step::Started, Step::FinishEntered);
    ordered(&log, Step::FinishEntered, Step::FinishReturned);
    ordered(&log, Step::FinishReturned, Step::PostAuto);
    if matches!(case, Case::Normal) {
        ordered(&log, Step::ItemEntered, Step::ItemReturned);
        assert!(!log.contains(&Step::ItemDropped));
        assert!(!log.contains(&Step::TurnAbort));
        ordered(&log, Step::PostAuto, Step::TurnStop);
    } else {
        ordered(&log, Step::FinishEntered, Step::ItemDropped);
        ordered(&log, Step::ItemDropped, Step::FinishReturned);
        ordered(&log, Step::PostAuto, Step::TurnAbort);
        assert_eq!(
            log.iter().filter(|step| **step == Step::TurnAbort).count(),
            1
        );
    }
    let stopped = goal(h).await;
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    let requests_before = responses::received_responses_requests(&h._server)
        .await
        .len();
    if matches!(case, Case::ApiPause | Case::Failed) {
        assert_eq!(stopped.status, ThreadGoalStatus::Paused);
        assert!(delivered[0].binding.is_none());
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
    let binding = delivered[0].binding.as_ref().expect(
        "Auto accepted the original committed source after its preceding observer returned",
    );
    assert_eq!(binding.goal.goal_id, original.goal_id);
    assert_eq!(binding.goal.objective, original.objective);
    assert_eq!(binding.goal.token_budget, original.token_budget);
    assert_eq!(binding.goal.turn, starts[0].turn);
    assert_eq!(binding.epoch, entered[0].epoch);
    assert_eq!(returned[0].epoch, entered[0].epoch);
    assert_eq!(delivered[0].finish.epoch, entered[0].epoch);
    assert_eq!(
        goals.alab_model_goal_epoch(h.rt.id),
        Some(entered[0].epoch.clone())
    );
    assert_eq!(stopped.status, ThreadGoalStatus::Blocked);
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
        serde_json::to_value(started).expect("native goal attribution")["turn_attribution"]["turn_trigger"],
        "goal"
    );
    let requests = responses::received_responses_requests(&h._server).await;
    assert_eq!(
        requests.len(),
        requests_before + 1,
        "one goal-triggered continuation"
    );
    assert!(requests[requests_before].body_contains_text(OBJECTIVE));
    assert!(requests[requests_before].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
}

async fn prepared(case: Case) -> (Harness, Arc<GoalService>, Arc<Observed>) {
    let goals = Arc::new(GoalService::new());
    let observed = Arc::new(Observed::default());
    let before = Arc::new(BeforeAuto {
        observed: Arc::clone(&observed),
        goals: Arc::clone(&goals),
    });
    let after = Arc::new(AfterAuto {
        observed: Arc::clone(&observed),
        goals: Arc::clone(&goals),
    });
    let h = Harness::new_with_accounted_goals_and_lifecycle_observers(
        Arc::clone(&goals),
        before as Arc<dyn ToolLifecycleContributor>,
        after as Arc<dyn ToolLifecycleContributor>,
        Arc::clone(&observed) as Arc<dyn TurnLifecycleContributor>,
        matches!(
            case,
            Case::Suspend
                | Case::SuspendApiPause
                | Case::SuspendCallerDrop
                | Case::SuspendCallerDropApiPause
        )
        .then(|| Arc::clone(&observed) as Arc<dyn ThreadLifecycleContributor<Config>>),
    )
    .await;
    let (h, goals) = configured_harness(h, goals).await;
    (h, goals, observed)
}

type NativeResult = Result<Result<(), tokio::time::error::Elapsed>, Box<dyn std::any::Any + Send>>;

async fn checked(work: impl std::future::Future<Output = ()>) -> NativeResult {
    let mut pending = Box::pin(tokio::time::timeout(HANG, work));
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
}

async fn cleanup(h: &mut Harness, observed: Option<&Observed>, passed: bool) {
    if let Some(observed) = observed {
        observed.release();
    }
    if !passed {
        h.retain_artifacts();
        if let Some(observed) = observed {
            eprintln!(
                "native partial-delivery lifecycle: {:?}",
                *lock(&observed.log)
            );
        }
    }
    Autopilot::cancel_timer(&h.rt).await;
    if h.test.codex.has_active_turn().await {
        h.test
            .codex
            .submit(Op::Interrupt)
            .await
            .expect("interrupt owned cleanup");
    }
    h.shutdown_and_export()
        .await
        .expect("owned native partial-delivery shutdown");
}

fn resolve(result: NativeResult, label: &str) {
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("{label}: native partial-finish hang detector: {error}"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

async fn run(case: Case) {
    let (mut h, goals, observed) = prepared(case).await;
    let result = checked(scenario(&h, &goals, &observed, case)).await;
    cleanup(&mut h, Some(&observed), matches!(&result, Ok(Ok(())))).await;
    resolve(result, &format!("{case:?}"));
}

/// The record writer delivers this passive fact only after selecting the same-session
/// record. This callback seam supplies its genuine published codec value, never a
/// predecessor's native epoch, binding, accepted receipt, or invocation scope.
fn deliver_selected_same_session_record_source(
    h: &Harness,
    saved: codex_alab_core::session::WallGoal,
) {
    h.shared.carry_model_goal(h.rt.key(), Some(saved));
}

async fn no_imported_work(h: &Harness) {
    {
        let state = h.rt.state();
        assert!(
            state.point.is_none(),
            "unfinished native history supplies no idle point"
        );
        assert!(!state.carry_on);
        assert!(state.owed.is_none());
    }
    assert!(!h.shared.carry_on_owed());
    assert!(h.shared.owed_act().is_none());
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), 0);
    assert!(
        responses::received_responses_requests(&h._server)
            .await
            .is_empty()
    );
    assert!(!h.test.codex.has_active_turn().await);
}

fn suspended_source_history(
    items: &[codex_history::RolloutItem],
    saved: &codex_alab_core::session::WallGoal,
) {
    assert!(
        scan_history(items).0,
        "the actual history retains the original task"
    );
    let native = crate::resume_history::latest(items);
    assert_eq!(native.turn_id, Some(saved.turn.as_str()));
    assert!(matches!(
        native.point,
        crate::resume_history::NativePoint::Running
    ));
    assert!(crate::resume_history::unfinished_recovery_turn(
        items,
        &saved.turn
    ));
    assert_eq!(items.iter().filter(|item| matches!(item,
        codex_history::RolloutItem::ResponseItem(envelope)
            if matches!(&envelope.item, codex_protocol::models::ResponseItem::FunctionCall { call_id, name, .. }
                if call_id == CALL && name == codex_goal_extension::UPDATE_GOAL_TOOL_NAME)
    )).count(), 1, "the actual original goal call is durable exactly once");
    assert!(!items.iter().any(|item| matches!(item,
        codex_history::RolloutItem::ResponseItem(envelope)
            if matches!(&envelope.item, codex_protocol::models::ResponseItem::FunctionCallOutput { call_id, .. } if call_id.as_deref() == Some(CALL))
    )), "the dropped sampling owner never saved the original tool output");
}

async fn verify_suspended_import(
    h: &Harness,
    goals: &GoalService,
    saved: codex_alab_core::session::WallGoal,
    original: &codex_state::ThreadGoal,
    pause: bool,
) {
    wait_for_supervisor(|| h.rt.scanned.load(Ordering::SeqCst)).await;
    let evaluation = h.rt.hold_eval_when_free().await;
    assert!(h.rt.resumed.load(Ordering::SeqCst));
    let epoch = goals
        .alab_model_goal_epoch(h.rt.id)
        .expect("the fresh native owner");
    assert!(
        epoch.is_initial(),
        "resume creates a new unmutated native owner"
    );
    assert!(lock(&h.rt.model_blocked_goal).is_none());
    assert!(h.shared.carried_model_goal(h.rt.key()).is_none());
    // Scanned includes an attempted read before this fixture binds its manager.
    // Only a successful import below must prove books_read and restored task.
    no_imported_work(h).await;
    let rollout = h
        .test
        .codex
        .rollout_path()
        .expect("the real resumed rollout");
    let (items, thread_id, parse_errors) =
        codex_core::RolloutRecorder::load_rollout_items(&rollout)
            .await
            .expect("read the actual suspended history");
    assert_eq!(thread_id, Some(h.rt.id));
    assert_eq!(parse_errors, 0);
    suspended_source_history(&items, &saved);
    assert!(!crate::resume_history::model_goal_stop_revoked_since(
        &items, h.rt.id, &saved
    ));
    if pause {
        pause_with_native_api(h, goals).await;
        assert!(goals.alab_model_goal_epoch(h.rt.id).as_ref() != Some(&epoch));
        let history = resumed_history(&h.test.codex)
            .await
            .expect("the actual successor API pause history");
        assert!(
            crate::resume_history::model_goal_stop_revoked_since(&history, h.rt.id, &saved),
            "persisted history independently vetoes the real stale source"
        );
    }
    deliver_selected_same_session_record_source(h, saved.clone());
    let imported = h
        .autopilot
        .current_model_goal(&h.rt)
        .await
        .expect("validate the genuine recorded source");
    assert_eq!(
        imported.map(|binding| binding.goal),
        (!pause).then_some(saved)
    );
    if pause {
        assert!(lock(&h.rt.model_blocked_goal).is_none());
    } else {
        assert!(h.rt.books_read.load(Ordering::SeqCst));
        assert!(
            h.rt.state().task,
            "successful import restores the real history's task"
        );
        assert_eq!(
            lock(&h.rt.model_blocked_goal)
                .as_ref()
                .map(|binding| &binding.epoch),
            Some(&epoch)
        );
    }
    no_imported_work(h).await;
    drop(evaluation);
    evaluate(h).await;
    no_imported_work(h).await;
    let after = goal(h).await;
    assert_eq!(after.thread_id, original.thread_id);
    assert_eq!(after.thread_id, h.rt.id);
    assert_eq!(after.goal_id, original.goal_id);
    assert_eq!(after.objective, original.objective);
    assert_eq!(after.token_budget, original.token_budget);
    assert_eq!(after.tokens_used, original.tokens_used);
    assert_eq!(
        after.status,
        if pause {
            ThreadGoalStatus::Paused
        } else {
            ThreadGoalStatus::Blocked
        }
    );
}

async fn run_suspended_import(pause: bool) {
    let (mut old, goals, observed) = prepared(Case::SuspendCallerDrop).await;
    let mut saved = None;
    let result = checked(async {
        assert!(observed.capture_shared.set(old.shared).is_ok());
        scenario(&old, &goals, &observed, Case::SuspendCallerDrop).await;
        let source = lock(&observed.saved_source)
            .clone()
            .expect("capture the actual published record source before retirement");
        assert_eq!(
            Some(&source),
            lock(&observed.delivered)[0]
                .binding
                .as_ref()
                .map(|binding| &binding.goal)
        );
        let rollout = old
            .test
            .codex
            .rollout_path()
            .expect("the closed original rollout");
        let (items, thread_id, parse_errors) =
            codex_core::RolloutRecorder::load_rollout_items(&rollout)
                .await
                .expect("read the closed original raw rollout before any resume");
        assert_eq!(thread_id, Some(old.rt.id));
        assert_eq!(parse_errors, 0);
        suspended_source_history(&items, &source);
        saved = Some((source, goal(&old).await));
    })
    .await;
    cleanup(&mut old, Some(&observed), matches!(&result, Ok(Ok(())))).await;
    resolve(result, "original suspended source");
    let (saved, original) = saved.expect("the original source completed its native checks");
    let old_shared = old.shared;
    let old_goals = Arc::downgrade(&goals);
    let home = Arc::clone(&old.test.home);
    let rollout = old
        .test
        .codex
        .rollout_path()
        .expect("the closed original rollout");
    drop(old);
    drop(goals);
    drop(observed);
    let goals = Arc::new(GoalService::new());
    let h =
        Harness::resume_with_accounted_goals_without_pending_act(Arc::clone(&goals), home, rollout)
            .await;
    let (mut h, goals) = configured_harness(h, goals).await;
    let result = checked(async {
        assert!(!std::ptr::eq(h.shared, old_shared));
        assert!(!std::sync::Weak::ptr_eq(
            &old_goals,
            &Arc::downgrade(&goals)
        ));
        verify_suspended_import(&h, &goals, saved, &original, pause).await;
    })
    .await;
    cleanup(&mut h, None, matches!(&result, Ok(Ok(())))).await;
    resolve(result, "fresh suspended-source import");
}

#[test]
fn a_claimed_tool_finish_survives_forced_waiter_teardown_without_replaying_its_outcome() {
    native_test(|| run(Case::Interrupted));
}

#[test]
fn a_normally_delivered_tool_finish_preserves_the_original_source_and_native_epoch() {
    native_test(|| run(Case::Normal));
}

#[test]
fn an_api_pause_during_claimed_tool_finish_defeats_its_later_source_delivery() {
    native_test(|| run(Case::ApiPause));
}

#[test]
fn a_claimed_native_failure_survives_waiter_teardown_without_becoming_a_model_stop() {
    native_test(|| run(Case::Failed));
}

#[test]
fn a_claimed_tool_finish_precedes_nonterminal_suspension_and_native_owner_retirement() {
    native_test(|| run(Case::Suspend));
}

#[test]
fn an_api_pause_during_claimed_tool_finish_survives_nonterminal_suspension() {
    native_test(|| run(Case::SuspendApiPause));
}

#[test]
fn accepted_suspension_survives_caller_drop_while_claimed_tool_finish_is_held() {
    native_test(|| run(Case::SuspendCallerDrop));
}

#[test]
fn an_api_pause_survives_suspension_caller_drop_during_claimed_tool_finish() {
    native_test(|| run(Case::SuspendCallerDropApiPause));
}

#[test]
fn a_fresh_successor_imports_the_real_suspended_source_without_inventing_an_idle_turn() {
    native_test(|| run_suspended_import(false));
}

#[test]
fn a_successor_api_pause_rejects_the_real_source_recorded_before_suspension() {
    native_test(|| run_suspended_import(true));
}
