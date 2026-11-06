// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A prepared policy continuation must respect newer TUI input before native admission.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use codex_alab_core::session::Frame;
use core_test_support::responses::received_responses_requests;

#[derive(Clone, Copy, Debug)]
enum Hand {
    Empty,
    Draft,
    Queued,
    Pending,
    Popup,
    Editor,
    Overlay,
    KeyBeforeFrame,
}

fn show_hand(h: &Harness, hand: Hand) {
    if matches!(hand, Hand::KeyBeforeFrame) {
        h.shared.human_input(now_ms());
        return;
    }
    let view = h.shared.view();
    h.shared.observe(
        &Frame {
            primary_thread: view.primary_thread.as_deref(),
            displayed_thread: view.displayed_thread.as_deref(),
            rollout_path: view
                .primary()
                .and_then(|facts| facts.rollout_path.as_deref()),
            cwd: view.cwd.as_deref().expect("native fixture cwd"),
            model: view.model.as_deref().expect("native fixture model"),
            codex_home: view.codex_home.as_deref().expect("native fixture home"),
            embedded: view.embedded,
            state: view.state,
            draft_empty: !matches!(hand, Hand::Draft),
            queued_input: matches!(hand, Hand::Queued),
            pending_user_turn: matches!(hand, Hand::Pending),
            popup_open: matches!(hand, Hand::Popup),
            editor_open: matches!(hand, Hand::Editor),
        },
        now_ms(),
    );
    h.shared.set_overlay_open(matches!(hand, Hand::Overlay));
}

#[test]
fn a_policy_continuation_respects_input_arriving_after_its_ready_checks() {
    prepared_continuation_hands(false);
}

#[test]
fn a_compaction_respects_input_arriving_after_its_ready_checks() {
    prepared_continuation_hands(true);
}

fn prepared_continuation_hands(compact: bool) {
    native_test(move || async move {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut admitted_under_hand = Vec::new();
            for hand in [
                Hand::Empty,
                Hand::Draft,
                Hand::Queued,
                Hand::Pending,
                Hand::Popup,
                Hand::Editor,
                Hand::Overlay,
                Hand::KeyBeforeFrame,
            ] {
                let mut h = Harness::new().await;
                h.retain_artifacts();
                h.fake.set_policy_key("upgrade", "false");
                h.fake.set_policy_key("compact_on_context_wall", "true");
                wait_for_supervisor(|| {
                    h.autopilot
                        .sup
                        .policy()
                        .is_some_and(|policy| !policy.upgrade && policy.compact_on_context_wall)
                })
                .await;
                h.immediate_point().await;
                h.hold_model_response().await;
                h.rt.with_state(|state| {
                    state.point.as_mut().expect("existing native point").wall = Some(if compact {
                        Wall::Context { auto_compact: true }
                    } else {
                        Wall::Policy
                    });
                });
                let decision = decide::decide(
                    &h.rt.state(),
                    &h.autopilot.sup.policy().expect("proven policy"),
                    &h.autopilot.timing,
                    clock(),
                    Duration::ZERO,
                );
                let rule = if compact {
                    assert_eq!(decision.act, Act::Compact);
                    words::RULE_COMPACT
                } else {
                    assert!(matches!(
                        decision.act,
                        Act::Type {
                            rule: words::RULE_POLICY_CONTINUE,
                            ..
                        }
                    ));
                    words::RULE_POLICY_CONTINUE
                };
                let (entered, entry) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((entered, blocked));
                let pilot = Arc::clone(&h.autopilot);
                let rt = Arc::clone(&h.rt);
                let evaluation = tokio::spawn(async move { pilot.evaluate(rt).await });
                entry
                    .await
                    .expect("ready succeeded and native preparation reached its gate");
                let point = h.rt.state();
                let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
                show_hand(&h, hand);
                assert_eq!(
                    h.rt.state(),
                    point,
                    "TUI input changes no native decision point"
                );
                assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
                release
                    .send(())
                    .expect("release prepared native continuation");
                evaluation.await.expect("actual native evaluator completed");
                let acts = h.fake.acts();
                if compact && acts.iter().any(|act| act == rule) {
                    wait_for_supervisor(|| h.rt.accepted_turns.load(Ordering::SeqCst) != accepted)
                        .await;
                }
                let started = h.rt.accepted_turns.load(Ordering::SeqCst) != accepted;
                if started {
                    while received_responses_requests(&h._server).await.is_empty() {
                        tokio::task::yield_now().await;
                    }
                }
                let requests = received_responses_requests(&h._server).await.len();
                if matches!(hand, Hand::Empty) {
                    assert!(started && requests == 1 && acts.iter().any(|act| act == rule));
                } else if started || requests != 0 || !acts.is_empty() {
                    admitted_under_hand.push((hand, started, requests, acts));
                } else {
                    assert_eq!(
                        h.rt.state().point,
                        point.point,
                        "waiting preserves the refusal point"
                    );
                    assert!(
                        h.rt.timer_pending(),
                        "the held continuation must remain owed"
                    );
                    show_hand(&h, Hand::Empty);
                    h.immediate_point().await;
                    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
                    if compact {
                        wait_for_supervisor(|| {
                            h.rt.accepted_turns.load(Ordering::SeqCst) != accepted
                        })
                        .await;
                    }
                    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
                    assert_eq!(h.fake.acts().iter().filter(|act| *act == rule).count(), 1);
                }
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("owned native shutdown before verdict");
            }
            assert!(
                admitted_under_hand.is_empty(),
                "new TUI input must hold the prepared continuation: {admitted_under_hand:?}"
            );
        })
        .await
        .expect("TUI input native admission hang detector");
    });
}

