// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A detached Active read cannot retire a later real Blocked goal's attention.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::protocol::ThreadSettingsOverrides;
use core_test_support::responses;

const HANG: Duration = super::upgrade_handoff_tests::NATIVE_HANG;

struct StartGate(
    Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
);

impl TurnLifecycleContributor for StartGate {
    fn turn_start_phase(&self, _thread: &ExtensionData) -> codex_extension_api::TurnStartPhase {
        // Hold native work after registration so Core can answer the Active
        // effect's ordered idle request while the turn remains busy.
        codex_extension_api::TurnStartPhase::RegularTaskStart
    }

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
fn goal_attention_rechecks_a_goal_changed_while_the_wait_clears() {
    native_test(|| async {
        const CLEAR: &str = "meta unset attention owner=codex-alab-wait";
        const WAIT: &str = "The older automatic wait";
        for change in ["unchanged", "cleared", "unreadable"] {
            tokio::time::timeout(HANG, async {
                let goals = Arc::new(GoalService::new());
                let h = Harness::new_with_control_timeout(
                    Some(Arc::clone(&goals)),
                    None,
                    Duration::from_secs(30),
                )
                .await;
                h.immediate_point().await;
                h.hold_model_response().await;
                let evaluation = h.rt.hold_eval_when_free().await;
                set_goal(&h, &goals, codex_protocol::protocol::ThreadGoalStatus::Blocked)
                    .await
                    .apply_runtime_effects(&goals)
                    .await;
                let db = h.test.codex.state_db().expect("real goal database");
                let original = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
                assert_eq!(original.status, codex_state::ThreadGoalStatus::Blocked);
                let authority = DecisionAuthority {
                    quota: None,
                    quota_identity: None,
                    policy: h.autopilot.sup.policy().expect("live policy"),
                    offs: h.autopilot.sup.offs(),
                    point: h.rt.state().point_at(),
                    accepted_turns: h.rt.accepted_turns.load(Ordering::SeqCst),
                    native_origin: h.rt.state().native_origin,
                };
                let deadline = clock() + Duration::from_secs(3600);
                h.autopilot.schedule(&h.rt, deadline);
                assert!(h.autopilot.sup.want_wait_for(h.rt.key(), Some((deadline, WAIT.into()))));
                h.autopilot.sup.show_waits().await;
                assert_eq!(h.fake.attention().get("codex-alab-wait").map(String::as_str), Some(WAIT));
                h.fake.gate(CLEAR);
                let pilot = Arc::clone(&h.autopilot);
                let runtime = Arc::clone(&h.rt);
                let thread = Arc::clone(&h.test.codex);
                let point = authority.point;
                let accepted = authority.accepted_turns;
                let mut holding = tokio::spawn(async move {
                    pilot.goal_owns_pace(&runtime, &thread, &authority).await
                });
                tokio::select! {
                    () = wait_for_supervisor(|| h.fake.gated_requests().iter().any(|line| line.ends_with(CLEAR))) => {}
                    result = &mut holding => panic!("goal admission ended before the wait clear: {result:?}"),
                }
                assert!(!h.autopilot.sup.attention_stands(h.rt.key(), "goal"));
                assert!(!h.rt.timer_pending());
                match change {
                    "cleared" => {
                        let cleared = goals
                            .clear_thread_goal(&db, h.rt.id, async { Ok(()) })
                            .await
                            .expect("clear the actual goal while wait transport is held");
                        cleared.apply_runtime_effects(&goals).await;
                        assert_eq!(db.thread_goals().get_thread_goal(h.rt.id).await.unwrap(), None);
                    }
                    "unreadable" => {
                        super::description_goal_read_tests::saved_status(&db, h.rt.id, "unreadable").await;
                        assert!(db.thread_goals().get_thread_goal(h.rt.id).await.is_err());
                    }
                    _ => {}
                }
                assert_eq!(h.rt.state().point_at(), point);
                assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
                assert!(!holding.is_finished(), "the wait clear still holds the old read");
                let timer_before = h.rt.timer_seq.load(Ordering::SeqCst);
                let evaluations = h.rt.evaluations();
                let earliest_delayed = clock() + RECHECK;
                h.fake.open_gate(CLEAR);
                assert!(holding.await.expect("complete the old admission"));
                assert_eq!(h.rt.timer_seq.load(Ordering::SeqCst), timer_before + u64::from(change != "unchanged"));
                assert_eq!(h.autopilot.sup.attention_stands(h.rt.key(), "goal"), change == "unchanged");
                assert_eq!(h.fake.attention().get("codex-alab").is_some_and(|text| text.contains("the goal is blocked")), change == "unchanged");
                if change == "cleared" {
                    wait_for_supervisor(|| h.rt.evaluations() > evaluations).await;
                } else if change == "unreadable" {
                    assert!(lock(&h.rt.timer).as_ref().is_some_and(|timer| timer.at >= earliest_delayed));
                    super::description_goal_read_tests::saved_status(&db, h.rt.id, original.status.as_str()).await;
                }
                assert_eq!(h.rt.state().point_at(), point);
                assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
                assert!(responses::received_responses_requests(&h._server).await.is_empty());
                Autopilot::cancel_timer(&h.rt).await;
                h.test.codex.shutdown_and_wait().await.expect("native shutdown");
                drop(evaluation);
            }).await.unwrap_or_else(|error| panic!("{change}: goal attention recheck hang detector: {error}"));
        }
    });
}

async fn set_mode(h: &Harness, mode: ModeKind) {
    core_test_support::submit_thread_settings(
        &h.test.codex,
        ThreadSettingsOverrides {
            collaboration_mode: Some(CollaborationMode {
                mode,
                settings: Settings {
                    model: h.test.session_configured.model.clone(),
                    reasoning_effort: None,
                    developer_instructions: None,
                },
            }),
            ..Default::default()
        },
    )
    .await
    .expect("native collaboration mode");
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
                set_mode(&h, ModeKind::Plan).await;
                let active_outcome =
                    set_goal(&h, &goals, codex_protocol::protocol::ThreadGoalStatus::Active).await;
                // Settle the exact Active effect before native-start accounting can
                // change its boundary. Plan keeps this setup from launching a turn.
                active_outcome.apply_runtime_effects(&goals).await;
                drop(active_outcome);
                assert!(!h.test.codex.has_active_turn().await, "Plan defers the setup goal");
                assert!(h._server.received_requests().await.unwrap_or_default().is_empty(),
                        "the setup goal makes no provider request in Plan mode");
                set_mode(&h, ModeKind::Default).await;
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
                // This explicit native input supplies the gated turn. The later
                // Blocked mutation applies its exact effects too.
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
