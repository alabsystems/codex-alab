// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Drive the existing evaluator past a real conditional-leave refusal before
//! delivering its displayed end, exhausting its bound or changing its authority.

use super::*;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

const COMPLETED: &str = "displayed-complete-turn";

fn bounded<F: Future<Output = ()>>(exercise: impl FnOnce() -> F + Send + 'static) {
    // Construct the native fixture on the same owned stack as the other live
    // upgrade tests; moving only its evaluator to the heap still overflows libtest.
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("owned current-thread runtime")
                .block_on(bounded_exercise(exercise()));
        })
        .expect("owned test thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

async fn bounded_exercise(exercise: impl Future<Output = ()>) {
    struct Watch {
        cancel: std::sync::mpsc::Sender<()>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for Watch {
        fn drop(&mut self) {
            let _ = self.cancel.send(());
            if let Some(thread) = self.thread.take() {
                thread.join().expect("wall hang detector settled");
            }
        }
    }
    let (cancel, cancelled) = std::sync::mpsc::channel();
    let (expired, wall) = tokio::sync::oneshot::channel();
    let _watch = Watch {
        cancel,
        thread: Some(std::thread::spawn(move || {
            if cancelled.recv_timeout(Duration::from_secs(60)).is_err() {
                let _ = expired.send(());
            }
        })),
    };
    tokio::select! {
        () = exercise => {},
        _ = wall => panic!("displayed-end catch-up real-wall hang detector"),
    }
}

async fn displayed_complete() -> Harness {
    let mut h = Harness::new().await;
    h.retain_artifacts();
    h.immediate_point().await;
    h.hold_model_response().await;
    assert!(h.shared.set_waker(Box::new(|| {})));
    assert!(h.shared.turn_started(h.rt.key(), COMPLETED));
    h.shared.turn_ended(h.rt.key(), COMPLETED);
    h.rt.with_state(|state| {
        state.native_origin = Some(codex_alab_core::session::NativeOrigin {
            turn_id: COMPLETED.into(),
            phase: codex_alab_core::session::NativePhase::Complete,
        });
    });
    assert!(h.shared.displays(h.rt.key()));
    assert!(!h.shared.tui_handled_end(h.rt.key(), COMPLETED));
    h
}

fn count(h: &Harness, rule: &str) -> usize {
    h.fake.acts().iter().filter(|act| *act == rule).count()
}

async fn refused_then_waiting(h: &mut Harness, mut evaluating: Pin<&mut impl Future<Output = ()>>) {
    let request = tokio::select! {
        request = h.requests.recv() => request.expect("first conditional handoff"),
        () = &mut evaluating => panic!("the evaluator skipped the first handoff"),
    };
    let carried = h.shared.owed_act().expect("the exact decision is carried");
    assert_eq!(carried.rule.as_deref(), Some(words::RULE_CONTINUE));
    assert_eq!(carried.native_origin.as_ref().unwrap().turn_id, COMPLETED);
    assert_eq!(
        carried.native_origin.as_ref().unwrap().phase,
        codex_alab_core::session::NativePhase::Complete
    );
    assert!(!h.shared.tui_handled_end(h.rt.key(), COMPLETED));
    assert!(!request.decide(false));
    // decide synchronously resolves request_checked's only pending await. Its
    // next poll runs the refusal and reaches the existing catch-up sleep.
    std::future::poll_fn(|cx| {
        assert!(
            evaluating.as_mut().poll(cx).is_pending(),
            "the refusal must wait for its displayed end"
        );
        Poll::Ready(())
    })
    .await;
    assert!(h.autopilot.upgrading.load(Ordering::SeqCst));
    assert_eq!(count(h, upgrade::RULE_UPGRADE), 1);
    assert_eq!(count(h, words::RULE_CONTINUE), 0);
    assert!(matches!(
        h.requests.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

fn pause_without_auto_advance() -> tokio::task::JoinHandle<()> {
    tokio::time::pause();
    tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    })
}

async fn resume_clock(runnable: tokio::task::JoinHandle<()>) {
    runnable.abort();
    let _ = runnable.await;
    tokio::time::resume();
}

#[test]
fn a_later_displayed_end_retries_the_same_handoff_once() {
    bounded(|| async {
        let mut h = displayed_complete().await;
        let before = h.rt.evaluations();
        let pilot = Arc::clone(&h.autopilot);
        let rt = Arc::clone(&h.rt);
        let evaluating = pilot.evaluate(rt);
        tokio::pin!(evaluating);
        refused_then_waiting(&mut h, evaluating.as_mut()).await;
        let runnable = pause_without_auto_advance();
        // Stay inside the semantic window without depending on a rounded
        // millisecond sleep deadline becoming ready at exactly one poll tick.
        tokio::time::advance(TUI_CATCH_UP / 4).await;
        std::future::poll_fn(|cx| {
            assert!(evaluating.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert_eq!(count(&h, upgrade::RULE_UPGRADE), 1);
        assert_eq!(count(&h, words::RULE_CONTINUE), 0);
        h.shared.turn_handled_end(h.rt.key(), COMPLETED);
        tokio::time::advance(TUI_CATCH_UP / 4).await;
        let request = tokio::select! {
            request = h.requests.recv() => request.expect("one fresh handoff after catch-up"),
            () = &mut evaluating => panic!("catch-up fell through to a continuation"),
        };
        assert!(request.decide(true));
        evaluating.await;
        assert!(h.shared.exec_pending());
        assert_eq!(count(&h, upgrade::RULE_UPGRADE), 2);
        assert_eq!(count(&h, words::RULE_CONTINUE), 0);
        assert_eq!(h.rt.evaluations(), before + 1);
        assert!(matches!(
            h.requests.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        assert!(
            responses::received_responses_requests(&h._server)
                .await
                .is_empty()
        );
        resume_clock(runnable).await;
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .expect("native shutdown");
    });
}

#[test]
fn an_unhandled_displayed_end_uses_the_bound_then_continues_once() {
    bounded(|| async {
        let mut h = displayed_complete().await;
        let before = h.rt.evaluations();
        let pilot = Arc::clone(&h.autopilot);
        let rt = Arc::clone(&h.rt);
        let evaluating = pilot.evaluate(rt);
        tokio::pin!(evaluating);
        refused_then_waiting(&mut h, evaluating.as_mut()).await;
        let runnable = pause_without_auto_advance();
        tokio::time::advance(TUI_CATCH_UP).await;
        evaluating.await;
        assert!(!h.shared.tui_handled_end(h.rt.key(), COMPLETED));
        assert!(!h.shared.exec_pending());
        assert_eq!(count(&h, upgrade::RULE_UPGRADE), 1);
        assert_eq!(count(&h, words::RULE_CONTINUE), 1);
        assert_eq!(h.rt.evaluations(), before + 1);
        assert!(h.shared.turn_running_on(h.rt.key()));
        assert!(matches!(
            h.requests.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        let requests = loop {
            let requests = responses::received_responses_requests(&h._server).await;
            if !requests.is_empty() {
                break requests;
            }
            tokio::task::yield_now().await;
        };
        assert_eq!(requests.len(), 1, "one native fallback submission");
        resume_clock(runnable).await;
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .expect("native shutdown");
    });
}

#[test]
fn authority_changes_during_catch_up_retire_the_old_retry_and_submission() {
    bounded(|| async {
        for change in ["off", "policy", "native"] {
            let mut h = displayed_complete().await;
            let pilot = Arc::clone(&h.autopilot);
            let rt = Arc::clone(&h.rt);
            let evaluating = pilot.evaluate(rt);
            tokio::pin!(evaluating);
            refused_then_waiting(&mut h, evaluating.as_mut()).await;
            let runnable = pause_without_auto_advance();
            match change {
                "off" => {
                    let offs = h.autopilot.sup.offs();
                    h.fake.set_status("autopilot", "off");
                    h.fake.set_effective(false, "autopilot-off");
                    let ready = h.autopilot.sup.ready().await;
                    eprintln!("{change}: readiness receipt {ready:?}");
                    assert_ne!(ready, Ready::Go);
                    while h.autopilot.sup.acting() || h.autopilot.sup.offs() == offs {
                        tokio::task::yield_now().await;
                    }
                }
                "policy" => {
                    h.fake.set_policy_key("questions", "ask");
                    while !h
                        .autopilot
                        .sup
                        .policy()
                        .is_some_and(|p| p.questions == decide::Questions::Ask)
                    {
                        tokio::task::yield_now().await;
                    }
                }
                "native" => {
                    use codex_protocol::config_types::CollaborationMode;
                    use codex_protocol::config_types::ModeKind;
                    use codex_protocol::config_types::Settings;
                    let session = codex_extension_api::ExtensionData::new("session");
                    let turn = codex_extension_api::ExtensionData::new("superseding-native-turn");
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
                            turn_id: "superseding-native-turn",
                            collaboration_mode: &mode,
                            token_usage_at_turn_start: None,
                            session_store: &session,
                            thread_store: h.test.codex.thread_extension_data(),
                            turn_store: &turn,
                        })
                        .await;
                    h.autopilot
                        .on_turn_stop(codex_extension_api::TurnStopInput {
                            session_store: &session,
                            thread_store: h.test.codex.thread_extension_data(),
                            turn_store: &turn,
                        })
                        .await;
                }
                _ => unreachable!(),
            }
            eprintln!(
                "{change}: authority observed: acting={} offs={} accepted={}",
                h.autopilot.sup.acting(),
                h.autopilot.sup.offs(),
                h.rt.accepted_turns.load(Ordering::SeqCst)
            );
            h.shared.turn_handled_end(h.rt.key(), COMPLETED);
            eprintln!("{change}: exact displayed end released");
            tokio::time::advance(TUI_CATCH_UP).await;
            evaluating.await;
            eprintln!("{change}: original evaluator returned");
            assert!(!h.shared.exec_pending(), "{change}");
            assert_eq!(count(&h, upgrade::RULE_UPGRADE), 1, "{change}");
            assert_eq!(count(&h, words::RULE_CONTINUE), 0, "{change}");
            assert!(
                matches!(
                    h.requests.try_recv(),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                ),
                "{change}"
            );
            resume_clock(runnable).await;
            eprintln!("{change}: native cleanup begins");
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
            eprintln!("{change}: native cleanup completed");
        }
    });
}
