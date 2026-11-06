// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! LAND'S HOLD IN ATERM'S LANDING (gap 11 of the parity sweep, 2026-10-28: a codex-alab worker
//! under Land showed no landing in aterm), against alab-aterm's in-process FAKE aterm (never a
//! real one): the primary thread's hold goes up as `meta set harness_land since=… nudges=…
//! [read=1 legs=… …]` on the claim's connection while the proven claim is held under Land, once
//! per change; the land act sets `since` with no nudge, each nudge counts, a land act's yield
//! as judged names its legs and the repository's counts until another's turn begins; a
//! landing, a person's turn (at its message), leaving Land, an Off, a standby and a stopped
//! runtime take it down; a new claim sends the standing
//! hold again, and takes down one aterm kept for this process; an aterm before the verb is
//! asked once per change, a denied hold not again under its claim; another root's Land shows
//! only as the primary thread. Each with its negative control.

use codex_alab_aterm::LandLeg;
use codex_alab_aterm::LandRead;
use codex_alab_aterm::LandReport;

use super::*;
use crate::decide::Act;
use crate::decide::Mode as Word;
use crate::repo::RepoRead;
use crate::repo::RepoWork;
use crate::words;

/// The `harness_land` sets the fake took whose fields are `fields`.
fn land_set(h: &Harness, fields: &str) -> usize {
    let line = format!(" meta set harness_land {fields}");
    h.fake
        .requests()
        .iter()
        .filter(|r| r.ends_with(&line))
        .count()
}

/// The `harness_land` unsets the fake took.
fn land_unset(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.ends_with(" meta unset harness_land"))
        .count()
}

/// Every `harness_land` request the fake took.
fn land_requests(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.contains(" harness_land"))
        .count()
}

pub(super) async fn evaluate(h: &Harness) {
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
}

/// [`harness`] under Land, once the claim's first sync settled what aterm may keep for this
/// process: nothing holds yet, so it takes it down.
pub(super) async fn settled() -> Harness {
    let h = harness(&[]).await;
    word(&h, "land").await;
    wait_until("the first claim's unset", || land_unset(&h) == 1).await;
    h
}

/// The act `rule` typed under Land at `at_ms` (unix ms), as `submit` books it (the act and its
/// stamp under one lock), then told as the act's dispatch tells it.
pub(super) fn land_act(h: &Harness, rule: &'static str, at_ms: u64) {
    let timing = Timing::default();
    h.rt.with_state(|s| {
        s.task = true;
        s.observe_mode(Word::Land);
        s.acted(
            &Act::Type {
                rule,
                text: words::LAND.to_string(),
            },
            Instant::now(),
            &timing,
        );
        s.stamp_land_hold(at_ms);
    });
    h.autopilot.tell_land(&h.rt);
}

/// [`land_act`] of Land's instruction at 1 000 ms, shown.
pub(super) async fn shown_after_land(h: &Harness) {
    land_act(h, words::RULE_LAND, 1_000);
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 0, None)))
            .await,
        "{:?}",
        h.sup.journal()
    );
}

fn report(since_ms: u64, nudges: u32, read: Option<LandRead>) -> LandReport {
    LandReport {
        since_ms,
        nudges,
        read,
    }
}

/// What the probe finds where a test says the repository holds work: one untracked path, one
/// commit no remote has, one the remote main does not.
fn work() -> RepoWork {
    RepoWork::of_git("/r", "?? new.rs\0", "", 1, 1, "origin/main").unwrap_or_else(|| panic!("work"))
}

/// Make `thread` the TUI's primary thread.
pub(super) fn primary(h: &Harness, thread: &str) {
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

/// The turn `turn` begins on the harness's thread, as core starts it (no message yet).
pub(super) async fn begin(h: &Harness, turn: &ExtensionData) {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStartInput;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
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
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &ExtensionData::new("session"),
            thread_store: &h._store,
            turn_store: turn,
        })
        .await;
}

