// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A decision must still answer its own point after the last preparatory read.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use core_test_support::responses::received_responses_requests;

async fn wait_for(ready: impl Fn() -> bool) {
    while !ready() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[test]
fn explicit_status_off_revokes_a_prepared_act_even_if_stale_on_policy_returns() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut h = Harness::new().await;
            h.retain_artifacts();
            h.fake.set_policy_key("upgrade", "false");
            wait_for(|| h.autopilot.sup.policy().is_some_and(|p| !p.upgrade)).await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let original = h.autopilot.sup.policy();
            let offs = h.autopilot.sup.offs();
            let accepted_before = h.rt.accepted_turns.load(Ordering::SeqCst);
            let (entered, entry) = tokio::sync::oneshot::channel();
            let (release, blocked) = tokio::sync::oneshot::channel();
            *lock(&h.rt.before_submit) = Some((entered, blocked));
            let pilot = Arc::clone(&h.autopilot);
            let rt = Arc::clone(&h.rt);
            let evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
            entry.await.expect("the native act passed readiness and paused before submit");
            h.fake.set_status("autopilot", "off");
            let ready = h.autopilot.sup.ready().await;
            h.fake.set_status("autopilot", "cruise");
            h.fake.set_effective(true, "enabled");
            wait_for(|| h.autopilot.sup.acting() && h.autopilot.sup.policy() == original).await;
            let stale_on_restored = h.autopilot.sup.acting()
                && h.autopilot.sup.policy() == original;
            let revoked = h.autopilot.sup.offs() > offs;
            h.fake.set_status("autopilot", "off");
            release.send(()).expect("release the prepared act");
            evaluating.await.expect("the prepared evaluation finished");
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst) > accepted_before;
            if accepted {
                while received_responses_requests(&h._server).await.is_empty() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
            let requested = !received_responses_requests(&h._server).await.is_empty();
            Autopilot::cancel_timer(&h.rt).await;
            h.test.codex.shutdown_and_wait().await.expect("native shutdown");
            eprintln!("prepared Off observation: ready={ready:?}, stale_on_restored={stale_on_restored}, revoked={revoked}, accepted={accepted}, requested={requested}");
            assert!(stale_on_restored, "the control must recreate the same active policy");
            assert!(ready != Ready::Go && revoked && !accepted && !requested);
        })
        .await
        .expect("prepared Off native admission hang detector");
    });
}

#[test]
fn explicit_status_off_prevents_native_submit_before_the_policy_event() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut observed = Vec::new();
            for off in [false, true] {
                let mut h = Harness::new().await;
                h.retain_artifacts();
                h.fake.set_policy_key("upgrade", "false");
                wait_for(|| h.autopilot.sup.policy().is_some_and(|p| !p.upgrade)).await;
                h.immediate_point().await;
                h.hold_model_response().await;
                if off {
                    h.fake.set_status("autopilot", "off");
                }
                let accepted_before = h.rt.accepted_turns.load(Ordering::SeqCst);
                Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
                let accepted = h.rt.accepted_turns.load(Ordering::SeqCst) > accepted_before;
                // A witnessed native acceptance must reach the private model fixture.
                // Off must accept none; the On control proves the fixture can submit.
                if accepted {
                    while received_responses_requests(&h._server).await.is_empty() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
                let requested = !received_responses_requests(&h._server).await.is_empty();
                observed.push((off, accepted, requested));
                let quiet_off =
                    !h.autopilot.sup.acting() && !h.autopilot.sup.held() && !h.rt.timer_pending();
                Autopilot::cancel_timer(&h.rt).await;
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
                if off {
                    assert!(
                        quiet_off,
                        "Off must not immediately re-arm the native evaluator"
                    );
                }
            }
            eprintln!("actual native Off/On observations: {observed:?}");
            assert_eq!(observed, [(false, true, true), (true, false, false)]);
        })
        .await
        .expect("explicit Off native admission hang detector");
    });
}

