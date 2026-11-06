// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ATERM'S FACT AND AN ACT WHOSE TURN DID NOT START (CONTRACT §5 "aterm fact"; gap 23's §9.7),
//! on a loaded native core thread and the fake aterm: the fact is read with the act and its
//! rider composed, but a person's message takes the thread between the act's checks and its
//! start, so core refuses the turn (`NotSubmitted`). Nothing is acknowledged, and aterm's lease
//! lets the same thread's next act take the same episode again. The end-to-end rides are
//! `tests/autopilot/fact_rider.rs`.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;

const HANG: Duration = Duration::from_secs(60);

/// The sentence aterm's jobs board names a stalled background call with.
const STALLED: &str = "a background call (pid 51240) has made no progress for 6 min";

/// A TURN THAT DID NOT START ACKNOWLEDGES NOTHING, AND THE NEXT ACT CARRIES THE FACT: the act
/// reads the fact and reaches its start; a person's message takes the thread; core refuses the
/// act's turn, so no `harness fact told` is sent and aterm keeps the episode untold. The same
/// thread's next act, within aterm's 30 s lease, reads the same episode again (a reader takes its
/// own offer again), types it and acknowledges it. That next act is Cruise carrying on after the
/// person stops their own turn (Esc, the grace 0 here): FOREVER CRUISE never parks there. Kills
/// M29 (an ack before `Started`) and M39 (a lease that blocks its own reader: the next act would
/// be told `offered` and go bare).
#[test]
fn a_turn_that_did_not_start_acknowledges_nothing() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let h = Harness::new().await;
            h.fake.set_policy_key("upgrade", "false");
            h.fake.set_policy_key("facts", "true");
            wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| !policy.upgrade && policy.facts)
            })
            .await;
            let episode = h.fake.plant_fact(STALLED);
            let (entered, at_start) = tokio::sync::oneshot::channel();
            let (release, released) = tokio::sync::oneshot::channel();
            *lock(&h.rt.before_start) = Some((entered, released));
            h.hold_model_response().await;
            h.immediate_point().await;
            let pilot = Arc::clone(&h.autopilot);
            let rt = Arc::clone(&h.rt);
            let evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
            at_start.await.expect("the act reaches its start");
            assert_eq!(
                h.fake.fact_reads(),
                [(words::RULE_CONTINUE.to_string(), h.rt.key().to_string())],
                "the act read its fact"
            );
            h.test
                .codex
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "The person's own message.".into(),
                    text_elements: Vec::new(),
                }]))
                .await
                .expect("the person's message is admitted");
            wait_for_supervisor(|| h.shared.turn_running_on(h.rt.key())).await;
            release.send(()).expect("the act's start resumes");
            evaluating.await.expect("the refused act's evaluation ends");
            assert!(
                h.fake.facts_told().is_empty(),
                "no ack for a turn that did not start"
            );
            assert!(
                !h.fake
                    .requests()
                    .iter()
                    .any(|line| line.contains("harness fact told")),
                "{:?}",
                h.fake.requests()
            );
            // The person stops their own turn (interrupted, its reply held back). FOREVER CRUISE
            // carries on once the person's grace (0 here) has passed, so that carry-on is the
            // thread's next act, still within aterm's lease. It starts at once, so the aborted
            // point is the thread's only between two polls: the act's ack is what is awaited.
            h.test
                .codex
                .submit(codex_protocol::protocol::Op::Interrupt)
                .await
                .expect("the person's turn is interrupted");
            wait_for_supervisor(|| h.fake.facts_told() == [episode]).await;
            assert!(
                h.rt.state().esc_at.is_some(),
                "the carry-on decided the person's Esc"
            );
            let reads = h.fake.fact_reads();
            assert_eq!(reads.len(), 2, "{reads:?}");
            assert!(reads.iter().all(|(_, reader)| reader == h.rt.key()));
            let journal = h.autopilot.sup.journal();
            assert!(
                journal.iter().any(|line| line.contains(&format!(
                    "typed \"keep going{}\"",
                    codex_alab_aterm::facts::rider(STALLED)
                ))),
                "the next act carried the fact: {journal:?}"
            );
            Autopilot::cancel_timer(&h.rt).await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("refused act hang detector");
    });
}
