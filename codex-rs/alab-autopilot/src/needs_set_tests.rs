// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE WORKER'S NEEDS WHOLE IN ATERM (gap 16 of the parity sweep, 2026-10-28: a codex NEEDS YOU
//! showed only its first need and a count in aterm), against alab-aterm's in-process FAKE aterm
//! (never a real one), each driven through the extension's turns as a report reaches it: the
//! set a report names goes up whole and in order as `meta set harness_needs entry=… need=…`
//! beside the `owner=needs` entry it describes, on the claim's connection while the proven
//! claim is held, once per change; a replacing report replaces it, its first need and count
//! unchanged included; a report's `NEEDS YOU: nothing`, a person's turn's end that met no
//! wall and the exit take it down, the exit's after its entry and before its release, and an
//! Off's release takes it down with the claim; a new claim sends the standing set again, and
//! takes down one aterm kept for this process; an aterm before the verb is asked once per
//! change, a denied set not again under its claim; nothing is sent without a proven claim; 33
//! needs send the first 32. Each with its negative control.

use codex_alab_aterm::fake::FakeNeeds;
use codex_alab_aterm::wire::pct_encode;

use super::*;

/// The `harness_needs` sets the fake took.
fn needs_sets(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.contains(" meta set harness_needs "))
        .count()
}

/// The `harness_needs` unsets the fake took.
fn needs_unsets(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.ends_with(" meta unset harness_needs"))
        .count()
}

/// Every `harness_needs` request the fake took.
fn needs_requests(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.contains(" harness_needs"))
        .count()
}

/// The `owner=needs` entry the fake shows.
fn entry(h: &Harness) -> Option<String> {
    h.fake
        .attention()
        .get(codex_alab_aterm::NEEDS_OWNER)
        .cloned()
}

/// A report naming `needs`, one NEEDS YOU line each.
fn report_of(needs: &[&str]) -> String {
    let mut text = "Stage 2 is in.".to_string();
    for need in needs {
        text.push_str("\nNEEDS YOU: ");
        text.push_str(need);
    }
    text
}

fn owned(needs: &[&str]) -> Vec<String> {
    needs.iter().map(|need| (*need).to_string()).collect()
}

/// [`harness`], once the claim's first sync settled what aterm may keep for this process: no
/// need stands, so it takes it down.
async fn settled() -> Harness {
    let h = harness(&[]).await;
    wait_until("the first claim's unset", || needs_unsets(&h) == 1).await;
    h
}

/// codex-alab's own turn reports `needs`, and aterm's readers read them whole.
async fn told(h: &Harness, id: &str, needs: &[&str]) {
    turn(h, id, true, &report_of(needs)).await;
    let want = owned(needs);
    assert!(
        h.fake
            .wait_for(|fake| fake.needs_told().as_ref() == Some(&want))
            .await,
        "{:?} {:?}",
        h.fake.requests(),
        h.sup.journal()
    );
}

/// Three needs, the first past the entry's [`crate::supervisor::NEED_CELLS`] (93 cells).
const THREE: [&str; 3] = [
    "the release credential for alabsystems, from the vault on this Mac or one minted for this cut",
    "a decision: ship 0.5 now or wait for the reader",
    "50% more of the API budget \u{2014} or a plan change",
];

/// A REPORT'S NEEDS REACH ATERM WHOLE AND IN ORDER, WITH THEIR ENTRY: three needs a report
/// names go up as one `harness_needs` set beside the `owner=needs` entry, which says the first,
/// clipped at [`crate::supervisor::NEED_CELLS`] with `…`, and `(+2 more)`; the set carries each
/// need whole (the first past the entry's clip, a colon, a `%` and a dash included); a sync
/// again, and the same set reported again, send nothing more. NEGATIVE CONTROL: before the
/// report aterm's readers read no set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reports_needs_reach_aterm_whole_and_in_order() {
    assert!(THREE[0].chars().count() > crate::supervisor::NEED_CELLS);
    let h = settled().await;
    // NEGATIVE CONTROL.
    assert_eq!(h.fake.needs_told(), None);
    assert_eq!(needs_sets(&h), 0);
    told(&h, "t1", &THREE).await;
    let shown = entry(&h).unwrap_or_else(|| panic!("an entry"));
    assert_eq!(
        shown,
        "Codex needs you: the release credential for alabsystems, from the vault on this\u{2026} \
         (+2 more) \u{2014} carrying on"
    );
    assert_eq!(
        h.fake.needs_told_last(),
        Some(FakeNeeds {
            entry: shown,
            needs: owned(&THREE)
        })
    );
    assert_eq!(needs_sets(&h), 1, "{:?}", h.fake.requests());
    h.sup.show_needs().await;
    turn(&h, "t2", true, &report_of(&THREE)).await;
    h.sup.show_needs().await;
    assert_eq!(needs_sets(&h), 1, "{:?}", h.fake.requests());
    wait_until("the set told is noted", || {
        noted(&h, "needs told whole: 3") == 1
    })
    .await;
}

