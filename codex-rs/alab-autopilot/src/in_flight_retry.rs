// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Provider pacing retained by an ordinary witnessed crash carry-on. Unlike terminal
//! Recovery this metadata grants no action and cannot bypass the one-carry-on bound.

use crate::decide::ThreadState;
use crate::resume_history::NativePoint;
use codex_alab_core::session::InFlightRetry;
use codex_history::RolloutItem;
use codex_protocol::protocol::TurnAbortReason;
use std::time::Instant;

pub(crate) fn restore(
    state: &mut ThreadState,
    items: &[RolloutItem],
    retry: &InFlightRetry,
    signal_teardown: bool,
    now: Instant,
    epoch: u64,
) -> bool {
    if !state.task || !state.carry_on {
        return false;
    }
    // The carry-on and its provider bound must agree about the latest native turn:
    // a late terminal from a retired attempt cannot erase its successor's pacing.
    let native = crate::resume_history::latest(items);
    if !native.started || native.turn_id != Some(retry.turn_id.as_str()) {
        return false;
    }
    let current = match native.point {
        NativePoint::Running => true,
        // Native ShutdownFirst aborts an active task with Interrupted. Only
        // the exact authenticated TERM/HUP handoff can retain that wait.
        NativePoint::Aborted(aborted) => {
            native.interrupted_running_turn
                && signal_teardown
                && aborted.turn_id.as_deref() == Some(&retry.turn_id)
                && aborted.reason == TurnAbortReason::Interrupted
        }
        _ => false,
    };
    if !current {
        return false;
    }
    let Some(deadline) = crate::recovery::instant_of(retry.not_before_ms, now, epoch) else {
        return false;
    };
    state.carry_on_not_before = Some(deadline);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(value: serde_json::Value) -> RolloutItem {
        RolloutItem::EventMsg(serde_json::from_value(value).expect("native event fixture"))
    }

    fn started(turn: &str) -> RolloutItem {
        event(
            json!({"type":"task_started", "turn_id":turn, "model_context_window":258400, "collaboration_mode_kind":"default"}),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn a_measured_cut_crosses_the_record_format_with_its_corrected_epoch_deadline() {
        use codex_alab_core::capacity::InFlightRetryObserver;
        use codex_alab_core::wake::ArmedWait;
        use codex_alab_core::wake::Deadline;
        use codex_alab_core::wake::POLL;
        use codex_alab_core::wake::Reading;
        use codex_alab_core::wake::Wake;
        use std::sync::Arc;
        use std::sync::Mutex;
        use std::time::Duration;

        let start = tokio::time::Instant::now();
        let second = Duration::from_secs(1);
        let original = start + 60 * second;
        let receipt_epoch = 1_000_000;
        // The host slept 120 seconds before selection, while the awake clock stood still.
        let selection_epoch = receipt_epoch + 120_000;
        assert_eq!(
            crate::recovery::epoch_of(original.into_std(), start.into_std(), receipt_epoch),
            1_060_001
        );
        let deadline = Deadline::with_reading(
            original.into_std(),
            Some(Reading {
                asleep: 5000 * second,
                last: None,
            }),
        );
        // The selection sample failed. The fresh local minimum must keep its own
        // unknown origin, while the provider still retains its known receipt origin.
        let wait =
            ArmedWait::from_deadline_with_clock(deadline, Some(start + 10 * second), true, || None);
        let published = Arc::new(Mutex::new(None::<InFlightRetry>));
        let captured = Arc::clone(&published);
        let observer = InFlightRetryObserver::new(move |deadline| {
            let now = tokio::time::Instant::now();
            let elapsed =
                u64::try_from(now.duration_since(start).as_millis()).expect("fixture clock fits");
            *captured.lock().unwrap() = deadline.map(|deadline| InFlightRetry {
                thread: "thread".into(),
                turn_id: "waiting".into(),
                not_before_ms: crate::recovery::epoch_of(
                    deadline,
                    now.into_std(),
                    selection_epoch + elapsed,
                ),
            });
        });
        let guard = observer.waiting(wait.deadline().into_std());
        assert_eq!(
            published.lock().unwrap().as_ref().unwrap().not_before_ms,
            1_180_001,
            "the rejected sample initially leaves a conservative deadline"
        );

        let restored_retry = {
            let waiting = wait.wait_with_clock_observed(
                POLL,
                || {
                    Some(Reading {
                        asleep: 5120 * second,
                        last: Some(Wake {
                            at: (start + POLL).into_std(),
                            slept: 120 * second,
                            seq: 1,
                        }),
                    })
                },
                |deadline| {
                    assert!(guard.update(deadline.into_std()));
                },
            );
            tokio::pin!(waiting);
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(waiting.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            tokio::time::advance(POLL).await;
            std::future::poll_fn(|cx| {
                assert!(
                    std::future::Future::poll(waiting.as_mut(), cx).is_pending(),
                    "the corrected ten-second minimum still holds the live wait"
                );
                std::task::Poll::Ready(())
            })
            .await;
            let retry = published
                .lock()
                .unwrap()
                .clone()
                .expect("the current wait is published");
            assert_eq!(
                retry.not_before_ms, 1_130_001,
                "the first valid poll corrects durable UTC pacing before the wait finishes"
            );
            tokio::time::advance(2 * second).await;
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(waiting.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            assert_eq!(
                published.lock().unwrap().as_ref(),
                Some(&retry),
                "repeated readings do not move the deadline again"
            );
            let wire: codex_alab_tui::record::InFlightRetry = retry.into();
            let bytes = serde_json::to_vec(&wire).expect("actual pacing record encoding");
            let loaded: codex_alab_tui::record::InFlightRetry =
                serde_json::from_slice(&bytes).expect("actual pacing record decoding");
            InFlightRetry::from(loaded)
        };

        let now = tokio::time::Instant::now().into_std();
        let epoch = selection_epoch + 3_000;
        let items = vec![started("waiting")];
        let mut carried = ThreadState::default();
        carried.observe_resume(true, None, true);
        assert!(restore(
            &mut carried,
            &items,
            &restored_retry,
            false,
            now,
            epoch
        ));
        assert_eq!(
            carried.carry_on_not_before,
            Some(now + Duration::from_millis(7_001)),
            "replay keeps the corrected remaining minimum rather than the old 57-second delay"
        );
        let mut uncarried = ThreadState::default();
        uncarried.observe_resume(true, None, false);
        assert!(
            !restore(&mut uncarried, &items, &restored_retry, false, now, epoch),
            "corrected pacing still grants no carry-on authority"
        );
        drop(guard);
        assert_eq!(
            *published.lock().unwrap(),
            None,
            "ordinary wait cleanup remains effective"
        );
    }

    #[test]
    fn pacing_requires_an_existing_carry_on_and_the_exact_latest_native_attempt() {
        let now = Instant::now();
        let retry = InFlightRetry {
            thread: "thread".into(),
            turn_id: "waiting".into(),
            not_before_ms: 60_000,
        };
        let base = vec![
            started("waiting"),
            event(
                json!({"type":"user_message", "message":"original task", "images":null, "local_images":[], "text_elements":[]}),
            ),
        ];
        let tails = [
            (None, false, true),
            (Some(started("newer")), false, false),
            (
                Some(event(
                    json!({"type":"task_complete", "turn_id":"waiting", "last_agent_message":"finished"}),
                )),
                false,
                false,
            ),
            (
                Some(event(
                    json!({"type":"turn_aborted", "turn_id":"waiting", "reason":"interrupted"}),
                )),
                false,
                false,
            ),
            (
                Some(event(
                    json!({"type":"turn_aborted", "turn_id":"waiting", "reason":"interrupted"}),
                )),
                true,
                true,
            ),
            (
                Some(event(
                    json!({"type":"turn_aborted", "turn_id":"other", "reason":"interrupted"}),
                )),
                true,
                false,
            ),
            (
                Some(event(
                    json!({"type":"turn_aborted", "reason":"interrupted"}),
                )),
                true,
                false,
            ),
            // A mismatched completion cannot finish the identified native turn;
            // unlike an unknown abort, it does not revoke its existing carry-on.
            (
                Some(event(
                    json!({"type":"task_complete", "turn_id":"other", "last_agent_message":"unrelated"}),
                )),
                false,
                true,
            ),
            (
                Some(event(
                    json!({"type":"turn_aborted", "turn_id":"waiting", "reason":"replaced"}),
                )),
                true,
                false,
            ),
        ];
        for (tail, signal, expected) in tails {
            let mut items = base.clone();
            items.extend(tail);
            let mut state = ThreadState::default();
            state.observe_resume(true, None, true);
            assert_eq!(
                restore(&mut state, &items, &retry, signal, now, 1_000),
                expected,
                "{items:?} signal={signal}"
            );
            assert_eq!(
                state.carry_on_not_before,
                expected.then_some(now + std::time::Duration::from_secs(59))
            );
        }
        let mut state = ThreadState::default();
        state.observe_resume(true, None, false);
        assert!(!restore(&mut state, &base, &retry, true, now, 1_000));
        assert!(!state.carry_on, "pacing never authorizes a person's resume");

        let no_start = vec![
            event(json!({"type":"task_complete", "turn_id":"waiting"})),
            event(json!({"type":"turn_aborted", "turn_id":"waiting", "reason":"interrupted"})),
        ];
        let mut state = ThreadState::default();
        state.observe_resume(true, None, true);
        assert!(
            !restore(&mut state, &no_start, &retry, true, now, 1_000),
            "a completion-derived turn identity does not witness a native TurnStarted"
        );
        assert_eq!(state.carry_on_not_before, None);
    }

    #[test]
    fn late_retired_terminals_keep_the_native_carry_on_behind_the_provider_deadline() {
        let now = Instant::now();
        let retry = InFlightRetry {
            thread: "thread".into(),
            turn_id: "waiting".into(),
            not_before_ms: 60_000,
        };
        for terminal in [
            event(json!({"type":"turn_aborted", "turn_id":"older", "reason":"interrupted"})),
            event(
                json!({"type":"task_complete", "turn_id":"older", "last_agent_message":"older reply"}),
            ),
        ] {
            let items = vec![
                started("older"),
                started("waiting"),
                event(
                    json!({"type":"user_message", "message":"original task", "images":null, "local_images":[], "text_elements":[]}),
                ),
                terminal,
            ];
            let mut state = ThreadState::default();
            assert!(crate::extension::resume_bound_with(
                &mut state,
                &items,
                crate::resume_history::ResumeSource::Crash,
                None,
                0,
                now,
                |_| false,
            ));
            assert!(
                state.carry_on,
                "the retired terminal does not end the current native turn"
            );
            assert!(restore(&mut state, &items, &retry, false, now, 1_000));
            assert_eq!(
                state.carry_on_not_before,
                Some(now + std::time::Duration::from_secs(59)),
                "the existing carry-on must retain its matching provider wait"
            );
        }
    }

    #[test]
    fn signal_teardown_cannot_revive_an_attempt_an_earlier_terminal_ended() {
        let now = Instant::now();
        let retry = InFlightRetry {
            thread: "thread".into(),
            turn_id: "waiting".into(),
            not_before_ms: 60_000,
        };
        let abort = |turn: Option<&str>, reason: &str| {
            event(json!({"type":"turn_aborted", "turn_id":turn, "reason":reason}))
        };
        let complete = event(json!({"type":"task_complete", "turn_id":"waiting"}));
        let interrupted = abort(Some("waiting"), "interrupted");
        for (terminals, expected) in [
            (vec![interrupted.clone()], true),
            (
                vec![abort(Some("older"), "interrupted"), interrupted.clone()],
                true,
            ),
            (
                vec![interrupted.clone(), abort(Some("older"), "interrupted")],
                true,
            ),
            (vec![abort(None, "interrupted"), interrupted.clone()], false),
            (
                vec![abort(Some("unknown"), "interrupted"), interrupted.clone()],
                false,
            ),
            (
                vec![abort(Some("waiting"), "replaced"), interrupted.clone()],
                false,
            ),
            (vec![complete.clone(), interrupted.clone()], false),
            (vec![interrupted.clone(), abort(None, "interrupted")], false),
            (
                vec![interrupted.clone(), abort(Some("waiting"), "replaced")],
                false,
            ),
            (vec![interrupted.clone(), complete], false),
            (vec![interrupted.clone(), interrupted], false),
        ] {
            let mut items = vec![
                started("older"),
                started("waiting"),
                event(
                    json!({"type":"user_message", "message":"original task", "images":null, "local_images":[], "text_elements":[]}),
                ),
            ];
            items.extend(terminals);
            let mut state = ThreadState::default();
            assert!(crate::extension::resume_bound_with(
                &mut state,
                &items,
                crate::resume_history::ResumeSource::Signal,
                None,
                0,
                now,
                |_| false,
            ));
            assert_eq!(state.carry_on, expected, "{items:?}");
            // Check the pacing helper independently, even when native cancellation
            // correctly denied the outer carry-on before it could be called.
            state.observe_resume(true, None, true);
            assert_eq!(
                restore(&mut state, &items, &retry, true, now, 1_000),
                expected,
                "{items:?}"
            );
        }
    }
}
