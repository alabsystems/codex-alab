// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! RULING F'S ARM IN ATERM'S `status held=` (gap 4 stage 3 of the parity sweep, 2026-10-28:
//! aterm's `status held=` said `-` on a stopped codex-alab tab), against alab-aterm's in-process
//! FAKE aterm (never a real one): the told hold's arm goes up as `meta set harness_held <arm>`
//! on the claim's connection while the proven claim is held, once per change; a limit's hold
//! names none; the hold's lift, a standby, an Off and a stopped runtime take it down; a new
//! claim sends the standing arm again, and takes down one aterm kept for this process (an
//! earlier claim's, an earlier image's, a failed call's); an aterm before the verb is asked once
//! per arm, a denied one not again under its claim; the tab shows the primary thread's arm. Each
//! with its negative control.

use super::*;
use crate::replies::HeldBy;
use crate::replies::Kept;
use crate::replies::NO_WORK_ACTS;
use crate::replies::Replies;
use crate::replies::mark_of;

/// The `harness_held` requests the fake took: its sets of `arm` (`Some`), or its unsets.
fn held_sent(h: &Harness, arm: Option<&str>) -> usize {
    let line = match arm {
        Some(arm) => format!(" meta set harness_held {arm}"),
        None => " meta unset harness_held".to_string(),
    };
    h.fake
        .requests()
        .iter()
        .filter(|r| r.ends_with(&line))
        .count()
}

/// Every `harness_held` request the fake took.
fn held_requests(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.contains(" harness_held"))
        .count()
}

async fn evaluate(h: &Harness) {
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
}

/// [`harness`], once the claim's first sync settled what aterm may keep for this process (an
/// earlier image's arm): nothing holds yet, so it takes it down, an unset this image sends
/// whatever aterm shows.
async fn settled(policy: &[(&str, &str)]) -> Harness {
    let h = harness(policy).await;
    wait_until("the first claim's unset", || held_sent(&h, None) == 1).await;
    h
}

/// The hold lifts with no sync of its own, as in a gap with no claim held: the thread decides
/// nothing held from here on, and its arm is out of the supervisor's.
fn lift_unsent(h: &Harness) {
    h.rt.with_state(|s| s.task = false);
    assert!(h.sup.want_held_for(h.rt.key(), None), "an arm stood");
}

/// Make `thread` the TUI's primary thread.
fn primary(h: &Harness, thread: &str) {
    h.shared.observe(
        &Frame {
            primary_thread: Some(thread),
            displayed_thread: Some(thread),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: h._dir.path(),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
}

fn claim_connection(h: &Harness) -> codex_alab_aterm::Client {
    h.sup.client().unwrap_or_else(|| panic!("claim connection"))
}

/// The thread's point held by `arm` at no wall: the newest reply kept is the point's, settled
/// twice ([`HeldBy::SameReply`]), or once after [`NO_WORK_ACTS`] no-work answers in a row
/// ([`HeldBy::NoWork`]).
async fn hold(h: &Harness, arm: HeldBy, text: &str) {
    // Transporting a standing arm is a Land behavior; Cruise recovers autonomously.
    if h.sup.held() {
        word(h, "land").await;
    }
    point(h, ended(Some(text), None));
    h.rt.with_state(|s| {
        let end = s.point.clone().unwrap_or_else(|| panic!("a point"));
        s.replies_at = Some(end.at);
        s.replies = Replies {
            run: if arm == HeldBy::NoWork {
                NO_WORK_ACTS
            } else {
                0
            },
            ..Replies::of(&[Kept {
                mark: mark_of(&end),
                answers: if arm == HeldBy::SameReply { 2 } else { 1 },
                walled: false,
                tries: 0,
            }])
        };
    });
    assert_eq!(h.rt.state().point_held_by(), Some(arm));
}

/// A held point names its arm until it lifts.
async fn shown_after_hold(h: &Harness, arm: HeldBy) {
    hold(h, arm, "Still on the parser.").await;
    evaluate(h).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.held().as_deref() == Some(arm.word()))
            .await,
        "{:?}",
        h.sup.journal()
    );
}