/// A REPLACING REPORT REPLACES THEM: a report naming another set replaces the entry and the set
/// together; one whose first need and count are the ones shown leaves the entry's words as they
/// are, and the set it names, whole, is what aterm's readers read. NEGATIVE CONTROL: that report
/// writes no entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replacing_report_replaces_them() {
    let h = settled().await;
    told(&h, "t1", &THREE).await;
    let entries = |h: &Harness| {
        h.fake
            .requests()
            .iter()
            .filter(|r| r.contains(" meta set attention owner=needs "))
            .count()
    };
    told(
        &h,
        "t2",
        &["the signing key password", "a reviewer for #42"],
    )
    .await;
    assert_eq!(
        entry(&h).as_deref(),
        Some("Codex needs you: the signing key password (+1 more) \u{2014} carrying on")
    );
    let written = entries(&h);
    told(
        &h,
        "t3",
        &["the signing key password", "a reviewer for #43"],
    )
    .await;
    // NEGATIVE CONTROL: the entry's words did not move.
    assert_eq!(entries(&h), written, "{:?}", h.fake.requests());
    assert_eq!(needs_sets(&h), 3, "{:?}", h.fake.requests());
}

/// THE SET COMES DOWN WHERE THE ENTRY DOES: a report's `NEEDS YOU: nothing` takes the entry
/// down and the set with it; so does a person's turn whose end met no wall, which answered what
/// stood. NEGATIVE CONTROL: codex-alab's own report that says nothing of its needs leaves both
/// standing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_and_a_persons_turn_take_the_set_down() {
    let h = settled().await;
    told(&h, "t1", &THREE).await;
    // NEGATIVE CONTROL.
    turn(&h, "t2", true, "Stage 3 is in.").await;
    h.sup.show_needs().await;
    assert_eq!(h.fake.needs_told(), Some(owned(&THREE)));
    assert_eq!(needs_unsets(&h), 1);
    turn(&h, "t3", true, "Stage 4 is in.\nNEEDS YOU: nothing").await;
    assert!(
        h.fake
            .wait_for(|fake| fake.needs_told_last().is_none())
            .await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(entry(&h), None);
    assert_eq!(needs_unsets(&h), 2);
    told(&h, "t4", &THREE).await;
    turn(&h, "t5", false, "I rotated the credential myself.").await;
    assert!(
        h.fake
            .wait_for(|fake| fake.needs_told_last().is_none())
            .await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(entry(&h), None);
    assert_eq!(needs_unsets(&h), 3);
}

/// AN OFF'S RELEASE TAKES THE SET DOWN: the Off's entry stands until after its release, and
/// aterm keeps reading a set beside the entry it says, an unset's outgoing one included
/// (`ToldNeeds::tell`), so the Off sends no `harness_needs` and its release, which drops the
/// set with the claim, takes it down; the entry comes down after it. NEGATIVE CONTROL: up to
/// the release aterm's readers read the set whole beside the entry, as they did before the Off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_offs_release_takes_the_set_down() {
    const RELEASE: &str = "meta unset supervisor";
    let h = settled().await;
    told(&h, "t1", &THREE).await;
    let asked = needs_requests(&h);
    h.fake.gate(RELEASE);
    h.fake.set_effective(false, "autopilot-off");
    wait_until("the Off's release is held at aterm", || {
        h.fake.gated_requests().iter().any(|r| r.contains(RELEASE))
    })
    .await;
    assert!(h.fake.claim().is_some(), "the release is not applied yet");
    // NEGATIVE CONTROL.
    assert!(entry(&h).is_some());
    assert_eq!(h.fake.needs_told(), Some(owned(&THREE)));
    assert_eq!(needs_requests(&h), asked, "{:?}", h.fake.requests());
    h.fake.open_gate(RELEASE);
    wait_until("released, the entry down", || {
        h.fake.claim().is_none() && entry(&h).is_none() && !h.sup.acting()
    })
    .await;
    assert_eq!(h.fake.needs_told(), None);
    assert_eq!(h.fake.needs_told_last(), None, "the release dropped it");
    assert_eq!(needs_requests(&h), asked, "{:?}", h.fake.requests());
    // When the Off's entry clear is refused once and a late report while unclaimed names a set
    // whose entry words match `needs_shown`, the unclaimed sync sends nothing and the next
    // claim still tells the new set whole.
    act_again(&h).await;
    told(
        &h,
        "t2",
        &["the signing key password", "a reviewer for #42"],
    )
    .await;
    h.fake.reply_once(
        &format!("@{} meta unset attention owner=needs", h.fake.sid()),
        "ERR refused\n",
    );
    h.fake.set_effective(false, "autopilot-off");
    wait_until("released after Off, entry clear refused once", || {
        h.fake.claim().is_none() && !h.sup.held() && entry(&h).is_some()
    })
    .await;
    let same_entry = owned(&["the signing key password", "a reviewer for #43"]);
    assert!(
        h.sup
            .want_needs(&same_entry, crate::needs::needs_key(&same_entry))
    );
    let before = needs_requests(&h);
    h.sup.show_needs().await;
    assert_eq!(needs_requests(&h), before);
    act_again(&h).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.needs_told().as_ref() == Some(&same_entry))
            .await,
        "{:?}",
        h.fake.requests()
    );
}