/// The message that opened `turn` completes: a person's (no `client_id`).
pub(super) async fn persons_message(h: &Harness, turn: &ExtensionData) {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_protocol::items::TurnItem;
    let opened = TurnItem::UserMessage(codex_protocol::items::UserMessageItem {
        id: "user-1".to_string(),
        client_id: None,
        content: vec![codex_protocol::user_input::UserInput::Text {
            text: "Also fix the docs typo.".to_string(),
            text_elements: Vec::new(),
        }],
    });
    h.autopilot
        .on_item_completed(&h._store, turn, &opened)
        .await;
}

/// `turn` ends with no reply, as core stops it.
pub(super) async fn stop(h: &Harness, turn: &ExtensionData) {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStopInput;
    h.autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &ExtensionData::new("session"),
            thread_store: &h._store,
            turn_store: turn,
        })
        .await;
}

/// THE LAND ACT SETS `since` WITH NO NUDGE, EACH NUDGE COUNTS: Land's instruction stamped at
/// 1 000 ms goes up as `since=1000 nudges=0`, once however often it is told or synced; a nudge
/// moves it to `nudges=1`, its `since` kept. NEGATIVE CONTROL: Land's point before its act
/// shows nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_land_act_sets_since_and_each_nudge_counts() {
    let h = settled().await;
    // NEGATIVE CONTROL: the point, no act yet.
    point(&h, ended(Some("Stage 1 is in."), None));
    evaluate(&h).await;
    h.sup.show_land().await;
    assert_eq!(land_requests(&h), 1, "{:?}", h.fake.requests());
    shown_after_land(&h).await;
    h.autopilot.tell_land(&h.rt);
    evaluate(&h).await;
    h.sup.show_land().await;
    assert_eq!(land_set(&h, "since=1000 nudges=0"), 1);
    land_act(&h, words::RULE_LAND_NUDGE, 2_000);
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 1, None)))
            .await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(land_set(&h, "since=1000 nudges=1"), 1);
    // The fake applies the line before the supervisor reads its answer.
    wait_until("the landing shown is noted", || {
        noted(&h, "landing shown: nudges=1 legs=-") == 1
    })
    .await;
}

/// A JUDGED YIELD NAMES ITS LEGS AND COUNTS: the land act's yield, ended through the extension,
/// names the need and the open words it says (`read=1 legs=needs,open open=not%20landed`); the
/// probe for that point adds the repository's work and its counts, or `repo-unread` where it
/// could not read. NEGATIVE CONTROL: before the yield ends nothing is read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judged_yield_names_its_legs_and_counts() {
    let h = settled().await;
    shown_after_land(&h).await;
    // NEGATIVE CONTROL: nothing read before the yield.
    assert_eq!(h.fake.land().and_then(|land| land.read), None);
    turn(
        &h,
        "the-yield",
        true,
        "Nothing left: merged, but the lexer is not landed.\nNEEDS YOU: the signing key password",
    )
    .await;
    let read = LandRead {
        legs: vec![LandLeg::Needs, LandLeg::Open],
        open: vec!["not landed".to_string()],
        ..LandRead::default()
    };
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 0, Some(read.clone()))))
            .await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(
        land_set(
            &h,
            "since=1000 nudges=0 read=1 legs=needs,open open=not%20landed"
        ),
        1,
        "{:?}",
        h.fake.requests()
    );
    // The probe for the yield's point, as the evaluation runs it.
    h.rt.with_state(|s| s.probed(Some(work())));
    h.autopilot.tell_land(&h.rt);
    assert!(
        h.fake
            .wait_for(|fake| fake
                .land()
                .and_then(|land| land.read)
                .is_some_and(|read| read.legs.contains(&LandLeg::Repo)))
            .await
    );
    assert_eq!(
        land_set(
            &h,
            "since=1000 nudges=0 read=1 legs=needs,repo,open open=not%20landed untracked=1 \
             unpushed=1 unlanded=1"
        ),
        1,
        "{:?}",
        h.fake.requests()
    );
    h.rt.with_state(|s| s.probed(RepoRead::Unread));
    h.autopilot.tell_land(&h.rt);
    assert!(
        h.fake
            .wait_for(|fake| fake
                .land()
                .and_then(|land| land.read)
                .is_some_and(|read| read.legs.contains(&LandLeg::RepoUnread)))
            .await
    );
    assert_eq!(
        land_set(
            &h,
            "since=1000 nudges=0 read=1 legs=needs,repo-unread,open open=not%20landed"
        ),
        1,
        "{:?}",
        h.fake.requests()
    );
}

