// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! LAND'S HOLD GOES UP AS THE LAND ACT IS TYPED, on a loaded native core thread (review of gap
//! 11, finding 3: every land_status test booked its act by hand, so neither the stamp `submit`
//! books with the act nor the dispatch's tell after it was ever run): the evaluation types
//! Land's instruction through `submit`, the hold's `since` is the time booked with the act,
//! and aterm shows it while the act's own turn still runs (its model reply held back), never
//! only at that turn's end.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;

/// THE LAND ACT TYPED THROUGH `submit` SETS `since` TO ITS BOOKED TIME, SHOWN WHILE ITS TURN
/// RUNS: Land's instruction is typed (`land@v1` recorded), the thread's hold names the time
/// `submit` booked it at, between the evaluation's start and end, and the fake shows `since=`
/// that time with no nudge while the instruction's turn is still running. NEGATIVE CONTROL:
/// before the evaluation nothing is shown.
#[test]
fn the_land_act_typed_through_submit_shows_its_booked_since_while_its_turn_runs() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.fake.set_policy_key("upgrade", "false");
            h.fake.set_status("autopilot", "land");
            h.fake.set_policy_key("autopilot", "land");
            wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| policy.autopilot == decide::Mode::Land && !policy.upgrade)
            })
            .await;
            h.immediate_point().await;
            h.hold_model_response().await;
            // NEGATIVE CONTROL: nothing typed, nothing shown.
            h.autopilot.sup.show_land().await;
            assert_eq!(h.fake.land(), None);
            let before = now_ms();
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            let after = now_ms();
            assert!(
                h.fake.acts().iter().any(|act| act == words::RULE_LAND),
                "{:?} {:?}",
                h.fake.acts(),
                h.autopilot.sup.journal()
            );
            let since =
                h.rt.state()
                    .land
                    .hold
                    .since_ms
                    .unwrap_or_else(|| panic!("the act's booked time: {:?}", h.rt.state().land));
            assert!(
                before <= since && since <= after,
                "{before} {since} {after}"
            );
            let shown = codex_alab_aterm::LandReport {
                since_ms: since,
                nudges: 0,
                read: None,
            };
            assert!(
                h.fake
                    .wait_for(|fake| fake.land() == Some(shown.clone()))
                    .await,
                "{:?} {:?}",
                h.fake.requests(),
                h.autopilot.sup.journal()
            );
            assert!(
                h.shared.turn_running_on(h.rt.key()),
                "shown while the land act's turn runs, not at its end"
            );
            Autopilot::cancel_timer(&h.rt).await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("land act native hang detector");
    });
}