/// THE ARM GOES UP ONCE AND COMES DOWN AT THE LIFT: a point held as the same reply sets
/// `same-reply` on aterm (`status held=same-reply`), once however often it is decided again; a
/// person's turn in other words lifts the hold at its end and the arm comes down. NEGATIVE
/// CONTROL: the same point decided before its hold sends nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_point_names_its_arm_once_and_the_lift_takes_it_down() {
    let h = settled(&[]).await;
    // NEGATIVE CONTROL: not held yet.
    point(&h, ended(Some("Still on the parser."), None));
    evaluate(&h).await;
    assert_eq!(held_requests(&h), 1, "{:?}", h.fake.requests());
    assert_eq!(h.fake.held(), None);
    shown_after_hold(&h, HeldBy::SameReply).await;
    evaluate(&h).await;
    evaluate(&h).await;
    h.sup.show_held().await;
    assert_eq!(
        held_sent(&h, Some("same-reply")),
        1,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(held_sent(&h, None), 1);
    // The lift: a person's whole turn in other words, through the extension.
    turn(&h, "the-persons-turn", false, "I fixed the build myself.").await;
    assert_eq!(h.rt.state().point_held_by(), None);
    assert!(h.fake.wait_for(|fake| fake.held().is_none()).await);
    assert_eq!(held_sent(&h, None), 2, "{:?}", h.fake.requests());
}

/// SIX NO-WORK ANSWERS NAME `no-work`, A LIMIT NONE: a usage window held once its tries are
/// spent is told in aterm's `limit_repeats` words, which name no arm, so nothing is sent; six
/// no-work answers in a row set `no-work`; a decision not held (no task stands) takes it down.
/// NEGATIVE CONTROL: the limit's hold was told (its appnotice) while no arm went up.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn six_no_work_answers_name_no_work_and_a_limit_names_none() {
    let h = settled(&[("limit_wait", "true")]).await;
    let wall = Wall::Usage {
        reset_at: Some(Instant::now() + Duration::from_secs(3600)),
        reset_label: Some("15:00".to_string()),
    };
    point(&h, ended(None, Some(wall)));
    h.rt.with_state(|s| {
        let end = s.point.clone().unwrap_or_else(|| panic!("a point"));
        s.replies_at = Some(end.at);
        s.replies_reset = match &end.wall {
            Some(Wall::Usage { reset_at, .. }) => *reset_at,
            _ => None,
        };
        s.replies = Replies::of(&[Kept {
            mark: mark_of(&end),
            answers: 0,
            walled: true,
            tries: 3,
        }]);
    });
    assert!(h.rt.state().point_held_by().is_some());
    evaluate(&h).await;
    assert!(
        h.fake
            .notices()
            .iter()
            .any(|n| n.contains(crate::replies::LIMIT_STANDS)),
        "{:?}",
        h.fake.notices()
    );
    assert_eq!(held_requests(&h), 1, "{:?}", h.fake.requests());
    shown_after_hold(&h, HeldBy::NoWork).await;
    assert_eq!(held_sent(&h, Some("no-work")), 1);
    // A decision not held.
    h.rt.with_state(|s| s.task = false);
    evaluate(&h).await;
    assert!(h.fake.wait_for(|fake| fake.held().is_none()).await);
    assert_eq!(held_sent(&h, None), 2, "{:?}", h.fake.requests());
}