/// A LANDING TAKES IT DOWN: the land act's yield defends cleanly, the evaluation's probe reads
/// a clean repository (none to read here) and lands, and the hold comes down with it.
/// NEGATIVE CONTROL: the hold stands through the clean defense's end, which names no leg.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_landing_takes_it_down() {
    let h = settled().await;
    shown_after_land(&h).await;
    turn(
        &h,
        "the-yield",
        true,
        "Nothing left: the parser and the lexer are merged and pushed to origin/main, and the \
         worktrees are removed.",
    )
    .await;
    // NEGATIVE CONTROL.
    h.sup.show_land().await;
    assert_eq!(h.fake.land(), Some(report(1_000, 0, None)));
    evaluate(&h).await;
    assert!(h.rt.state().landed, "{:?}", h.sup.journal());
    assert!(
        h.fake.wait_for(|fake| fake.land().is_none()).await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(land_unset(&h), 2, "{:?}", h.fake.requests());
    // The first claim's unset, then the landing's.
    wait_until("the landing's unset is noted", || {
        noted(&h, "landing cleared") == 2
    })
    .await;
}

/// A PERSON'S TURN TAKES IT DOWN at its message, which ends the Land episode (aterm 791977cae8
/// keeps its hold there; codex-alab's person's turn ends the episode, gap 32), before the turn
/// runs on: its start, a turn no act awaits, took the yield's reading down
/// (`since=1000 nudges=0`), and its message the hold. NEGATIVE CONTROL: codex-alab's own turn
/// keeps it (review of gap 11, finding 4: only the whole turn was asserted, which the turn
/// end's own tell also takes down).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_turn_takes_it_down() {
    let h = settled().await;
    shown_after_land(&h).await;
    // NEGATIVE CONTROL: codex-alab's own turn.
    turn(&h, "the-yield", true, "Merging the lexer now.").await;
    assert!(
        h.fake
            .wait_for(|fake| fake.land().is_some_and(|land| land.read.is_some()))
            .await
    );
    let persons = ExtensionData::new("the-persons-turn");
    begin(&h, &persons).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 0, None)))
            .await,
        "{:?}",
        h.fake.requests()
    );
    persons_message(&h, &persons).await;
    assert!(
        h.fake.wait_for(|fake| fake.land().is_none()).await,
        "the message, not the turn's end: {:?}",
        h.fake.requests()
    );
    stop(&h, &persons).await;
    h.sup.show_land().await;
    assert_eq!(land_unset(&h), 2, "{:?}", h.fake.requests());
}

/// ANOTHER'S TURN TAKES THE YIELD'S READING DOWN AS IT BEGINS (aterm's `took`; review of gap
/// 11, finding 2): after the land act's yield names its legs, a goal's own continuation (a
/// turn no act of codex-alab's awaits, opened by no message) begins, and the hold stands with
/// no legs read while it runs, through its end. NEGATIVE CONTROL: the turn of a nudge
/// codex-alab typed keeps the reading through its start.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anothers_turn_takes_the_yields_reading_down_as_it_begins() {
    let h = settled().await;
    shown_after_land(&h).await;
    turn(&h, "the-yield", true, "Merging the lexer now.").await;
    let defense = LandRead {
        legs: vec![LandLeg::Defense],
        ..LandRead::default()
    };
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 0, Some(defense.clone()))))
            .await,
        "{:?}",
        h.fake.requests()
    );
    // NEGATIVE CONTROL: the nudge's turn.
    land_act(&h, words::RULE_LAND_NUDGE, 2_000);
    let nudged = report(1_000, 1, Some(defense.clone()));
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(nudged.clone()))
            .await,
        "{:?}",
        h.fake.requests()
    );
    turn(&h, "the-nudge", true, "Merging the lexer now.").await;
    h.sup.show_land().await;
    assert_eq!(h.fake.land(), Some(nudged.clone()));
    assert_eq!(
        land_set(&h, "since=1000 nudges=1"),
        0,
        "the nudge's turn never dropped the reading: {:?}",
        h.fake.requests()
    );
    // A goal's own continuation.
    let goals = ExtensionData::new("the-goals-turn");
    begin(&h, &goals).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 1, None)))
            .await,
        "{:?}",
        h.fake.requests()
    );
    stop(&h, &goals).await;
    h.sup.show_land().await;
    assert_eq!(h.fake.land(), Some(report(1_000, 1, None)));
    assert_eq!(land_set(&h, "since=1000 nudges=1"), 1, "once");
}

