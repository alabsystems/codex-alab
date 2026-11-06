// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A coherent Off and On can reclaim before the release's older event is handled.

use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_own_release_queued_before_a_fresh_claim_does_not_clear_it() {
    for case in ["ordinary", "person", "gap", "own-echo"] {
        let mut h = started_with(
            &[],
            |fake, sup| {
                fake.set_status("autopilot", "cruise");
                sup.set_policy_recheck(Duration::from_secs(3600));
                sup.set_recheck(Duration::from_millis(20));
            },
            true,
            ClientOptions {
                timeout: Duration::from_secs(60),
                ..ClientOptions::default()
            },
        )
        .await;
        h._dir.disable_cleanup(true);
        eprintln!("{case} fixture: {}", h._dir.path().display());
        wait_until("the initial proven claim acts", || h.sup.acting()).await;
        wait_go(&h.sup).await;
        let claimed = noted(&h, CLAIMED);
        let returned = noted(&h, "gave the claim back (");
        let epoch = h.sup.epochs().0;
        let delayed = matches!(case, "gap" | "own-echo");
        if delayed {
            h.fake.delay_clear_event();
        }
        h.fake.set_status("autopilot", "off");
        h.fake.set_effective(false, "autopilot-off");
        wait_until("the actual Off releases its proven claim", || {
            noted(&h, "gave the claim back (") > returned
                && !h.sup.held()
                && h.fake.claim().is_none()
        })
        .await;
        assert!(!h.sup.acting(), "{case}: {:?}", h.sup.journal());

        // A new claim must reconcile its keyed attention. Hold that real write after
        // the proof, so the reader can queue a late clear before the owning loop's
        // check_boxes calls ready_for_box and then dequeues the clear.
        let met = h.fake.gated_requests().len();
        h.fake.gate("meta unset attention");
        h.shared.report_box(question_box(&h));
        h.fake.set_status("autopilot", "cruise");
        if case == "gap" {
            // Gap discards old echo credit. Its ordinary refresh can reclaim, but
            // a later value=- must still be treated conservatively as a clear.
            // emit_frame does not flush FakeAterm's delayed ordinary event.
            h.fake.set_effective_quietly(true, "enabled");
            h.fake.emit_frame("GAP 1 events");
        } else {
            // This policy event flushes the held release immediately after itself.
            // The controller handles On first, proving a fresh claim, while the
            // independent reader can already have queued the older clear.
            h.fake.set_effective(true, "enabled");
        }
        wait_until(
            "the fresh proven claim reconciles attention at the gate",
            || {
                noted(&h, CLAIMED) > claimed
                    && h.fake
                        .gated_requests()
                        .iter()
                        .skip(met)
                        .any(|request| request.contains("meta unset attention"))
            },
        )
        .await;
        let client = h.sup.client().expect("the claim's actual connection");
        assert!(h.sup.acting() && h.sup.held(), "{case}");
        assert!(client.holds_claim(), "{case}");
        assert!(!client.reconfirmed_since_claim(), "{case}");
        if case == "person" {
            // No release debt remains: this is an actual person's clear after
            // the new claim was independently proven, not an emitted imitation.
            let before = h.sup.epochs().0;
            h.fake.clear_claim();
            wait_until("the reader queues the person's clear", || {
                h.sup.epochs().0 > before
            })
            .await;
        } else if delayed {
            wait_until("the reader queues the older own release", || {
                h.sup.epochs().0 > epoch
            })
            .await;
        }
        h.fake.open_gate("meta unset attention");

        if matches!(case, "person" | "gap") {
            wait_until("an untrusted clear stays given up", || {
                !h.sup.held()
                    && (case == "gap" || h.fake.claim().is_none())
                    && (noted(&h, "the claim was cleared: not taken again") > 0
                        || noted(&h, "status shows no proven claim of ours") > 0
                        || noted(&h, STAYS_CLEARED) > 0)
            })
            .await;
            assert!(!h.sup.acting(), "{case}: {:?}", h.sup.journal());
            assert!(!h.shared.box_answer_pending(), "{case}");
            eprintln!("{case} control: clear preserved; {:?}", h.sup.journal());
            continue;
        }
        if delayed {
            wait_until("the owning loop handles its queued release echo", || {
                noted(&h, "came after it claimed again: the claim stands") > 0
                    || noted(&h, "the claim was cleared: not taken again") > 0
            })
            .await;
        } else {
            wait_until("the ordinary reclaimed box receives its answer", || {
                h.shared.box_answer_pending()
            })
            .await;
        }
        eprintln!(
            "{case}: held={} active={} reconfirmed={} native={:?}; {:?}",
            h.sup.held(),
            h.sup.acting(),
            client.reconfirmed_since_claim(),
            h.fake.claim(),
            h.sup.journal()
        );
        assert!(
            h.sup.held() && h.sup.acting(),
            "a coherent Off/On must retain its fresh claim after its own older release: \
             {case}: {:?}",
            h.sup.journal()
        );
        wait_until("the surviving claim answers the queued box", || {
            h.shared.box_answer_pending()
        })
        .await;
        assert!(h.fake.acts().is_empty(), "no native turn was submitted");
    }
}