/// A LOST CLAIM'S NEXT CLAIM SENDS THE STANDING ARM AGAIN: aterm drops the arm with the claim
/// (the connection's close), and the claim the reconnect proves sets it again after its own
/// claim; so does the claim taken again after a person's clear, once the policy moves.
/// NEGATIVE CONTROL: an Off parks the thread and lifts its hold, so the claim after it sets no
/// arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_and_remade_claim_sends_the_standing_arm_again() {
    let h = harness(&[]).await;
    shown_after_hold(&h, HeldBy::SameReply).await;
    // The connection's close.
    let claims = noted(&h, CLAIMED);
    claim_connection(&h).close();
    wait_until("the reconnect sets the standing arm again", || {
        noted(&h, CLAIMED) > claims && held_sent(&h, Some("same-reply")) == 2
    })
    .await;
    assert_eq!(h.fake.held().as_deref(), Some("same-reply"));
    let requests = h.fake.requests();
    let last_claim = requests
        .iter()
        .rposition(|r| r.contains("meta set supervisor codex-alab@"));
    let last_set = requests
        .iter()
        .rposition(|r| r.ends_with(" meta set harness_held same-reply"));
    assert!(last_claim < last_set, "{requests:#?}");
    // A person's clear, then the policy moves.
    h.fake.clear_claim();
    assert_eq!(h.fake.held(), None);
    wait_until("passive after the clear", || !h.sup.held()).await;
    act_again(&h).await;
    wait_until("the new claim sets the standing arm again", || {
        held_sent(&h, Some("same-reply")) == 3
    })
    .await;
    assert_eq!(h.fake.held().as_deref(), Some("same-reply"));
    // NEGATIVE CONTROL: an Off.
    h.fake.set_effective(false, "autopilot-off");
    wait_until("passive under the off", || !h.sup.held()).await;
    assert!(h.fake.wait_for(|fake| fake.held().is_none()).await);
    act_again(&h).await;
    evaluate(&h).await;
    assert_eq!(
        h.rt.state().point_held_by(),
        None,
        "the Off's park lifted it"
    );
    h.sup.show_held().await;
    assert_eq!(
        held_sent(&h, Some("same-reply")),
        3,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(h.fake.held(), None);
}

/// A STANDBY TAKES THE ARM DOWN, ACTING AGAIN SHOWS IT: a suspension keeps the claim, and
/// aterm's arm with it, but nothing is held back on standby, so it comes down; acting again
/// under the kept claim sets the standing arm with no decision told anew. NEGATIVE CONTROL: an
/// arm wanted on standby is never sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_standby_takes_the_arm_down_and_acting_again_shows_it() {
    let h = settled(&[]).await;
    shown_after_hold(&h, HeldBy::SameReply).await;
    let claimed = noted(&h, CLAIMED);
    h.fake.set_effective(false, "suspended");
    wait_until("on standby", || h.sup.standby().is_some()).await;
    assert!(h.fake.wait_for(|fake| fake.held().is_none()).await);
    assert!(h.sup.held(), "the claim is kept");
    assert_eq!(held_sent(&h, None), 2);
    // NEGATIVE CONTROL: on standby.
    assert!(!h.sup.want_held_for(h.rt.key(), Some(HeldBy::NoWork)));
    h.sup.show_held().await;
    assert_eq!(held_sent(&h, Some("no-work")), 0);
    let told = h.fake.notices().len();
    act_again(&h).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.held().as_deref() == Some("same-reply"))
            .await,
        "{:?}",
        h.sup.journal()
    );
    assert_eq!(held_sent(&h, Some("same-reply")), 2);
    assert_eq!(
        noted(&h, CLAIMED),
        claimed,
        "the same claim: {:?}",
        h.sup.journal()
    );
    assert!(
        !h.fake.notices()[told..]
            .iter()
            .any(|n| n.contains(crate::replies::STOPPED_TYPING)),
        "{:?}",
        h.fake.notices()
    );
}