/// LEAVING LAND TAKES IT DOWN: the session's word read as Cruise, the hold comes down at the
/// policy's read with no decision of its own, and the thread's episode ends at its next look.
/// NEGATIVE CONTROL: a moved setting under the same word keeps it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leaving_land_takes_it_down() {
    let h = settled().await;
    shown_after_land(&h).await;
    // NEGATIVE CONTROL: the word kept, a setting moved.
    let moved = noted(&h, "settings changed: the threads look again");
    h.fake.set_policy_key("answer_text", "Pick%20another.");
    wait_until("the policy is read again", || {
        noted(&h, "settings changed: the threads look again") > moved
    })
    .await;
    h.sup.show_land().await;
    assert_eq!(h.fake.land(), Some(report(1_000, 0, None)));
    // The thread looks at nothing while the word moves: the policy's read alone takes it down.
    let permit = h.rt.hold_eval_when_free().await;
    word(&h, "cruise").await;
    assert!(
        h.fake.wait_for(|fake| fake.land().is_none()).await,
        "{:?}",
        h.fake.requests()
    );
    assert!(h.rt.state().land_held().is_some(), "no look yet");
    drop(permit);
    evaluate(&h).await;
    assert_eq!(h.rt.state().land_held(), None);
    // Land again: a new episode, nothing until its act.
    word(&h, "land").await;
    evaluate(&h).await;
    h.sup.show_land().await;
    assert_eq!(h.fake.land(), None);
    assert_eq!(land_set(&h, "since=1000 nudges=0"), 1);
}

/// AN OFF TAKES IT DOWN, AND THE CLAIM AFTER IT SHOWS NONE: the Off releases the claim (aterm
/// drops the hold with it) and parks the thread, which ends its Land episode. NEGATIVE CONTROL:
/// the same hold stands up to the Off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_takes_it_down() {
    let h = settled().await;
    shown_after_land(&h).await;
    // NEGATIVE CONTROL.
    h.sup.show_land().await;
    assert_eq!(h.fake.land(), Some(report(1_000, 0, None)));
    h.fake.set_effective(false, "autopilot-off");
    wait_until("passive under the off", || !h.sup.held()).await;
    assert!(h.fake.wait_for(|fake| fake.land().is_none()).await);
    act_again(&h).await;
    evaluate(&h).await;
    assert_eq!(h.rt.state().land_held(), None, "the Off's park ended it");
    h.sup.show_land().await;
    assert_eq!(
        land_set(&h, "since=1000 nudges=0"),
        1,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(h.fake.land(), None);
}

