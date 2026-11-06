// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The supervisor's `harness fact` read and its once-per-claim latch (CONTRACT §5 "aterm fact";
//! gap 23), against the fake aterm: an aterm before the verb is asked once under a claim and once
//! again under the next, and a read is sent only on a held claim codex-alab acts under.

use super::*;

/// The sentence aterm's jobs board names a stalled background call with.
const STALLED: &str = "a background call (pid 51240) has made no progress for 6 min";

/// AN ATERM BEFORE THE VERB IS ASKED ONCE PER CLAIM, AND ONCE AGAIN UNDER A NEW ONE: its usage
/// names no `fact` form, so the claim's next read sends nothing; a person's clear and the policy
/// that moves after it bring a new claim (as a newer aterm holds after an update), which asks
/// once. A later aterm with the verb is not asked under the latched claim either. Kills M26
/// (asking at every act) and M27 (never asking again).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_aterm_before_the_verb_is_asked_once_per_claim_and_again_after_a_new_one() {
    let h = harness(&[("facts", "true")]).await;
    h.fake.support_harness_fact(false);
    h.fake.plant_fact(STALLED);
    let read = || h.sup.fact_for(crate::words::RULE_CONTINUE, h.rt.key(), "-");
    assert_eq!(read().await, None);
    assert_eq!(read().await, None);
    assert_eq!(h.fake.fact_reads().len(), 1, "asked once under the claim");
    // A person's `meta unset supervisor`, then the policy moves: a new claim.
    h.fake.clear_claim();
    wait_until("the cleared claim is given up", || !h.sup.held()).await;
    h.fake.set_policy_key("facts", "true");
    wait_until("claimed again and acting", || h.sup.acting()).await;
    assert_eq!(read().await, None);
    assert_eq!(h.fake.fact_reads().len(), 2, "the new claim asks once");
    h.fake.support_harness_fact(true);
    assert_eq!(read().await, None);
    assert_eq!(
        h.fake.fact_reads().len(),
        2,
        "still latched under this claim"
    );
}

/// A READ IS SENT ONLY ON A HELD CLAIM CODEX-ALAB ACTS UNDER: while the claim is gone nothing is
/// sent; acting again, the read answers aterm's fact, which the ack then tells.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fact_is_read_only_while_codex_alab_acts_under_its_claim() {
    let h = harness(&[("facts", "true")]).await;
    let episode = h.fake.plant_fact(STALLED);
    h.fake.clear_claim();
    wait_until("the cleared claim is given up", || !h.sup.held()).await;
    let read = || h.sup.fact_for(crate::words::RULE_CONTINUE, h.rt.key(), "-");
    assert_eq!(read().await, None);
    assert!(h.fake.fact_reads().is_empty(), "nothing sent unclaimed");
    h.fake.set_policy_key("facts", "true");
    wait_until("claimed again and acting", || h.sup.acting()).await;
    let fact = read().await.expect("aterm's fact");
    assert_eq!(fact.episode, episode);
    assert_eq!(fact.text, STALLED);
    h.sup.fact_told(&fact).await;
    assert_eq!(h.fake.facts_told(), [episode]);
}