/// AN ATERM BEFORE THE VERB IS ASKED ONCE PER ARM: it answers `meta set|unset harness_held` as
/// a field it does not know (`ERR unknown meta field (…)`); the claim's first unset is asked
/// once, the same arm is not asked again at any decision or sync, another arm is asked once, a
/// lift asks nothing (nothing shows), and the arm after it is asked once more. NEGATIVE
/// CONTROL: the same claim on an aterm that takes the verb shows the next arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_aterm_before_the_verb_is_asked_once_per_arm() {
    let h = started(&[], |fake, _| fake.support_harness_held(false)).await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    wait_until("the first claim's unset", || held_sent(&h, None) == 1).await;
    hold(&h, HeldBy::SameReply, "Still on the parser.").await;
    evaluate(&h).await;
    assert_eq!(
        held_sent(&h, Some("same-reply")),
        1,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(
        noted(
            &h,
            "held not shown: aterm takes no harness_held (ERR unknown meta field"
        ),
        2,
        "{:?}",
        h.sup.journal()
    );
    for _ in 0..3 {
        evaluate(&h).await;
        h.sup.show_held().await;
    }
    assert_eq!(
        held_sent(&h, Some("same-reply")),
        1,
        "{:?}",
        h.fake.requests()
    );
    hold(&h, HeldBy::NoWork, "Still on the lexer.").await;
    evaluate(&h).await;
    evaluate(&h).await;
    h.sup.show_held().await;
    assert_eq!(held_sent(&h, Some("no-work")), 1, "{:?}", h.fake.requests());
    // A lift: nothing shows, so nothing comes down.
    h.rt.with_state(|s| s.task = false);
    evaluate(&h).await;
    assert_eq!(held_sent(&h, None), 1, "{:?}", h.fake.requests());
    h.rt.with_state(|s| s.task = true);
    evaluate(&h).await;
    assert_eq!(held_sent(&h, Some("no-work")), 2, "{:?}", h.fake.requests());
    assert_eq!(held_requests(&h), 4);
    assert_eq!(h.fake.held(), None);
    // NEGATIVE CONTROL: the verb taken.
    h.fake.support_harness_held(true);
    hold(&h, HeldBy::SameReply, "Still on the parser.").await;
    evaluate(&h).await;
    assert_eq!(h.fake.held().as_deref(), Some("same-reply"));
}

/// A DENIED ARM IS NOT ASKED AGAIN UNDER ITS CLAIM: aterm answers `ERR denied`, and neither a
/// decision again nor another arm asks again while that claim stands. NEGATIVE CONTROL: the
/// next claim (the reconnect's) asks again, and the standing arm is shown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_denied_arm_is_not_asked_again_under_its_claim() {
    let h = settled(&[]).await;
    h.fake.reply_once(
        &format!("@{} meta set harness_held same-reply", h.fake.sid()),
        "ERR denied\n",
    );
    hold(&h, HeldBy::SameReply, "Still on the parser.").await;
    evaluate(&h).await;
    assert_eq!(
        noted(&h, "held not shown: ERR denied"),
        1,
        "{:?}",
        h.sup.journal()
    );
    evaluate(&h).await;
    h.sup.show_held().await;
    hold(&h, HeldBy::NoWork, "Still on the lexer.").await;
    evaluate(&h).await;
    h.sup.show_held().await;
    assert_eq!(held_requests(&h), 2, "{:?}", h.fake.requests());
    assert_eq!(h.fake.held(), None);
    // NEGATIVE CONTROL: the next claim.
    let claims = noted(&h, CLAIMED);
    claim_connection(&h).close();
    wait_until("the next claim asks again", || {
        noted(&h, CLAIMED) > claims && h.fake.held().as_deref() == Some("no-work")
    })
    .await;
    assert_eq!(held_requests(&h), 3, "{:?}", h.fake.requests());
}

/// NOTHING IS SENT WITHOUT A PROVEN CLAIM: while another holder has the session, a held point
/// decides nothing and an arm wanted is never sent. NEGATIVE CONTROL: once that holder's claim
/// goes and codex-alab's own is proven, the same hold is shown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_is_sent_without_a_proven_claim() {
    let h = started(&[], |fake, _| {
        fake.set_status("autopilot", "land");
        fake.set_policy_key("autopilot", "land");
        fake.claim_as("another-supervisor");
    })
    .await;
    wait_until("the claim policy was read", || {
        h.fake
            .requests()
            .iter()
            .any(|request| request.ends_with("harness policy"))
    })
    .await;
    hold(&h, HeldBy::SameReply, "Still on the parser.").await;
    evaluate(&h).await;
    assert!(!h.sup.want_held_for(h.rt.key(), Some(HeldBy::SameReply)));
    h.sup.show_held().await;
    assert!(!h.sup.held());
    assert_eq!(held_requests(&h), 0, "{:?}", h.fake.requests());
    // NEGATIVE CONTROL: the other holder's claim goes.
    h.fake.clear_claim();
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    word(&h, "land").await;
    evaluate(&h).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.held().as_deref() == Some("same-reply"))
            .await,
        "{:?}",
        h.sup.journal()
    );
}