/// A STANDBY TAKES IT DOWN, ACTING AGAIN SHOWS IT: a suspension keeps the claim, but nothing
/// acts, so the hold comes down; acting again under the kept claim sets the standing hold with
/// no decision told anew. NEGATIVE CONTROL: a hold wanted on standby is never sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_standby_takes_it_down_and_acting_again_shows_it() {
    let h = settled().await;
    shown_after_land(&h).await;
    let claimed = noted(&h, CLAIMED);
    h.fake.set_effective(false, "suspended");
    wait_until("on standby", || h.sup.standby().is_some()).await;
    assert!(h.fake.wait_for(|fake| fake.land().is_none()).await);
    assert!(h.sup.held(), "the claim is kept");
    assert_eq!(land_unset(&h), 2);
    // NEGATIVE CONTROL: on standby.
    assert!(
        !h.sup
            .want_land_for(h.rt.key(), Some(report(1_000, 5, None)))
    );
    h.sup.show_land().await;
    assert_eq!(land_set(&h, "since=1000 nudges=5"), 0);
    act_again(&h).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 0, None)))
            .await,
        "{:?}",
        h.sup.journal()
    );
    assert_eq!(land_set(&h, "since=1000 nudges=0"), 2);
    assert_eq!(noted(&h, CLAIMED), claimed, "the same claim");
}

/// A TURN STARTING OR ENDING ON STANDBY UPDATES THE STANDING HOLD'S READING BEFORE STANDBY LIFTS:
/// while passive under the kept claim, another's turn that begins takes the old yield's `read`
/// off the standing hold in [`crate::supervisor::Supervisor`]'s `lands`, and a land act's yield
/// that ends on standby records its new `read` on the standing hold, so lifting standby sends the
/// current reading at once without re-sending the stale one or waiting on an evaluation.
/// NEGATIVE CONTROL: an awaited turn's start on standby keeps the standing reading.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_on_standby_updates_the_standing_holds_reading_before_standby_lifts() {
    let h = settled().await;
    shown_after_land(&h).await;
    turn(&h, "the-yield", true, "Merging the lexer now.").await;
    let defense = LandRead {
        legs: vec![LandLeg::Defense],
        ..LandRead::default()
    };
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 0, Some(defense.clone()))))
            .await,
        "{:?}",
        h.fake.requests()
    );
    h.fake.set_effective(false, "suspended");
    wait_until("on standby", || h.sup.standby().is_some()).await;
    assert!(h.fake.wait_for(|fake| fake.land().is_none()).await);
    // Hold the evaluation permit so only `resume_held`'s own `sync_land` can speak when standby
    // lifts.
    let permit = h.rt.hold_eval_when_free().await;
    // NEGATIVE CONTROL: an awaited turn's start on standby keeps `defense` in `lands`.
    h.rt.with_state(|s| s.awaited = Some(false));
    let awaited_turn = ExtensionData::new("awaited-on-standby");
    begin(&h, &awaited_turn).await;
    assert_eq!(
        h.rt.state().land_held(),
        Some(report(1_000, 0, Some(defense.clone())))
    );
    // Another's turn begins on standby: its start takes `defense` off the standing hold.
    h.rt.with_state(|s| s.awaited = None);
    let goals = ExtensionData::new("goals-on-standby");
    begin(&h, &goals).await;
    assert_eq!(h.rt.state().land_held(), Some(report(1_000, 0, None)));
    assert_eq!(h.fake.land(), None, "still on standby");
    act_again(&h).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 0, None)))
            .await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(
        land_set(&h, "since=1000 nudges=0 read=1 legs=defense"),
        1,
        "lifting standby never re-sent the cleared defense leg: {:?}",
        h.fake.requests()
    );
    stop(&h, &goals).await;
    // A nudge is typed while acting; standby comes while its turn is in flight, and its yield ends
    // on standby with `open`.
    land_act(&h, words::RULE_LAND_NUDGE, 2_000);
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 1, None)))
            .await,
        "{:?}",
        h.fake.requests()
    );
    h.fake.set_effective(false, "suspended");
    wait_until("on standby again", || h.sup.standby().is_some()).await;
    assert!(h.fake.wait_for(|fake| fake.land().is_none()).await);
    turn(
        &h,
        "yield-on-standby",
        true,
        "Nothing left: merged, but the lexer is not landed.",
    )
    .await;
    let open = LandRead {
        legs: vec![LandLeg::Open],
        open: vec!["not landed".to_string()],
        ..LandRead::default()
    };
    assert_eq!(h.fake.land(), None, "still on standby");
    act_again(&h).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 1, Some(open.clone()))))
            .await,
        "{:?}",
        h.fake.requests()
    );
    drop(permit);
}

