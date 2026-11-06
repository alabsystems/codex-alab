// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A real detached native finisher can still be delivering a late user's item at stop.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::turn_input::TurnInputSubmission;
use core_test_support::responses;

const HANG: Duration = Duration::from_secs(60);
const LATE_INPUT: &str = "late input held by the native finisher";

struct ErrorGate(
    Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
);

impl TurnLifecycleContributor for ErrorGate {
    fn on_turn_error<'a>(&'a self, _input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let gate = lock(&self.0).take();
            if let Some((entered, release)) = gate {
                entered.send(()).expect("terminal error observer remains");
                release.await.expect("terminal error release");
            }
        })
    }
}

#[test]
fn a_retired_item_cannot_consume_new_mail_or_turn_expectations() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let h = Harness::new().await;
            h.immediate_point().await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("old native shutdown");
            let fresh = h
                .test
                .thread_manager
                .start_thread(codex_core::StartThreadOptions {
                    environments: Some(Vec::new()),
                    ..codex_core::StartThreadOptions::new(h.test.config.clone())
                })
                .await
                .expect("new loaded root");
            h.shared.expect_own_turn(LATE_INPUT);
            h.shared.expect_mail_turn(codex_alab_core::mail::MailTurn {
                id: 123,
                off: 7,
                from: "new caller".into(),
                kind: "message".into(),
                text: LATE_INPUT.into(),
            });
            h.autopilot
                .on_item_completed(
                    h.test.codex.thread_extension_data(),
                    &ExtensionData::new("retired-callback"),
                    &TurnItem::UserMessage(codex_protocol::items::UserMessageItem {
                        id: "retired-input".into(),
                        client_id: None,
                        content: vec![UserInput::Text {
                            text: LATE_INPUT.into(),
                            text_elements: Vec::new(),
                        }],
                    }),
                )
                .await;
            let actual = (
                h.shared.is_own_turn(LATE_INPUT),
                h.shared.is_mail_turn(LATE_INPUT),
            );
            fresh
                .thread
                .shutdown_and_wait()
                .await
                .expect("fresh native shutdown");
            assert_eq!(
                actual,
                (true, true),
                "a retired callback must not claim fresh global input provenance"
            );
        })
        .await
        .expect("retired item entry hang detector");
    });
}

#[test]
fn a_detached_finishers_person_item_cannot_overwrite_its_replacement_after_hand_read() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            // A terminal error returns from RegularTask without consuming newly pending
            // input. Hold its real observer while the native API accepts that late steer.
            let (error_entered, entered) = tokio::sync::oneshot::channel();
            let (error_release, release) = tokio::sync::oneshot::channel();
            let observer = Arc::new(ErrorGate(Mutex::new(Some((error_entered, release)))));
            let h = Harness::new_with_goals_and_observer(None, Some(observer)).await;
            eprintln!("item custody: native fixture ready");
            h.immediate_point().await;
            responses::mount_sse_once(
                &h._server,
                responses::sse_failed("old-response", "cyber_policy", "This action was refused."),
            )
            .await;
            let input = |text: &str| {
                TurnInputRequest::user_input(vec![UserInput::Text {
                    text: text.into(),
                    text_elements: Vec::new(),
                }])
            };
            let start = h.test.codex.start_or_steer_turn(input("Begin the task.")).await.expect("native start");
            let TurnInputSubmission::Started { turn_id, .. } = start else {
                panic!("expected a new native turn");
            };
            entered.await.expect("native terminal error observer reached");
            eprintln!("item custody: terminal error observer held");
            let steer = h.test.codex.start_or_steer_turn(input(LATE_INPUT)).await.expect("native late steer");
            assert!(matches!(steer, TurnInputSubmission::Steered { turn_id: id, .. } if id == turn_id));
            eprintln!("item custody: late native steer accepted");

            // This fixture widens only the hand-read deadline to the hang detector. Its
            // actual status request remains gated; shutdown speed cannot decide the test.
            h.autopilot.sup.set_hand_read_limit(HANG);
            h.fake.gate("status");
            let before = h.fake.gated_requests().len();
            error_release.send(()).expect("release native terminal error");
            core_test_support::wait_for_event_with_timeout(
                &h.test.codex,
                |event| matches!(event, EventMsg::ItemStarted(started) if matches!(&started.item, TurnItem::UserMessage(user) if user.message().contains(LATE_INPUT))),
                HANG,
            ).await;
            eprintln!("item custody: late item started in detached finisher");
            wait_for_supervisor(|| h.fake.gated_requests().len() > before).await;
            h.rt.with_state(|state| {
                state.needs = decide::Needs { standing: vec!["old approval".into()], key: 11, acked: None, ..Default::default() };
            });
            h.test.codex.shutdown_and_wait().await.expect("native shutdown while detached finisher awaits status");
            loop {
                match h.test.codex.next_event().await.expect("old native shutdown event").msg {
                    EventMsg::TurnAborted(_) => panic!("the pending item belonged to an abortable task, not the detached finisher"),
                    EventMsg::ItemCompleted(done) if matches!(&done.item, TurnItem::UserMessage(user) if user.message().contains(LATE_INPUT)) => panic!("late callback completed before its status gate was released"),
                    EventMsg::ShutdownComplete => break,
                    _ => {}
                }
            }
            eprintln!("item custody: native shutdown retired old runtime without aborting the detached finisher");
            assert!(h.autopilot.thread_rt(h.rt.key()).is_none());

            let replacement = h.test.thread_manager.resume_legacy_thread_from_rollout(
                h.test.config.clone(),
                h.test.codex.rollout_path().expect("native rollout"),
                h.test.thread_manager.auth_manager(),
                None,
                Default::default(),
            ).await.expect("native same-ID replacement");
            eprintln!("item custody: genuine same-ID resume completed");
            let fresh = replacement.thread.thread_extension_data().get::<ThreadRt>().expect("fresh runtime");
            assert!(!Arc::ptr_eq(&h.rt, &fresh));
            wait_for_supervisor(|| fresh.scanned.load(Ordering::SeqCst)).await;
            eprintln!("item custody: replacement resume scan completed");
            fresh.with_state(|state| {
                state.task = true;
                state.needs = decide::Needs { standing: vec!["fresh approval".into()], key: 22, acked: None, ..Default::default() };
            });
            h.autopilot.sup.want_needs(&["fresh approval".into()], 22);
            h.shared.set_carried_on(1);
            h.fake.open_gate("status");
            core_test_support::wait_for_event_with_timeout(
                &h.test.codex,
                |event| matches!(event, EventMsg::ItemCompleted(completed) if matches!(&completed.item, TurnItem::UserMessage(user) if user.message().contains(LATE_INPUT))),
                HANG,
            ).await;
            let actual = (h.shared.carried_on(), h.autopilot.sup.needs_key());
            replacement.thread.shutdown_and_wait().await.expect("replacement shutdown");
            assert_eq!(actual, (1, 22), "retired pending input must not replace the fresh runtime's shared state");
        }).await.expect("detached item callback hang detector");
    });
}