#[test]
fn a_usage_goal_resume_respects_a_draft_after_its_initial_read() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for hand in [Hand::Empty, Hand::Draft] {
                let goals = Arc::new(GoalService::new());
                let mut h = Harness::new_with_goals(Some(goals)).await;
                h.retain_artifacts();
                h.immediate_point().await;
                h.hold_model_response().await;
                let db = h.test.codex.state_db().expect("native goal database");
                let original = db
                    .thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "the permitted task",
                        codex_state::ThreadGoalStatus::UsageLimited,
                        None,
                    )
                    .await
                    .expect("existing limited goal");
                let authority = DecisionAuthority {
                    quota: None,
                    quota_identity: None,
                    policy: h.autopilot.sup.policy().expect("proven policy"),
                    offs: h.autopilot.sup.offs(),
                    point: h.rt.state().point_at(),
                    accepted_turns: h.rt.accepted_turns.load(Ordering::SeqCst),
                    native_origin: h.rt.state().native_origin,
                };
                let before = h.rt.state();
                let (entered, entry) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((entered, blocked));
                let pilot = Arc::clone(&h.autopilot);
                let rt = Arc::clone(&h.rt);
                let thread = Arc::clone(&h.test.codex);
                let resume =
                    tokio::spawn(async move { pilot.resume_goal(&rt, &thread, &authority).await });
                entry
                    .await
                    .expect("the original usage-limited goal was read");
                show_hand(&h, hand);
                release
                    .send(())
                    .expect("release the goal's final preparation");
                let outcome = resume.await.expect("actual GoalService resume completed");
                let current = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("read goal after resume");
                if matches!(hand, Hand::Draft) {
                    assert!(matches!(outcome, GoalResume::TuiHeld { .. }));
                    assert_eq!(current, Some(original));
                    assert_eq!(h.rt.state(), before, "a held resume spends no try");
                    assert!(received_responses_requests(&h._server).await.is_empty());
                } else {
                    assert_eq!(outcome, GoalResume::Resumed);
                    assert_eq!(
                        current.expect("goal remains").status,
                        codex_state::ThreadGoalStatus::Active
                    );
                    assert!(
                        h.rt.state().point.is_none(),
                        "the healthy resume books its try"
                    );
                }
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("owned goal fixture shutdown");
            }
        })
        .await
        .expect("TUI goal resume hang detector");
    });
}