/// A STOPPED RUNTIME TAKES IT DOWN: nothing holds a stopped thread, so the primary thread's
/// hold comes down at its runtime's stop, with no decision of its own. NEGATIVE CONTROL: the
/// hold stands through a sync before the stop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_runtime_takes_it_down() {
    use codex_extension_api::ThreadLifecycleContributor;
    use codex_extension_api::ThreadStopInput;
    let h = settled().await;
    shown_after_land(&h).await;
    // NEGATIVE CONTROL.
    h.sup.show_land().await;
    assert_eq!(h.fake.land(), Some(report(1_000, 0, None)));
    h.autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &ExtensionData::new("session"),
            thread_store: &h._store,
        })
        .await;
    assert!(
        h.fake.wait_for(|fake| fake.land().is_none()).await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(land_unset(&h), 2, "{:?}", h.fake.requests());
    // A late tell of the stopped runtime sends nothing.
    land_act(&h, words::RULE_LAND_NUDGE, 2_000);
    h.sup.show_land().await;
    assert_eq!(land_set(&h, "since=1000 nudges=1"), 0);
}

/// A RE-CLAIM SENDS THE STANDING HOLD AGAIN: aterm drops the hold with the claim (the
/// connection's close), and the claim the reconnect proves sets it again after its own claim.
/// NEGATIVE CONTROL: a hold an earlier image left under this process's proof comes down at the
/// first claim, though this image never showed one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_re_claim_sends_the_standing_hold_again() {
    let h = started(&[], |fake, _| {
        fake.set_status("autopilot", "land");
        fake.leave_land(std::process::id(), report(500, 3, None));
    })
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    // NEGATIVE CONTROL: the earlier image's hold.
    assert!(
        h.fake.wait_for(|fake| fake.land().is_none()).await,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(land_unset(&h), 1);
    shown_after_land(&h).await;
    let claims = noted(&h, CLAIMED);
    claim_connection(&h).close();
    wait_until("the reconnect sets the standing hold again", || {
        noted(&h, CLAIMED) > claims && land_set(&h, "since=1000 nudges=0") == 2
    })
    .await;
    assert_eq!(h.fake.land(), Some(report(1_000, 0, None)));
    let requests = h.fake.requests();
    let last_claim = requests
        .iter()
        .rposition(|r| r.contains("meta set supervisor codex-alab@"));
    let last_set = requests
        .iter()
        .rposition(|r| r.ends_with(" meta set harness_land since=1000 nudges=0"));
    assert!(last_claim < last_set, "{requests:#?}");
}

/// AN ATERM BEFORE THE VERB IS ASKED ONCE PER CHANGE: it answers `meta set|unset
/// harness_land` as a field it does not know (`ERR unknown meta field (…)`); the claim's first
/// unset is asked once, the same hold is not asked again at any tell or sync, and the nudge
/// that moves it is asked once. NEGATIVE CONTROL: the same claim on an aterm that takes the
/// verb shows the next change.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_aterm_before_the_verb_is_asked_once_per_change() {
    let h = started(&[], |fake, _| {
        fake.set_status("autopilot", "land");
        fake.support_harness_land(false);
    })
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    wait_until("the first claim's unset", || land_unset(&h) == 1).await;
    land_act(&h, words::RULE_LAND, 1_000);
    wait_until("the hold is asked", || {
        land_set(&h, "since=1000 nudges=0") == 1
    })
    .await;
    for _ in 0..3 {
        h.autopilot.tell_land(&h.rt);
        evaluate(&h).await;
        h.sup.show_land().await;
    }
    assert_eq!(
        land_set(&h, "since=1000 nudges=0"),
        1,
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(
        noted(
            &h,
            "landing not shown: aterm takes no harness_land (ERR unknown meta field"
        ),
        2,
        "{:?}",
        h.sup.journal()
    );
    land_act(&h, words::RULE_LAND_NUDGE, 2_000);
    wait_until("the nudge is asked", || {
        land_set(&h, "since=1000 nudges=1") == 1
    })
    .await;
    h.sup.show_land().await;
    assert_eq!(land_requests(&h), 3, "{:?}", h.fake.requests());
    assert_eq!(h.fake.land(), None);
    // NEGATIVE CONTROL: the verb taken.
    h.fake.support_harness_land(true);
    land_act(&h, words::RULE_LAND_NUDGE, 3_000);
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 2, None)))
            .await,
        "{:?}",
        h.fake.requests()
    );
}