/// THE EXIT TAKES THE SET DOWN ON THE CLAIM IT STILL HOLDS, after the entry and before its
/// release. NEGATIVE CONTROL: the set stands up to the exit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_takes_the_set_down_before_its_release() {
    const RELEASE: &str = "meta unset supervisor";
    let h = settled().await;
    told(&h, "t1", &THREE).await;
    // NEGATIVE CONTROL.
    assert!(h.fake.needs_told_last().is_some());
    h.fake.gate(RELEASE);
    let shared = h.shared;
    let release =
        tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(60)));
    wait_until("the exit's release is held at aterm", || {
        h.fake.gated_requests().iter().any(|r| r.contains(RELEASE))
    })
    .await;
    assert!(h.fake.claim().is_some(), "the release is not applied yet");
    assert_eq!(entry(&h), None);
    assert_eq!(h.fake.needs_told_last(), None, "{:?}", h.fake.requests());
    h.fake.open_gate(RELEASE);
    let released = release
        .await
        .unwrap_or_else(|err| panic!("the release: {err}"));
    assert!(released, "{:?}", h.sup.journal());
    assert!(h.fake.claim().is_none());
}

/// THE EXIT TAKES AN IN-FLIGHT SET DOWN BEFORE ITS RELEASE: when a claim's sync is mid-write on
/// the connection (`meta set harness_needs` already queued on `Conn`, its caller not yet
/// answered or dropped mid-await as the session retires), neither the next sync nor the exit
/// can read the earlier unset's `Shown(None)` as proof nothing stands. The claim connection
/// orders the exit's take-down after that set and before its release.
/// NEGATIVE CONTROL: each in-flight `meta set harness_needs` was applied at aterm before the
/// unset cleared it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_takes_an_in_flight_set_down_before_its_release() {
    const SET_NEEDS: &str = "meta set harness_needs";
    const RELEASE: &str = "meta unset supervisor";
    let h = harness_within(&[], Duration::from_secs(60)).await;
    wait_until("the first claim's unset", || needs_unsets(&h) == 1).await;
    // 1. A caller dropped mid-await while `meta set harness_needs` is held at aterm still
    // unsettles `needs_told_sent`, so taking the entry down takes the set down too.
    let set = owned(&THREE);
    assert!(h.sup.want_needs(&set, crate::needs::needs_key(&set)));
    h.fake.gate(SET_NEEDS);
    let dropped = tokio::spawn({
        let sup = Arc::clone(&h.sup);
        async move { sup.show_needs().await }
    });
    wait_until("the first set is held at aterm", || {
        h.fake
            .gated_requests()
            .iter()
            .any(|r| r.contains(SET_NEEDS))
    })
    .await;
    dropped.abort();
    let _ = dropped.await;
    h.fake.open_gate(SET_NEEDS);
    wait_until("aterm applied the aborted caller's set", || {
        h.fake.needs_told_last().is_some()
    })
    .await;
    assert_eq!(needs_sets(&h), 1, "{:?}", h.fake.requests());
    h.sup.needs(&[], 0).await;
    assert_eq!(h.fake.needs_told_last(), None, "{:?}", h.fake.requests());
    assert_eq!(needs_unsets(&h), 2, "{:?}", h.fake.requests());
    // 2. A sync still in flight when the exit begins completes under `needs_told_sync` and is
    // taken down before `meta unset supervisor`.
    h.fake.gate(SET_NEEDS);
    turn(&h, "t1", true, &report_of(&THREE)).await;
    wait_until("the report's needs set is held at aterm", || {
        h.fake.gated_requests().len() == 2
    })
    .await;
    h.fake.gate(RELEASE);
    let shared = h.shared;
    let release =
        tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(60)));
    wait_until("closing", || h.sup.closing()).await;
    h.fake.open_gate(SET_NEEDS);
    wait_until("the exit's release is held at aterm", || {
        h.fake.gated_requests().iter().any(|r| r.contains(RELEASE))
    })
    .await;
    // NEGATIVE CONTROL: the report's `meta set harness_needs` ran before the exit's release.
    assert_eq!(needs_sets(&h), 2, "{:?}", h.fake.requests());
    assert!(h.fake.claim().is_some(), "the release is not applied yet");
    assert_eq!(entry(&h), None);
    assert_eq!(h.fake.needs_told_last(), None, "{:?}", h.fake.requests());
    assert_eq!(needs_unsets(&h), 3, "{:?}", h.fake.requests());
    h.fake.open_gate(RELEASE);
    let released = release
        .await
        .unwrap_or_else(|err| panic!("the release: {err}"));
    assert!(released, "{:?}", h.sup.journal());
    assert!(h.fake.claim().is_none());
}