/// THE TAB SHOWS THE PRIMARY THREAD'S ARM ([`crate::supervisor`]'s `held_want`): another
/// root's hold stands for that thread only, and shows once the TUI makes it the primary
/// thread; the arm follows the primary back. NEGATIVE CONTROL: before the switch the tab's arm
/// is the primary thread's (none).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tab_shows_the_primary_threads_arm() {
    let h = settled(&[]).await;
    let other = ThreadId::new().to_string();
    h.shared.note_root_thread(&other);
    assert!(h.sup.want_held_for(&other, Some(HeldBy::NoWork)));
    h.sup.show_held().await;
    assert_eq!(held_requests(&h), 1, "{:?}", h.fake.requests());
    primary(&h, &other);
    assert!(
        h.fake
            .wait_for(|fake| fake.held().as_deref() == Some("no-work"))
            .await,
        "{:?}",
        h.sup.journal()
    );
    primary(&h, h.rt.key());
    assert!(h.fake.wait_for(|fake| fake.held().is_none()).await);
    assert_eq!(held_sent(&h, Some("no-work")), 1);
    assert_eq!(held_sent(&h, None), 2);
}

/// AN ARM AN EARLIER IMAGE LEFT COMES DOWN AT THE FIRST CLAIM (review of 8482c60ba6, finding
/// 1): an image that exec'd before aterm took its release leaves its claim and arm standing
/// under this process's proof (an exec keeps the pid and kernel start), and aterm keeps the arm
/// for this image's claim; nothing holds here, so the claim's first sync takes it down, though
/// this image never showed one. NEGATIVE CONTROL: once settled, neither a sync nor a decision
/// not held sends anything.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_arm_an_earlier_image_left_comes_down_at_the_first_claim() {
    let h = started(&[], |fake, _| {
        fake.leave_held(std::process::id(), "same-reply");
    })
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    assert!(
        h.fake.wait_for(|fake| fake.held().is_none()).await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(held_requests(&h), 1, "{:?}", h.fake.requests());
    // NEGATIVE CONTROL.
    h.sup.show_held().await;
    point(&h, ended(Some("Still on the parser."), None));
    evaluate(&h).await;
    assert_eq!(held_requests(&h), 1, "{:?}", h.fake.requests());
}

/// AN ARM ATERM KEPT FOR THE RECONNECT'S CLAIM COMES DOWN AT ITS FIRST SYNC (review of
/// 8482c60ba6, finding 3): the claim's connection closed, and the hold lifted while no claim
/// was held, so nothing could take the arm down; aterm took the reconnect's claim before it
/// read the old connection's close, and kept the arm (it keys it by the claim's proof), so the
/// first sync under the new claim takes it down. NEGATIVE CONTROL: the arm stands at aterm
/// through the gap.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_arm_aterm_kept_for_the_reconnects_claim_comes_down_at_its_first_sync() {
    let h = settled(&[]).await;
    shown_after_hold(&h, HeldBy::SameReply).await;
    h.fake.defer_closes(true);
    h.fake.gate("meta set supervisor");
    let claims = noted(&h, CLAIMED);
    claim_connection(&h).close();
    lift_unsent(&h);
    h.sup.show_held().await;
    // NEGATIVE CONTROL: nothing was sent in the gap.
    assert_eq!(held_sent(&h, None), 1, "{:?}", h.fake.requests());
    assert_eq!(h.fake.held().as_deref(), Some("same-reply"));
    h.fake.open_gate("meta set supervisor");
    wait_until("the reconnect's claim", || noted(&h, CLAIMED) > claims).await;
    assert!(
        h.fake.wait_for(|fake| fake.held().is_none()).await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(held_sent(&h, None), 2, "{:?}", h.fake.requests());
}

