// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A wait's announcement may finish after its native point or live policy retired.

use super::*;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_protocol::protocol::TurnAbortReason;
use std::future::Future;
use std::task::Poll;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delayed_wait_announcement_cannot_arm_its_retired_retry() {
    let mut observed = Vec::new();
    for change in [
        "unchanged",
        "native-start",
        "native-abort",
        "thread-stop",
        "retries-off",
        "off-epoch",
    ] {
        // Keep evaluation ownership here: a claim watcher must not race the
        // fixture's deliberate look at the server failure.

        let h = started_with_claim_watch(&[], |_, _| {}, false).await;
        wait_until("the claim is proven and acted under", || h.sup.acting()).await;
        let deadline = Instant::now() + Duration::from_secs(3600);
        point(
            &h,
            ended(
                None,
                Some(Wall::Server {
                    what: "model at capacity".into(),
                    cut_off: false,
                    retry_at: Some(deadline),
                }),
            ),
        );
        // The announcement's own wire write (`record_wait`'s `appnotice`) is held open.
        h.fake.gate("appnotice harness");
        let pilot = Arc::clone(&h.autopilot);
        let rt = Arc::clone(&h.rt);
        let mut evaluation = tokio::spawn(async move { pilot.evaluate(rt).await });
        assert!(
            h.fake
                .wait_for(|fake| fake
                    .gated_requests()
                    .iter()
                    .any(|line| { line.contains("appnotice harness") }))
                .await,
            "{change}: the actual wait announcement reached its wire gate"
        );
        // Take the next evaluation position before retiring the point. Any fresh
        // reconsideration then waits behind this observer instead of racing its verdict.
        let mut next = Box::pin(h.rt.hold_eval_when_free());
        std::future::poll_fn(|cx| {
            assert!(next.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;

        let session = ExtensionData::new("session");
        let successor = ExtensionData::new("new-native-turn");
        if matches!(change, "native-start" | "native-abort") {
            start_native_turn(&h, &session, &successor).await;
            if change == "native-abort" {
                h.autopilot
                    .on_turn_abort(TurnAbortInput {
                        reason: TurnAbortReason::Interrupted,
                        session_store: &session,
                        thread_store: &h._store,
                        turn_store: &successor,
                    })
                    .await;
            }
        } else if change == "thread-stop" {
            h.autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &session,
                    thread_store: &h._store,
                })
                .await;
        } else if change == "retries-off" {
            h.fake.set_policy_key("retry_api_errors", "false");
            wait_until("retry policy is disabled", || {
                h.sup
                    .policy()
                    .is_some_and(|policy| !policy.retry_api_errors)
            })
            .await;
        } else if change == "off-epoch" {
            // The existing Off epoch seam isolates an Off followed by the same
            // policy, without relying on a control connection timing out at its gate.
            h.sup.count_off();
        }
        h.fake.open_gate("appnotice harness");
        // Acquiring the observer's permit already waits for evaluation to end.
        // Keep its task handle outside the timeout so expiry cannot detach it.
        let completed = tokio::time::timeout(Duration::from_secs(60), async {
            let next = next.await;
            (next, (&mut evaluation).await)
        })
        .await;
        let timed_out = completed.is_err();
        if !matches!(&completed, Ok((_, Ok(())))) {
            if timed_out {
                evaluation.abort();
            }
            tokio::time::timeout(Duration::from_secs(60), async {
                if timed_out {
                    let _ = evaluation.await;
                }
                Autopilot::cancel_timer(&h.rt).await;
            })
            .await
            .expect("failed evaluation cleanup hang detector");
        }
        let (next, evaluated) = completed.expect("wait evaluation hang detector");
        evaluated.expect("wait evaluation task");
        let owed = h.rt.owed_timer();
        let timer = h.rt.timer_pending();
        let record = h.shared.owed_act();
        let kept = if change == "unchanged" {
            owed.as_ref().is_some_and(|owed| {
                owed.rule == crate::words::RULE_API_RETRY && owed.not_before == deadline
            }) && timer
                && record.is_some()
        } else {
            owed.is_none()
                && record.is_none()
                && (matches!(change, "native-abort" | "retries-off" | "off-epoch") || !timer)
        };
        // Retire the actual timer before the collected verdict, on RED too.
        tokio::time::timeout(Duration::from_secs(60), Autopilot::cancel_timer(&h.rt))
            .await
            .expect("wait timer cleanup hang detector");
        let announced = h
            .sup
            .journal()
            .iter()
            .any(|line| line.starts_with("wait: "));
        observed.push((change, kept, owed, timer, record, announced));
        drop(next);
        let root = h._dir.keep();
        eprintln!(
            "retained recovery wait fixture ({change}): {}",
            root.display()
        );
    }
    assert!(observed.iter().all(|(_, kept, ..)| *kept), "{observed:#?}");
    assert!(
        observed.iter().all(|(_, _, _, _, _, announced)| *announced),
        "automatic waits retain their diagnostic record without a persistent warning: {observed:#?}"
    );
}