/// A NEW CLAIM SENDS THE STANDING SET AGAIN: aterm drops the set with the claim (the
/// connection's close), and the claim the reconnect proves tells it again after its own claim;
/// so does the claim taken again after a person's clear, once the policy moves. NEGATIVE
/// CONTROL: an Off ends the needs the thread held, so the claim after it tells none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_claim_sends_the_standing_set_again() {
    let h = settled().await;
    told(&h, "t1", &THREE).await;
    // The connection's close.
    let claims = noted(&h, CLAIMED);
    h.sup
        .client()
        .unwrap_or_else(|| panic!("claim connection"))
        .close();
    wait_until("the reconnect tells the standing set again", || {
        noted(&h, CLAIMED) > claims && needs_sets(&h) == 2
    })
    .await;
    assert_eq!(h.fake.needs_told(), Some(owned(&THREE)));
    let requests = h.fake.requests();
    let last_claim = requests
        .iter()
        .rposition(|r| r.contains("meta set supervisor codex-alab@"));
    let last_set = requests
        .iter()
        .rposition(|r| r.contains(" meta set harness_needs "));
    assert!(last_claim < last_set, "{requests:#?}");
    // A person's clear, then the policy moves.
    h.fake.clear_claim();
    assert_eq!(h.fake.needs_told(), None);
    wait_until("passive after the clear", || !h.sup.held()).await;
    act_again(&h).await;
    wait_until("the new claim tells the standing set again", || {
        needs_sets(&h) == 3
    })
    .await;
    assert_eq!(h.fake.needs_told(), Some(owned(&THREE)));
    // NEGATIVE CONTROL: an Off.
    h.fake.set_effective(false, "autopilot-off");
    wait_until("passive under the off", || {
        !h.sup.held() && entry(&h).is_none()
    })
    .await;
    act_again(&h).await;
    h.sup.show_needs().await;
    assert_eq!(needs_sets(&h), 3, "{:?}", h.fake.requests());
    assert_eq!(h.fake.needs_told_last(), None);
}