/// A SET WHOSE CALL FAILED IS UNKNOWN, NOT UNSENT (review of 8482c60ba6, finding 1): aterm
/// applied the line and its answer was lost with the connection, whose close aterm read after
/// it took the reconnect's claim, so it kept the arm; the hold lifted before that claim. Under
/// the claim before, aterm's last answer was `-`, which the failed call unsettled, so the
/// first sync under the new claim takes the arm down. NEGATIVE CONTROL: the arm the failed call
/// left stands at aterm until that sync.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_set_whose_call_failed_comes_down_at_the_next_claim() {
    let h = settled(&[]).await;
    // Under this claim aterm's last answer is `-`: the hold, then its lift.
    shown_after_hold(&h, HeldBy::SameReply).await;
    turn(&h, "the-persons-turn", false, "I fixed the build myself.").await;
    assert!(h.fake.wait_for(|fake| fake.held().is_none()).await);
    h.fake.defer_closes(true);
    h.fake.gate("meta set supervisor");
    h.fake.drop_next_reply("meta set harness_held");
    let claims = noted(&h, CLAIMED);
    hold(&h, HeldBy::SameReply, "Still on the lexer.").await;
    evaluate(&h).await;
    assert_eq!(
        noted(&h, "held not shown: "),
        1,
        "the call failed: {:?}",
        h.sup.journal()
    );
    lift_unsent(&h);
    // NEGATIVE CONTROL: applied, its answer lost.
    assert_eq!(h.fake.held().as_deref(), Some("same-reply"));
    h.fake.open_gate("meta set supervisor");
    wait_until("the reconnect's claim", || noted(&h, CLAIMED) > claims).await;
    assert!(
        h.fake.wait_for(|fake| fake.held().is_none()).await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(held_sent(&h, None), 3, "{:?}", h.fake.requests());
}

/// A STOPPED RUNTIME TAKES ITS ARM DOWN (review of 8482c60ba6, finding 4: no test reached the
/// stop): nothing holds a stopped thread, so the primary thread's arm comes down at its
/// runtime's stop, with no decision of its own. NEGATIVE CONTROL: the arm stands through a
/// sync before the stop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_runtime_takes_its_arm_down() {
    use codex_extension_api::ThreadLifecycleContributor;
    use codex_extension_api::ThreadStopInput;
    let h = settled(&[]).await;
    shown_after_hold(&h, HeldBy::SameReply).await;
    // NEGATIVE CONTROL.
    h.sup.show_held().await;
    assert_eq!(h.fake.held().as_deref(), Some("same-reply"));
    h.autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &ExtensionData::new("session"),
            thread_store: &h._store,
        })
        .await;
    assert!(
        h.fake.wait_for(|fake| fake.held().is_none()).await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(held_sent(&h, None), 2, "{:?}", h.fake.requests());
}

/// AN OFF TAKES THE ARM OF A THREAD THAT PARKS LATER (review of 8482c60ba6, finding 4: the
/// park of the one thread covered the Off's own take-down): the Off's park lifts every
/// thread's hold whenever its memory parks, so a primary root that has not parked shows no arm
/// under the claim after the Off. NEGATIVE CONTROL: before the Off, that root's arm is the
/// tab's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_takes_the_arm_of_a_thread_that_parks_later() {
    let h = settled(&[]).await;
    let other = ThreadId::new().to_string();
    h.shared.note_root_thread(&other);
    assert!(h.sup.want_held_for(&other, Some(HeldBy::NoWork)));
    primary(&h, &other);
    // NEGATIVE CONTROL.
    assert!(
        h.fake
            .wait_for(|fake| fake.held().as_deref() == Some("no-work"))
            .await,
        "{:?}",
        h.sup.journal()
    );
    h.fake.set_effective(false, "autopilot-off");
    wait_until("passive under the off", || !h.sup.held()).await;
    assert!(h.fake.wait_for(|fake| fake.held().is_none()).await);
    let claims = noted(&h, CLAIMED);
    h.fake.set_effective(true, "enabled");
    wait_until("the claim after the off", || {
        noted(&h, CLAIMED) > claims && h.sup.acting()
    })
    .await;
    h.sup.show_held().await;
    assert_eq!(held_sent(&h, Some("no-work")), 1, "{:?}", h.fake.requests());
    assert_eq!(h.fake.held(), None);
}