#[test]
fn the_final_submit_check_rejects_changed_policy_and_superseding_native_input() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for change in ["unchanged", "land", "native", "native_running"] {
                let h = Harness::new().await;
                h.fake.set_policy_key("upgrade", "false");
                wait_for(|| {
                    h.autopilot
                        .sup
                        .policy()
                        .is_some_and(|policy| !policy.upgrade)
                })
                .await;
                h.immediate_point().await;
                h.hold_model_response().await;
                let decision = decide::decide(
                    &h.rt.state(),
                    &h.autopilot.sup.policy().expect("proven policy"),
                    &h.autopilot.timing,
                    clock(),
                    Duration::ZERO,
                );
                let Act::Type { rule, text } = decision.act else {
                    panic!("the initial native point owes a continuation");
                };
                assert_eq!(rule, words::RULE_CONTINUE);
                let (entered, entry) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((entered, blocked));
                let pilot = Arc::clone(&h.autopilot);
                let rt = Arc::clone(&h.rt);
                let evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
                entry.await.expect("the final submit preparation finished");
                let evaluations = h.rt.evaluations();
                assert!(!h.shared.turn_running_on(h.rt.key()));
                assert!(received_responses_requests(&h._server).await.is_empty());
                match change {
                    "land" => {
                        h.fake.set_status("autopilot", "land");
                        h.fake.set_policy_key("autopilot", "land");
                        wait_for(|| {
                            h.autopilot
                                .sup
                                .policy()
                                .is_some_and(|policy| policy.autopilot == decide::Mode::Land)
                        })
                        .await;
                    }
                    "native" | "native_running" => {
                        use codex_protocol::config_types::CollaborationMode;
                        use codex_protocol::config_types::ModeKind;
                        use codex_protocol::config_types::Settings;
                        let session = codex_extension_api::ExtensionData::new("session");
                        let turn = codex_extension_api::ExtensionData::new("corrected-native-turn");
                        let mode = CollaborationMode {
                            mode: ModeKind::Default,
                            settings: Settings {
                                model: "gpt-5.5".into(),
                                reasoning_effort: None,
                                developer_instructions: None,
                            },
                        };
                        h.autopilot
                            .on_turn_start(TurnStartInput {
                                turn_id: "corrected-native-turn",
                                collaboration_mode: &mode,
                                token_usage_at_turn_start: None,
                                session_store: &session,
                                thread_store: h.test.codex.thread_extension_data(),
                                turn_store: &turn,
                            })
                            .await;
                        if change == "native" {
                            // An abort no person's Esc made: the point it leaves decides nothing,
                            // so nothing may be typed for the stale decision or after it. (A
                            // person's Esc is held for `human_grace_s` and then carried on, FOREVER
                            // CRUISE: `decide_tests.rs`
                            // `a_persons_esc_holds_for_the_grace_then_cruise_and_land_carry_on`;
                            // this harness's grace of 0 would carry it on at once.)
                            h.autopilot
                                .on_turn_abort(codex_extension_api::TurnAbortInput {
                                    reason: codex_protocol::protocol::TurnAbortReason::Replaced,
                                    session_store: &session,
                                    thread_store: h.test.codex.thread_extension_data(),
                                    turn_store: &turn,
                                })
                                .await;
                            assert!(!h.shared.turn_running_on(h.rt.key()));
                        }
                    }
                    "unchanged" => {}
                    _ => unreachable!(),
                }
                release.send(()).expect("release the final submit check");
                evaluating.await.expect("evaluation completed");
                if change == "native_running" {
                    assert!(received_responses_requests(&h._server).await.is_empty());
                    assert!(h.shared.turn_running_on(h.rt.key()));
                    assert!(!h.rt.timer_pending(), "the running turn owns the next look");
                    assert_eq!(h.rt.evaluations(), evaluations);
                } else if change == "native" {
                    assert!(received_responses_requests(&h._server).await.is_empty());
                    assert!(!h.fake.acts().iter().any(|act| act == words::RULE_CONTINUE));
                    assert_eq!(
                        h.rt.state().point_turn_id.as_deref(),
                        Some("corrected-native-turn")
                    );
                    assert!(
                        h.rt.state()
                            .point
                            .as_ref()
                            .is_some_and(|point| point.aborted)
                    );
                } else {
                    wait_for(|| {
                        h.fake
                            .acts()
                            .iter()
                            .any(|act| act == words::RULE_CONTINUE || act == words::RULE_LAND)
                    })
                    .await;
                    let requests = loop {
                        let requests = received_responses_requests(&h._server).await;
                        if !requests.is_empty() {
                            break requests;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    };
                    let inputs: Vec<_> = requests
                        .iter()
                        .flat_map(|request| request.message_input_texts("user"))
                        .collect();
                    if change == "unchanged" {
                        assert!(inputs.iter().any(|input| input == &text));
                        assert_eq!(
                            h.fake
                                .acts()
                                .iter()
                                .filter(|act| *act == words::RULE_CONTINUE)
                                .count(),
                            1
                        );
                    } else {
                        assert!(
                            !inputs.iter().any(|input| input == &text),
                            "stale Cruise input reached the model"
                        );
                        assert!(h.fake.acts().iter().any(|act| act == words::RULE_LAND));
                        assert!(!h.fake.acts().iter().any(|act| act == words::RULE_CONTINUE));
                    }
                }
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            }
        })
        .await
        .expect("final submit authority hang detector");
    });
}