/// A DENIED HOLD IS NOT ASKED AGAIN UNDER ITS CLAIM: aterm answers `ERR denied`, and neither a
/// tell again nor a nudge asks again while that claim stands. NEGATIVE CONTROL: the next claim
/// (the reconnect's) asks again, and the standing hold is shown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_denied_hold_is_not_asked_again_under_its_claim() {
    let h = settled().await;
    h.fake.reply_once(
        &format!(
            "@{} meta set harness_land since=1000 nudges=0",
            h.fake.sid()
        ),
        "ERR denied\n",
    );
    land_act(&h, words::RULE_LAND, 1_000);
    wait_until("the denial", || {
        noted(&h, "landing not shown: ERR denied") == 1
    })
    .await;
    h.autopilot.tell_land(&h.rt);
    h.sup.show_land().await;
    land_act(&h, words::RULE_LAND_NUDGE, 2_000);
    h.sup.show_land().await;
    assert_eq!(land_requests(&h), 2, "{:?}", h.fake.requests());
    assert_eq!(h.fake.land(), None);
    // NEGATIVE CONTROL: the next claim.
    let claims = noted(&h, CLAIMED);
    claim_connection(&h).close();
    wait_until("the next claim asks again", || {
        noted(&h, CLAIMED) > claims && h.fake.land() == Some(report(1_000, 1, None))
    })
    .await;
    assert_eq!(land_requests(&h), 3, "{:?}", h.fake.requests());
}

/// NOTHING IS SENT WITHOUT A PROVEN CLAIM: while another holder has the session, the land act
/// shows nothing and a hold wanted is never sent. NEGATIVE CONTROL: once that holder's claim
/// goes and codex-alab's own is proven, the same hold is shown at the next look.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_is_sent_without_a_proven_claim() {
    let h = started(&[], |fake, _| {
        fake.set_status("autopilot", "land");
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
    land_act(&h, words::RULE_LAND, 1_000);
    assert!(
        !h.sup
            .want_land_for(h.rt.key(), Some(report(1_000, 0, None)))
    );
    h.sup.show_land().await;
    assert!(!h.sup.held());
    assert_eq!(land_requests(&h), 0, "{:?}", h.fake.requests());
    // NEGATIVE CONTROL: the other holder's claim goes.
    h.fake.clear_claim();
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    evaluate(&h).await;
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(1_000, 0, None)))
            .await,
        "{:?}",
        h.sup.journal()
    );
}

/// A NON-PRIMARY THREAD'S LAND DOES NOT SHOW ([`crate::supervisor`]'s `land_want`): another
/// root's hold stands for that thread only, and shows once the TUI makes it the primary thread;
/// the landing follows the primary back. NEGATIVE CONTROL: before the switch the tab's landing
/// is the primary thread's (none).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_non_primary_threads_land_does_not_show() {
    let h = settled().await;
    let other = ThreadId::new().to_string();
    h.shared.note_root_thread(&other);
    assert!(h.sup.want_land_for(&other, Some(report(4_000, 2, None))));
    h.sup.show_land().await;
    assert_eq!(land_requests(&h), 1, "{:?}", h.fake.requests());
    primary(&h, &other);
    assert!(
        h.fake
            .wait_for(|fake| fake.land() == Some(report(4_000, 2, None)))
            .await,
        "{:?}",
        h.sup.journal()
    );
    primary(&h, h.rt.key());
    assert!(h.fake.wait_for(|fake| fake.land().is_none()).await);
    assert_eq!(land_set(&h, "since=4000 nudges=2"), 1);
    assert_eq!(land_unset(&h), 2);
}
