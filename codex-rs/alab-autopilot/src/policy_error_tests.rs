// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A supported connection remains retryable after an unheld policy refusal.

use super::*;
use crate::decide::Mode as AutopilotWord;

const UNREADABLE: &str = "ERR internal: the policy cell is busy";

async fn policy_fixture(off: bool, unsupported: bool) -> Harness {
    let mut h = started_with_claim_watch(
        &[],
        |fake, sup| {
            fake.set_status("autopilot", "land");
            if off {
                fake.set_effective(false, "autopilot-off");
            }
            if unsupported {
                fake.answer_policy_with(Some("ERR unknown verb (try: help)"));
            }
            sup.set_word_recheck(Duration::from_secs(3600));
            sup.set_policy_recheck(Duration::from_millis(20));
        },
        false,
    )
    .await;
    h._dir.disable_cleanup(true);
    eprintln!("retained policy error fixture: {}", h._dir.path().display());
    h
}

fn policy_reads(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|request| request.ends_with(" harness policy"))
        .count()
}

async fn read_supported_off(h: &Harness) {
    wait_until("the supported Off policy is read", || {
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("effective=off why=autopilot-off: passive, unclaimed"))
    })
    .await;
    assert!(!h.sup.held() && h.fake.claim().is_none());
}

async fn quiet_land(h: &Harness) {
    h.fake.answer_policy_with(None);
    h.fake.set_status("autopilot", "land");
    h.fake.set_effective_quietly(true, "enabled");
    wait_until(
        "the passive clock recovers Land without another event",
        || {
            h.sup.acting()
                && h.sup
                    .policy()
                    .is_some_and(|policy| policy.autopilot == AutopilotWord::Land)
        },
    )
    .await;
    assert!(h.sup.held() && h.fake.claim().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_policy_error_after_status_off_keeps_the_passive_clock_and_landing() {
    let h = policy_fixture(false, false).await;
    wait_until("the original supported claim acts under Land", || {
        h.sup.acting()
            && h.sup
                .policy()
                .is_some_and(|policy| policy.autopilot == AutopilotWord::Land)
    })
    .await;
    landed_at_defense(&h);
    let offs = h.sup.offs();
    h.fake.answer_policy_with(Some(UNREADABLE));
    h.fake.set_status("autopilot", "off");
    let ready = h.sup.ready().await;
    assert_ne!(ready, Ready::Go);
    assert!(!h.sup.held() && !h.sup.acting() && h.sup.offs() > offs);
    let read = policy_reads(&h);
    // Two retries cannot be supplied by a single timer armed before the refusal.
    wait_until(
        "the released supported connection retries its policy error",
        || policy_reads(&h) >= read + 2,
    )
    .await;
    quiet_land(&h).await;
    assert!(h.rt.state().landed, "a read error cannot end the landing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_policy_error_after_supported_off_before_any_claim_remains_retryable() {
    let h = policy_fixture(true, false).await;
    read_supported_off(&h).await;
    h.fake.answer_policy_with(Some(UNREADABLE));
    let read = policy_reads(&h);
    h.fake
        .set_policy_key("answer_text", "Read the transient policy error.");
    wait_until(
        "the unclaimed supported connection retries its policy error",
        || policy_reads(&h) >= read + 3,
    )
    .await;
    assert!(!h.sup.held() && h.fake.claim().is_none());
    quiet_land(&h).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_supported_policy_event_retires_initial_unsupported_before_quiet_on() {
    let h = policy_fixture(true, true).await;
    wait_until("the initial unsupported host is recognized", || {
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("has no harness policy"))
    })
    .await;
    assert!(!h.sup.held() && h.fake.claim().is_none());
    assert!(
        h.fake
            .requests()
            .iter()
            .all(|request| !request.contains("meta set supervisor ")),
        "a host that has never served policy must not be claimed"
    );
    // Only this explicit event introduces support. It still says Off, so it
    // grants no claim; the following On is silent and must use the clock.
    h.fake.answer_policy_with(None);
    h.fake.set_effective(false, "autopilot-off");
    read_supported_off(&h).await;
    quiet_land(&h).await;
}
