// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The latest explicit Off withholds an act before its policy event is processed.

use super::*;
use crate::decide::Mode as AutopilotWord;

async fn landed_without_claim_evaluations() -> Harness {
    landed_with_clocks(Duration::from_secs(3600), Duration::from_secs(30)).await
}

async fn landed_with_clocks(word_recheck: Duration, policy_recheck: Duration) -> Harness {
    let mut h = started_with_claim_watch(
        &[],
        |fake, sup| {
            fake.set_status("autopilot", "land");
            sup.set_word_recheck(word_recheck);
            sup.set_policy_recheck(policy_recheck);
        },
        false,
    )
    .await;
    h._dir.disable_cleanup(true);
    eprintln!("retained Off fixture: {}", h._dir.path().display());
    wait_until("the proven claim acts under Land", || {
        h.sup.acting()
            && h.sup
                .policy()
                .is_some_and(|p| p.autopilot == AutopilotWord::Land)
    })
    .await;
    landed_at_defense(&h);
    h
}

async fn arm_word_clock(h: &Harness) {
    h.fake.set_status("autopilot", "-");
    h.fake.set_policy_key("answer_text", "Prime word clock.");
    wait_until("the owning loop reads the missing word", || {
        h.sup
            .policy()
            .is_some_and(|p| p.answer_text.as_deref() == Some("Prime word clock."))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quiet_off_word_clock_reconciles_effective_off_without_an_act_or_event() {
    let h = landed_with_clocks(Duration::from_millis(20), Duration::from_millis(20)).await;
    let offs = h.sup.offs();
    arm_word_clock(&h).await;
    h.fake.set_status("autopilot", "off");
    h.fake.set_effective_quietly(false, "autopilot-off");
    wait_until("the word clock reconciles quiet Off", || {
        h.sup.offs() > offs && !h.sup.held() && !h.sup.acting()
    })
    .await;
    h.fake.set_status("autopilot", "land");
    h.fake.set_effective(true, "enabled");
    wait_go(&h.sup).await;
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(AutopilotWord::Land)
    );
    assert!(h.rt.state().landed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quiet_off_word_clock_keeps_rechecking_contradictory_on_policy() {
    let h = landed_with_clocks(Duration::from_millis(20), Duration::from_millis(20)).await;
    let offs = h.sup.offs();
    let reads = || {
        h.fake
            .requests()
            .iter()
            .filter(|request| request.ends_with(" harness policy"))
            .count()
    };
    arm_word_clock(&h).await;
    let before = reads();
    h.fake.set_status("autopilot", "off");
    wait_until("the word clock first revokes quiet Off", || {
        h.sup.offs() > offs
    })
    .await;
    // A second release in one reconciliation is not another clock iteration.
    wait_until(
        "the passive backstop re-reads the contradictory policy",
        || reads() > before + 1,
    )
    .await;
    let first = h.sup.offs();
    wait_until("Off remains retryable after the stale On re-claim", || {
        h.sup.offs() > first
    })
    .await;
    h.fake.set_status("autopilot", "land");
    wait_go(&h.sup).await;
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(AutopilotWord::Land)
    );
    assert!(h.rt.state().landed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quiet_off_word_clock_keeps_the_hold_then_reconciles_its_quiet_lift() {
    let h = landed_with_clocks(Duration::from_millis(20), Duration::from_millis(20)).await;
    let offs = h.sup.offs();
    arm_word_clock(&h).await;
    h.fake.set_status("hold", "1");
    h.fake.set_status("autopilot", "off");
    h.fake.set_effective_quietly(false, "hold");
    wait_until(
        "the word clock reconciles the hold without releasing",
        || {
            h.sup
                .journal()
                .iter()
                .any(|line| line.contains("policy effective=off why=hold: the claim is kept"))
        },
    )
    .await;
    assert_eq!(h.sup.offs(), offs);
    assert!(h.sup.held());
    assert!(h.rt.state().landed);
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(AutopilotWord::Land)
    );
    h.fake.set_status("hold", "0");
    h.fake.set_effective_quietly(false, "autopilot-off");
    wait_until("the quiet hold lift now meets Off", || {
        h.sup.offs() > offs && !h.sup.held()
    })
    .await;
    h.fake.set_status("autopilot", "land");
    h.fake.set_effective(true, "enabled");
    wait_go(&h.sup).await;
    assert!(h.rt.state().landed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_status_off_withholds_turn_and_box_with_stale_on_policy() {
    let h = landed_without_claim_evaluations().await;
    let before = h.rt.state();
    // The status read sees Off before the queued policy refresh has changed the
    // supervisor's effective-on policy. Keep that stale policy deliberately.
    h.fake.set_status("autopilot", "off");
    let turn = h.sup.ready().await;
    let boxed = h.sup.ready_for_box().await;
    let passive = !h.sup.acting() && !h.sup.held();
    let after = h.rt.state();
    h.fake.set_status("autopilot", "-");
    h.fake.set_effective(true, "enabled");
    wait_go(&h.sup).await;
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(AutopilotWord::Land)
    );
    assert_eq!(
        after, before,
        "Off must not reinterpret the landed task as Cruise"
    );
    assert!(
        turn != Ready::Go && boxed != Ready::Go && passive,
        "explicit Off granted an act or left an active claim: turn={turn:?}, box={boxed:?}, passive={passive}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_status_off_refreshes_quiet_effective_off_and_recovers_land() {
    let h = landed_without_claim_evaluations().await;
    let offs = h.sup.offs();
    h.fake.set_status("autopilot", "off");
    h.fake.set_effective_quietly(false, "autopilot-off");
    let ready = h.sup.ready().await;
    let released = !h.sup.acting() && h.fake.claim().is_none();
    let off_seen = h.sup.offs() > offs;
    h.fake.set_status("autopilot", "land");
    h.fake.set_effective(true, "enabled");
    wait_go(&h.sup).await;
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(AutopilotWord::Land)
    );
    assert!(
        h.rt.state().landed,
        "the remembered landing survives Off and back"
    );
    assert!(
        ready != Ready::Go && released && off_seen,
        "quiet Off was not reconciled: ready={ready:?}, released={released}, off_seen={off_seen}"
    );
}
