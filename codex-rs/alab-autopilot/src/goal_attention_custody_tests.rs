// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A detached Active read cannot retire a later real Blocked goal's attention.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use core_test_support::responses;

const HANG: Duration = Duration::from_secs(60);

struct StartGate(
    Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
);

impl TurnLifecycleContributor for StartGate {
    fn on_turn_start<'a>(&'a self, _input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let gate = lock(&self.0).take();
            if let Some((entered, release)) = gate {
                entered.send(()).expect("native start observer remains");
                release.await.expect("native start release");
            }
        })
    }
}

async fn set_goal(
    h: &Harness,
    goals: &GoalService,
    status: codex_protocol::protocol::ThreadGoalStatus,
) -> codex_goal_extension::GoalSetOutcome {
    goals
        .set_thread_goal(
            &h.test.codex.state_db().expect("real goal database"),
            GoalSetRequest {
                thread_id: h.rt.id,
                objective: GoalObjectiveUpdate::Set("finish the original task"),
                status: Some(status),
                token_budget: GoalTokenBudgetUpdate::Keep,
                max_goal_token_budget: None,
            },
            async { Ok(()) },
        )
        .await
        .expect("real GoalService mutation")
}

#[test]
fn a_detached_active_read_keeps_a_later_blocked_goals_attention() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            for case in ["active-running", "active-complete", "same-words-running", "blocked-complete"] {
                let newer_blocked = case == "blocked-complete";
                let active_complete = case == "active-complete";
                let same_words = case == "same-words-running";
                let start_gate = Arc::new(StartGate(Mutex::new(None)));
                let goals = Arc::new(GoalService::new());
                // Without a goal runtime, the real Active database goal cannot
                // automatically launch a second turn in the completion control.
                let installed = (!active_complete).then(|| goals.clone());
                let h = Harness::new_with_goals_and_observer(installed, Some(start_gate.clone())).await;
                h.immediate_point().await;
                let _active = set_goal(&h, &goals, codex_protocol::protocol::ThreadGoalStatus::Active).await;
                h.autopilot.sup.attention_for(h.rt.key(), "goal", Some("goal: the previous stop".into())).await;
                responses::mount_sse_once(&h._server, responses::sse(vec![
                    responses::ev_response_created("goal-report"),
                    responses::ev_assistant_message("goal-report-message", "Continue working on the task."),
                    responses::ev_completed("goal-report"),
                ])).await;
                let (entered, mut entry) = tokio::sync::oneshot::channel();
                let (release_start, blocked_start) = tokio::sync::oneshot::channel();
                *lock(&start_gate.0) = Some((entered, blocked_start));
                let (read, reading) = tokio::sync::oneshot::channel();
                let (resume, resuming) = tokio::sync::oneshot::channel();
                let (finished, finishing) = tokio::sync::oneshot::channel();
                let mut resume = Some(resume);
                let mut finishing = Some(finishing);
                *lock(&h.rt.after_goal_read) = Some(GoalReadGate { read, resume: resuming, finished });
                h.shared.expect_own_turn("Continue the original task.");
                // Commit Active through the real service; this explicit native input
                // supplies its turn. The later Blocked mutation applies its effects too.
                let starting = h.test.codex.start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "Continue the original task.".into(), text_elements: Vec::new(),
                }]));
                tokio::pin!(starting);
                let accepted = tokio::select! {
                    started = &mut starting => { started.expect("native turn accepted"); true },
                    entered = &mut entry => { entered.expect("actual native turn held at start"); false },
                };
                if accepted {
                    entry.await.expect("actual native turn held at start");
                }
                wait_for_supervisor(|| h.shared.turn_running_on(h.rt.key())).await;
                assert_eq!(reading.await.expect("actual detached goal read"), Some(codex_state::ThreadGoalStatus::Active));
                let mut during = None;
                if !newer_blocked && !active_complete {
                    if same_words {
                        h.autopilot.sup.attention_for(h.rt.key(), "goal", Some("goal: the previous stop".into())).await;
                    }
                    resume.take().unwrap().send(()).expect("resume unchanged Active read");
                    finishing.take().unwrap().await.expect("unchanged Active publication finished");
                    during = h.autopilot.sup.standing_attention();
                }
                if !active_complete {
                    let blocked = set_goal(&h, &goals, codex_protocol::protocol::ThreadGoalStatus::Blocked).await;
                    blocked.apply_runtime_effects(&goals).await;
                }
                release_start.send(()).expect("release actual native turn");
                if !accepted {
                    starting.await.expect("native turn accepted");
                }
                core_test_support::wait_for_event_with_timeout(&h.test.codex, |event| matches!(event, EventMsg::TurnComplete(_)), HANG).await;
                wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
                if !active_complete {
                    wait_for_supervisor(|| h.fake.attention().get("codex-alab").is_some_and(|text| text.contains("goal is blocked"))).await;
                }
                let before = h.autopilot.sup.standing_attention();
                if newer_blocked || active_complete {
                    // Modeled preemption ends after actual native completion and,
                    // for Blocked, the evaluator's publication of its new reason.
                    resume.take().unwrap().send(()).expect("release stale Active result");
                    finishing.take().unwrap().await.expect("stale goal publication finished");
                }
                let after = h.autopilot.sup.standing_attention();
                let shown = h.fake.attention().get("codex-alab").cloned();
                h.test.codex.shutdown_and_wait().await.expect("owned native shutdown before verdict");
                if same_words {
                    assert_eq!(during.as_deref(), Some("goal: the previous stop"), "a later identical raise owns a new reason entry");
                } else if !newer_blocked && !active_complete {
                    assert!(during.is_none(), "an unchanged Active goal answers its earlier stop");
                }
                if active_complete {
                    assert_eq!(before.as_deref(), Some("goal: the previous stop"));
                    assert!(after.is_none(), "the same completed Active turn still answers its old stop");
                    assert!(shown.is_none());
                    continue;
                }
                assert!(before.as_deref().is_some_and(|text| text.contains("goal is blocked")), "the real evaluator published a fresh Blocked reason");
                assert_eq!(after, before, "an older Active read cannot clear the later Blocked goal");
                assert!(shown.as_deref().is_some_and(|text| text.contains("goal is blocked")), "the new Blocked goal remains on the real control transport");
            }
        }).await.expect("goal attention custody hang detector");
    });
}