/// A SET AN EARLIER IMAGE LEFT COMES DOWN AT THE FIRST CLAIM (the wire's first sync with no need
/// standing): an image that exec'd before aterm took its release leaves its claim, its needs
/// entry and the set standing under this process's proof, and aterm keeps the set for this
/// image's claim; no need stands here, so the claim's first sync takes the entry down and the
/// set with it, though this image never told one. NEGATIVE CONTROL: once settled, a sync sends
/// nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_set_an_earlier_image_left_comes_down_at_the_first_claim() {
    let left = "Codex needs you: the signing key password (+1 more) \u{2014} carrying on";
    let h = started(&[], |fake, _| {
        fake.leave_needs(
            std::process::id(),
            left,
            &["the signing key password", "a reviewer"],
        );
    })
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.needs_told_last().is_none())
            .await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(needs_requests(&h), 1, "{:?}", h.fake.requests());
    // NEGATIVE CONTROL.
    h.sup.show_needs().await;
    assert_eq!(needs_requests(&h), 1, "{:?}", h.fake.requests());
}

/// AN ATERM BEFORE THE VERB IS ASKED ONCE PER SET: it answers `meta set|unset harness_needs` as
/// a field it does not know (`ERR unknown meta field (…)`); the claim's first unset is asked
/// once, the same set is not asked again at any sync or report, another set is asked once,
/// `NEEDS YOU: nothing` asks nothing (nothing was told, so nothing comes down), and the first
/// set reported again is asked once more. NEGATIVE CONTROL: the same claim on an aterm that
/// takes the verb tells the next set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_aterm_before_the_verb_is_asked_once_per_set() {
    let h = started(&[], |fake, _| fake.support_harness_needs(false)).await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    wait_until("the first claim's unset", || needs_unsets(&h) == 1).await;
    let asked = |h: &Harness| {
        noted(
            h,
            "needs not told whole: aterm takes no harness_needs (ERR unknown meta field",
        )
    };
    turn(&h, "t1", true, &report_of(&THREE)).await;
    wait_until("the set is asked", || asked(&h) == 2).await;
    for id in ["t2", "t3"] {
        turn(&h, id, true, &report_of(&THREE)).await;
        h.sup.show_needs().await;
    }
    assert_eq!(needs_sets(&h), 1, "{:?}", h.fake.requests());
    turn(&h, "t4", true, &report_of(&["the signing key password"])).await;
    wait_until("another set is asked", || needs_sets(&h) == 2).await;
    h.sup.show_needs().await;
    turn(&h, "t5", true, "Stage 5 is in.\nNEEDS YOU: nothing").await;
    wait_until("the entry is down", || entry(&h).is_none()).await;
    h.sup.show_needs().await;
    assert_eq!(needs_unsets(&h), 1, "{:?}", h.fake.requests());
    turn(&h, "t6", true, &report_of(&THREE)).await;
    wait_until("the first set is asked once more", || needs_sets(&h) == 3).await;
    h.sup.show_needs().await;
    assert_eq!(needs_requests(&h), 4, "{:?}", h.fake.requests());
    assert_eq!(h.fake.needs_told_last(), None);
    // NEGATIVE CONTROL: the verb taken.
    h.fake.support_harness_needs(true);
    told(&h, "t7", &["a reviewer for #42"]).await;
}