/// A HELD LIMIT OR A MOVED ARM AT A TURN'S END TAKES THE OLD ARM OUT: a usage window held once
/// its tries are spent has `point_held_by() == Some`, yet its hold (`Told::Limit`) names no arm
/// (`point_held_arm() == None`), so a turn that ends at a held usage limit while on standby
/// takes the thread's old arm out of [`crate::supervisor::Supervisor`]'s `holds`, and acting
/// again does not show the old arm before the limit point is decided. NEGATIVE CONTROL: a turn
/// that ends while on standby and stays held by the same arm at no wall keeps that arm for the
/// standby's lift.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_limit_at_finish_takes_the_old_arm_out_before_standby_lifts() {
    use codex_extension_api::TurnErrorInput;
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStartInput;
    use codex_extension_api::TurnStopInput;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    use codex_protocol::error::CodexErrorDetails;
    use codex_protocol::protocol::CodexErrorInfo;

    let h = settled(&[("limit_wait", "true")]).await;
    shown_after_hold(&h, HeldBy::SameReply).await;
    h.fake.set_effective(false, "suspended");
    wait_until("on standby", || h.sup.standby().is_some()).await;
    assert!(h.fake.wait_for(|fake| fake.held().is_none()).await);
    // NEGATIVE CONTROL: a turn ending while on standby that is still held by `same-reply` at no
    // wall keeps `same-reply` in `holds`.
    h.rt.with_state(|s| s.awaited = Some(false));
    turn(&h, "same-reply-on-standby", true, "Still on the parser.").await;
    assert_eq!(h.rt.state().point_held_arm(), Some(HeldBy::SameReply));
    assert!(
        !h.sup.keep_held_for(h.rt.key(), Some(HeldBy::SameReply)),
        "still kept in holds"
    );
    // Now a turn ends at a usage limit whose tries are already spent (`point_held_by()` is
    // `Some`, while `point_held_arm()` is `None`).
    let sample = TurnEnd {
        wall: Some(Wall::Usage {
            reset_at: None,
            reset_label: None,
        }),
        ..ended(None, None)
    };
    h.rt.with_state(|s| {
        s.awaited = Some(true);
        s.replies_at = Some(sample.at);
        s.replies = Replies::of(&[Kept {
            mark: mark_of(&sample),
            answers: 0,
            walled: true,
            tries: 2,
        }]);
    });
    let session = ExtensionData::new("session");
    let turn_store = ExtensionData::new("usage-limit-turn");
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    h.autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn_store.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store: &h._store,
            turn_store: &turn_store,
        })
        .await;
    h.autopilot
        .on_turn_error(TurnErrorInput {
            turn_id: turn_store.level_id(),
            error: CodexErrorInfo::UsageLimitExceeded,
            error_details: &CodexErrorDetails::Fatal("usage limit".into()),
            session_store: &session,
            thread_store: &h._store,
            turn_store: &turn_store,
        })
        .await;
    h.autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &h._store,
            turn_store: &turn_store,
        })
        .await;
    assert_eq!(h.rt.state().point_held_by(), Some(HeldBy::SameReply));
    assert_eq!(h.rt.state().point_held_arm(), None);
    let sets_before = held_sent(&h, Some("same-reply"));
    act_again(&h).await;
    h.sup.show_held().await;
    assert_eq!(
        held_sent(&h, Some("same-reply")),
        sets_before,
        "lifting standby at a held limit point sends no arm: {:?}",
        h.fake.requests()
    );
    assert_eq!(h.fake.held(), None);
}