/// A DENIED SET IS NOT ASKED AGAIN UNDER ITS CLAIM: aterm answers `ERR denied`, and neither
/// another report nor its `NEEDS YOU: nothing` asks again while that claim stands. NEGATIVE
/// CONTROL: the next claim (the reconnect's) asks again, and the standing set is told.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_denied_set_is_not_asked_again_under_its_claim() {
    let h = settled().await;
    let need = "the signing key password";
    let shown = format!("Codex needs you: {need} \u{2014} carrying on");
    h.fake.reply_once(
        &format!(
            "@{} meta set harness_needs entry={} need={}",
            h.fake.sid(),
            pct_encode(&shown),
            pct_encode(need)
        ),
        "ERR denied\n",
    );
    turn(&h, "t1", true, &report_of(&[need])).await;
    wait_until("the denial is noted", || {
        noted(&h, "needs not told whole: ERR denied") == 1
    })
    .await;
    turn(&h, "t2", true, &report_of(&THREE)).await;
    wait_until("the second entry is up", || {
        entry(&h).is_some_and(|text| text.contains("(+2 more)"))
    })
    .await;
    h.sup.show_needs().await;
    turn(&h, "t3", true, "Stage 3 is in.\nNEEDS YOU: nothing").await;
    wait_until("the entry is down", || entry(&h).is_none()).await;
    h.sup.show_needs().await;
    turn(&h, "t4", true, &report_of(&THREE)).await;
    wait_until("the entry is up again", || entry(&h).is_some()).await;
    h.sup.show_needs().await;
    assert_eq!(needs_requests(&h), 2, "{:?}", h.fake.requests());
    assert_eq!(h.fake.needs_told_last(), None);
    // NEGATIVE CONTROL: the next claim.
    let claims = noted(&h, CLAIMED);
    h.sup
        .client()
        .unwrap_or_else(|| panic!("claim connection"))
        .close();
    wait_until("the next claim asks again", || {
        noted(&h, CLAIMED) > claims && h.fake.needs_told() == Some(owned(&THREE))
    })
    .await;
    assert_eq!(needs_requests(&h), 3, "{:?}", h.fake.requests());
}

/// NOTHING IS SENT WITHOUT A PROVEN CLAIM: while another holder has the session, a report's
/// needs are wanted and neither their entry nor their set is sent. NEGATIVE CONTROL: once that
/// holder's claim goes and codex-alab's own is proven, the same set is told.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_is_sent_without_a_proven_claim() {
    let h = started(&[], |fake, _| fake.claim_as("another-supervisor")).await;
    wait_until("the claim policy was read", || {
        h.fake
            .requests()
            .iter()
            .any(|request| request.ends_with("harness policy"))
    })
    .await;
    turn(&h, "t1", true, &report_of(&THREE)).await;
    assert_ne!(h.sup.needs_key(), 0, "wanted");
    h.sup.show_needs().await;
    assert!(!h.sup.held());
    assert_eq!(needs_requests(&h), 0, "{:?}", h.fake.requests());
    // NEGATIVE CONTROL: the other holder's claim goes.
    h.fake.clear_claim();
    tokio::time::timeout(Duration::from_secs(60), async {
        while !h.sup.acting() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|error| {
        panic!(
            "claim recovery hang detector: {error}; journal: {:?}; requests: {:?}",
            h.sup.journal(),
            h.fake.requests()
        )
    });
    assert!(
        h.fake
            .wait_for(|fake| fake.needs_told() == Some(owned(&THREE)))
            .await,
        "{:?}",
        h.sup.journal()
    );
}

/// THIRTY-THREE NEEDS SEND THE FIRST THIRTY-TWO, in the worker's order (aterm's wire takes at
/// most [`codex_alab_aterm::NEEDS_TOLD`]); the entry still counts every one. NEGATIVE CONTROL:
/// thirty-two are told whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thirty_three_needs_send_the_first_thirty_two() {
    let h = settled().await;
    let needs: Vec<String> = (1..=33).map(|n| format!("need number {n}")).collect();
    let all: Vec<&str> = needs.iter().map(String::as_str).collect();
    // NEGATIVE CONTROL.
    told(&h, "t1", &all[..32]).await;
    turn(&h, "t2", true, &report_of(&all)).await;
    assert!(
        h.fake
            .wait_for(|fake| {
                fake.needs_told_last()
                    .is_some_and(|told| told.entry.contains("(+32 more)"))
                    && fake.needs_told().as_deref() == Some(&needs[..32])
            })
            .await,
        "{:?} {:?}",
        h.fake.requests(),
        h.sup.journal()
    );
    assert_eq!(
        entry(&h).as_deref(),
        Some("Codex needs you: need number 1 (+32 more) \u{2014} carrying on")
    );
    assert_eq!(needs_sets(&h), 2, "{:?}", h.fake.requests());
}
