// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! LAND'S HOLD AS THE DECIDER DERIVES IT ([`ThreadState::land_held`], gap 11 of the parity
//! sweep, 2026-10-28: a codex-alab worker under Land showed no landing in aterm), as aterm's
//! `TurnEndState::land_held` derives its `LandHeld`: `since` is the act that began the hold,
//! stamped as it is typed; the nudges count from it; a land act's yield as judged names its
//! legs and the repository's counts until any other turn begins or ends; a landing, a point
//! left to a person, leaving Land, an Off and a person's turn take it down; the record carries
//! the standing hold to the image after a re-exec or a relaunch. Each with its negative
//! control.

use codex_alab_aterm::LandLeg;
use codex_alab_aterm::LandRead;
use codex_alab_aterm::LandReport;
use pretty_assertions::assert_eq;

use super::*;

/// When the instruction in [`landing`] was typed, unix ms.
const SINCE: u64 = 1_000;

/// A thread under Land whose instruction was typed (stamped [`SINCE`]) at a person's turn's
/// end at `now`.
fn landing(t: &Timing, now: Instant) -> ThreadState {
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), t);
    let d = run(&mut s, &land(), t, now);
    assert_eq!(typed(&d), Some((words::RULE_LAND, words::LAND)));
    s.stamp_land_hold(SINCE);
    s
}

/// What the land act's yield read, as the hold names it.
fn read(s: &ThreadState) -> Option<LandRead> {
    s.land_held().and_then(|held| held.read)
}

/// The legs the yield read.
fn legs(s: &ThreadState) -> Vec<LandLeg> {
    read(s).map(|read| read.legs).unwrap_or_default()
}

/// LAND'S INSTRUCTION BEGINS THE HOLD AT ITS STAMP: `since` is the act's own time, no nudge
/// counted, nothing read yet; a later stamp moves nothing. NEGATIVE CONTROLS: the person's
/// turn before it holds nothing under Land, and the act unstamped says nothing (the wire
/// takes no hold without its `since`).
#[test]
fn the_land_act_begins_the_hold_at_its_stamp() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), &t);
    s.observe_mode(Mode::Land);
    // NEGATIVE CONTROL: no land act yet.
    assert_eq!(s.land_held(), None);
    let d = run(&mut s, &land(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_LAND, words::LAND)));
    // NEGATIVE CONTROL: typed, not yet stamped.
    assert_eq!(s.land_held(), None);
    s.stamp_land_hold(SINCE);
    assert_eq!(
        s.land_held(),
        Some(LandReport {
            since_ms: SINCE,
            nudges: 0,
            read: None,
        })
    );
    s.stamp_land_hold(SINCE + 1);
    assert_eq!(s.land_held().map(|held| held.since_ms), Some(SINCE));
}

/// EACH NUDGE COUNTS FROM THE HOLD, WHICH KEEPS ITS `since`: three nudges name three, and the
/// stamp each act gets begins nothing. NEGATIVE CONTROL: the hold a landing's next land act
/// begins counts from that act (aterm's `base`, read after the act's own count), not the
/// episode's nudges before it, and names its own `since`.
#[test]
fn each_nudge_counts_from_the_hold_and_keeps_its_since() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = landing(&t, now);
    let mut at = now;
    for n in 1..=3_u32 {
        at += 30 * MIN;
        s.observe_turn_end(
            tooled(ours(
                at,
                "Merged the parser; pushing the lexer next.",
                10 * MIN,
            )),
            &t,
        );
        let d = run(&mut s, &land(), &t, at);
        assert_eq!(
            typed(&d),
            Some((words::RULE_LAND_NUDGE, words::LAND_NUDGE)),
            "{n}"
        );
        s.stamp_land_hold(SINCE + u64::from(n));
        assert_eq!(
            s.land_held().map(|held| (held.since_ms, held.nudges)),
            Some((SINCE, n)),
            "{n}"
        );
    }
    assert_eq!(s.land.nudges, 3);
    // NEGATIVE CONTROL: landed, then the landed task's own wake leaves work open; the land act
    // over it begins a hold of its own.
    at += 30 * MIN;
    s.observe_turn_end(ours(at, CLEAN, 10 * MIN), &t);
    assert_eq!(run(&mut s, &land(), &t, at).act, Act::Landed);
    assert_eq!(s.land_held(), None);
    at += 30 * MIN;
    s.observe_turn_end(
        TurnEnd {
            by_autopilot: false,
            by_person: false,
            ..end(
                at,
                "Started on the docs; the fix is parked on a branch.",
                5 * MIN,
            )
        },
        &t,
    );
    assert_eq!(s.land_held(), None, "relanded: no act yet");
    let d = probed(&mut s, &land(), &t, at, Some(work()));
    assert_eq!(
        typed(&d).map(|(rule, _)| rule),
        Some(words::RULE_LAND_NUDGE)
    );
    s.acted(&d.act, at, &t);
    s.stamp_land_hold(5_000);
    assert_eq!(
        s.land_held().map(|held| (held.since_ms, held.nudges)),
        Some((5_000, 0)),
        "the act that begins the hold is its start, not one of its nudges"
    );
    assert_eq!(s.land.nudges, 4);
}

/// A LAND ACT'S YIELD NAMES WHAT HELD IT (aterm's `hold_at_yield`): its needs, Land's strict
/// reading's open words and a missing defense from its words; the repository's work from the
/// probe for that point (`repo` with its counts, `repo-unread` where it could not read, neither
/// over a clean one). A clean defense names no leg until its probe finds work. NEGATIVE
/// CONTROLS: a probe for another point leaves the reading; nothing is cut.
#[test]
fn a_land_acts_yield_names_what_held_it() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = landing(&t, now);
    let at = now + 20 * MIN;
    s.observe_turn_end(
        ours(
            at,
            "Nothing left: merged, but the lexer is not landed.\nNEEDS YOU: the signing key \
             password",
            10 * MIN,
        ),
        &t,
    );
    assert_eq!(
        read(&s),
        Some(LandRead {
            legs: vec![LandLeg::Needs, LandLeg::Open],
            open: vec!["not landed".to_string()],
            ..LandRead::default()
        })
    );
    assert_eq!(s.land_held().map(|held| held.since_ms), Some(SINCE));
    s.observe_mode(Mode::Land);
    s.probed(Some(work()));
    assert_eq!(
        read(&s),
        Some(LandRead {
            legs: vec![LandLeg::Needs, LandLeg::Repo, LandLeg::Open],
            open: vec!["not landed".to_string()],
            untracked: 1,
            unpushed: 1,
            unlanded: 1,
            ..LandRead::default()
        })
    );
    // NEGATIVE CONTROL: another point's probe.
    s.probed_for(Some(at - MIN), RepoRead::Unread);
    assert_eq!(
        legs(&s),
        [LandLeg::Needs, LandLeg::Repo, LandLeg::Open],
        "another point's probe"
    );
    s.probed(RepoRead::Unread);
    assert_eq!(
        read(&s),
        Some(LandRead {
            legs: vec![LandLeg::Needs, LandLeg::RepoUnread, LandLeg::Open],
            open: vec!["not landed".to_string()],
            ..LandRead::default()
        })
    );
    s.probed(None);
    assert_eq!(legs(&s), [LandLeg::Needs, LandLeg::Open]);
    assert!(
        !legs(&s).contains(&LandLeg::Cut),
        "codex hands each message whole"
    );
    // The nudge, then a reply with no defense line.
    let d = probed(&mut s, &land(), &t, at, None);
    s.acted(&d.act, at, &t);
    let at = at + 20 * MIN;
    s.observe_turn_end(ours(at, "Merging the lexer now.", 10 * MIN), &t);
    assert_eq!(legs(&s), [LandLeg::Defense]);
    // A clean defense: no leg until its probe finds work.
    let d = probed(&mut s, &land(), &t, at, None);
    s.acted(&d.act, at, &t);
    let at = at + 20 * MIN;
    s.observe_turn_end(ours(at, CLEAN, 10 * MIN), &t);
    assert_eq!(
        s.land_held(),
        Some(LandReport {
            since_ms: SINCE,
            nudges: 2,
            read: None,
        })
    );
    s.observe_mode(Mode::Land);
    s.probed(Some(work()));
    assert_eq!(legs(&s), [LandLeg::Repo]);
}

/// ONLY A LAND ACT'S WHOLE YIELD IS READ, UNTIL ANY OTHER TURN ENDS: the reading goes at the
/// next turn's end, a goal's own continuation included (the hold stands, nothing read); a
/// yield cut by a wall or stopped reads nothing. NEGATIVE CONTROL: the same yield whole reads
/// its legs.
#[test]
fn only_a_land_acts_whole_yield_is_read_until_another_turn_ends() {
    let t = Timing::default();
    let now = Instant::now();
    let reply = "Merging the lexer now.";
    let mut s = landing(&t, now);
    let at = now + 20 * MIN;
    let mut walled_at = s.clone();
    let mut stopped = s.clone();
    // NEGATIVE CONTROL: whole.
    s.observe_turn_end(ours(at, reply, 10 * MIN), &t);
    assert_eq!(legs(&s), [LandLeg::Defense]);
    walled_at.observe_turn_end(
        walled(
            at,
            Wall::Server {
                what: "a server error".into(),
                cut_off: true,
                retry_at: None,
            },
        ),
        &t,
    );
    assert_eq!(walled_at.land_held().map(|held| held.read), Some(None));
    stopped.observe_turn_end(
        TurnEnd {
            aborted: true,
            ..ours(at, reply, 10 * MIN)
        },
        &t,
    );
    assert_eq!(stopped.land_held().map(|held| held.read), Some(None));
    // A goal's own continuation ends.
    s.observe_turn_end(
        TurnEnd {
            by_autopilot: false,
            by_person: false,
            ..end(at + MIN, "Still merging.", 5 * MIN)
        },
        &t,
    );
    assert_eq!(
        s.land_held(),
        Some(LandReport {
            since_ms: SINCE,
            nudges: 0,
            read: None,
        })
    );
}

/// THE HOLD COMES DOWN at a landing, a point left to a person (aterm's `theirs` face: one
/// stands only at Land's opening message or after an Off, gap 32), leaving Land, an Off and a
/// person's turn; a new episode holds nothing until its instruction. A person's quick message
/// mid-job is no point left to them: their turn ends the episode, and Land's instruction carries
/// the work on (where aterm 791977cae8 keeps its hold there). NEGATIVE CONTROL: each from the
/// same standing hold, which shows until then.
#[test]
fn a_landing_theirs_leaving_land_an_off_and_a_persons_turn_take_the_hold_down() {
    let t = Timing::default();
    let now = Instant::now();
    let held = landing(&t, now);
    // NEGATIVE CONTROL: the hold stands.
    assert!(held.land_held().is_some());
    // A landing.
    let at = now + 20 * MIN;
    let mut s = held.clone();
    s.observe_turn_end(ours(at, CLEAN, 10 * MIN), &t);
    let d = probed(&mut s, &land(), &t, at, None);
    assert_eq!(d.act, Act::Landed);
    assert!(s.land_held().is_some(), "judged, not yet landed");
    s.acted(&d.act, at, &t);
    assert_eq!(s.land_held(), None);
    // Left to a person.
    let mut s = held.clone();
    s.left = true;
    assert_eq!(s.land_held(), None);
    // A person's quick message mid-job: not theirs, and carried.
    let mut s = held.clone();
    by_hand(
        &mut s,
        &t,
        Some(Mode::Land),
        at,
        "Yes: stage 2 is half done.",
        Duration::from_secs(20),
    );
    assert!(!s.left);
    assert_eq!(s.land_held(), None);
    assert_eq!(
        typed(&run(&mut s, &land(), &t, at)),
        Some((words::RULE_LAND, words::LAND))
    );
    // Leaving Land, and back: a new episode.
    let mut s = held.clone();
    s.observe_mode(Mode::Cruise);
    assert_eq!(s.land_held(), None);
    s.observe_mode(Mode::Land);
    assert_eq!(s.land_held(), None);
    // An Off.
    let mut s = held.clone();
    s.park_off();
    assert_eq!(s.land_held(), None);
    // A person's turn, then the new episode's instruction.
    let mut s = held;
    s.observe_turn_end(end(at, "Also fix the docs typo.", 5 * MIN), &t);
    assert_eq!(s.land_held(), None);
    assert_eq!(
        typed(&run(&mut s, &land(), &t, at)),
        Some((words::RULE_LAND, words::LAND))
    );
    s.stamp_land_hold(9_000);
    assert_eq!(
        s.land_held().map(|held| (held.since_ms, held.nudges)),
        Some((9_000, 0))
    );
}

/// THE FIRST ACT THAT TYPES LAND'S WORDS BEGINS THE HOLD (aterm's: the act that "gave" Land's
/// instruction), the carry-on's under Land included; so does the first land act of an image
/// that knows no beginning (a re-exec's whose record carries none, an older build's).
/// NEGATIVE CONTROLS: the carry-on under Cruise begins none, and the re-exec'd image holds
/// nothing before its act.
#[test]
fn the_first_act_that_types_lands_words_begins_the_hold() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), true);
    let mut cruise = s.clone();
    let d = run(&mut s, &land(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_HOST_TURN, words::LAND)));
    s.stamp_land_hold(SINCE);
    assert_eq!(
        s.land_held().map(|held| (held.since_ms, held.nudges)),
        Some((SINCE, 0))
    );
    // NEGATIVE CONTROL: Cruise.
    run(&mut cruise, &policy(), &t, now);
    cruise.stamp_land_hold(SINCE);
    assert_eq!(cruise.land_held(), None);
    assert_eq!(cruise.land.hold.since_ms, None);
    // A re-exec's owed nudge.
    let owed = Owed {
        native_origin: None,
        rule: words::RULE_LAND_NUDGE,
        text: words::LAND_NUDGE.to_string(),
        mode: Mode::Land,
        not_before: now,
        repo_owed: None,
        resume_goal: false,
        landing: false,
    };
    let mut s = re_execed("Merged the parser.", Some(owed), false, now);
    // NEGATIVE CONTROL: before its act.
    assert_eq!(s.land_held(), None);
    let d = run(&mut s, &land(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_LAND_NUDGE, words::LAND_NUDGE)));
    s.stamp_land_hold(7_000);
    assert_eq!(
        s.land_held().map(|held| (held.since_ms, held.nudges)),
        Some((7_000, 0))
    );
}

/// ANOTHER'S TURN TAKES THE READING DOWN AS IT BEGINS (aterm's `took`; review of gap 11,
/// finding 2): after a land act's yield was read, a turn that no act of codex-alab's awaits (a
/// goal's own continuation) leaves the hold standing with no legs read from its start, and
/// says so once. NEGATIVE CONTROL: the turn of the nudge codex-alab typed keeps the reading
/// through its start, until its own yield is read.
#[test]
fn anothers_turn_takes_the_reading_down_as_it_begins() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = landing(&t, now);
    // The instruction's own turn begins: nothing read yet, nothing goes.
    assert!(!s.observe_turn_start());
    let at = now + 20 * MIN;
    s.observe_turn_end(ours(at, "Merging the lexer now.", 10 * MIN), &t);
    assert_eq!(legs(&s), [LandLeg::Defense]);
    // NEGATIVE CONTROL: the nudge's turn.
    let mut nudged = s.clone();
    let d = probed(&mut nudged, &land(), &t, at, None);
    assert_eq!(
        typed(&d).map(|(rule, _)| rule),
        Some(words::RULE_LAND_NUDGE)
    );
    nudged.acted(&d.act, at, &t);
    assert!(!nudged.observe_turn_start());
    assert_eq!(legs(&nudged), [LandLeg::Defense], "the nudge's own turn");
    // A goal's own continuation begins.
    assert!(s.observe_turn_start());
    assert_eq!(
        s.land_held(),
        Some(LandReport {
            since_ms: SINCE,
            nudges: 0,
            read: None,
        })
    );
    assert!(!s.observe_turn_start(), "said once");
}

/// A USAGE RESUME UNDER LAND THAT FIRST TYPES LAND'S WORDS BEGINS THE HOLD (CONTRACT §5 Land;
/// review of gap 11, finding 5): a person's turn under Land met the usage window, and the
/// resume typed at its reset is the episode's first land act; its stamp is the hold's `since`,
/// no nudge counted. A goal resumed through `GoalService` instead typed no words, so it is no
/// land act and begins no hold: the instruction typed at the goal's turn end does (this test
/// pinned the goal's resume as the first land act until the forever-persist lane, item b,
/// found the instruction was then never typed: the goal's continuation is the upstream
/// template). NEGATIVE CONTROL: a resume after Land's instruction keeps the hold the
/// instruction began.
#[test]
fn a_usage_resume_under_land_begins_the_hold() {
    let t = Timing::default();
    let now = Instant::now();
    let usage = Wall::Usage {
        reset_at: None,
        reset_label: None,
    };
    let mut walled_at = ThreadState::default();
    walled_at.observe_person_turn();
    walled_at.observe_mode(Mode::Land);
    walled_at.observe_turn_end(walled(now, usage.clone()), &t);
    let d = decide(&walled_at, &land(), &t, now, NO_JITTER);
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
    assert_eq!(walled_at.land_held(), None, "no land act yet");
    let mut resumed = walled_at.clone();
    resumed.goal_resumed(now, &t);
    resumed.stamp_land_hold(SINCE);
    assert_eq!(
        resumed.land_held(),
        None,
        "the goal's resume typed no land act"
    );
    let mut typed_at = walled_at;
    typed_at.acted(&d.act, now, &t);
    typed_at.stamp_land_hold(SINCE);
    assert_eq!(
        typed_at.land_held(),
        Some(LandReport {
            since_ms: SINCE,
            nudges: 0,
            read: None,
        })
    );
    // NEGATIVE CONTROL: after the instruction.
    let mut s = landing(&t, now);
    let at = now + 20 * MIN;
    s.observe_turn_end(walled(at, usage), &t);
    s.goal_resumed(at, &t);
    s.stamp_land_hold(SINCE + 1);
    assert_eq!(s.land_held().map(|held| held.since_ms), Some(SINCE));
}

/// A HOLD THE RECORD CARRIES STANDS IN THE IMAGE AFTER A RE-EXEC OR A RELAUNCH (aterm's
/// `seed_land_hold`, from its books; review of gap 11, finding 1): the image before carries
/// where its hold began and the nudges since ([`ThreadState::land_hold_carried`]), and the
/// image after, taking it up with the act the image before owed, says the same `since` and
/// nudges at once; an owed nudge counts on from them, its stamp beginning nothing, and a
/// limit's resume of a land act keeps them. NEGATIVE CONTROLS: a record that carries none (an
/// older build's) shows nothing until the nudge, which begins the hold at its own stamp, the
/// nudges started over; a hold this memory holds already is not replaced; a landing carries
/// none.
#[test]
fn a_hold_the_record_carries_stands_after_a_re_exec() {
    let t = Timing::default();
    let now = Instant::now();
    let mut before = landing(&t, now);
    let mut at = now;
    for n in 1..=3_u64 {
        at += 30 * MIN;
        before.observe_turn_end(
            tooled(ours(
                at,
                "Merged the parser; pushing the lexer next.",
                10 * MIN,
            )),
            &t,
        );
        run(&mut before, &land(), &t, at);
        before.stamp_land_hold(SINCE + n);
    }
    let carried = before.land_hold_carried();
    assert_eq!(carried, Some((SINCE, 3)));
    let nudge = Owed {
        native_origin: None,
        rule: words::RULE_LAND_NUDGE,
        text: words::LAND_NUDGE.to_string(),
        mode: Mode::Land,
        not_before: at,
        repo_owed: None,
        resume_goal: false,
        landing: false,
    };
    let resume = Owed {
        rule: words::RULE_LIMIT_RESUME,
        text: words::LAND.to_string(),
        resume_goal: true,
        landing: true,
        ..nudge.clone()
    };
    // The record's pace is taken up first, then the history and the owed act.
    let after = |owed: &Owed, carried: Option<(u64, u32)>| {
        let mut s = ThreadState::default();
        if let Some((since_ms, nudges)) = carried {
            s.seed_land_hold(since_ms, nudges);
        }
        s.observe_resume(true, Some("Merged the parser."), false);
        s.observe_owed(Some(owed.clone()), false, at);
        s
    };
    let since_and_nudges = |s: &ThreadState| s.land_held().map(|held| (held.since_ms, held.nudges));
    let mut s = after(&nudge, carried);
    assert_eq!(since_and_nudges(&s), Some((SINCE, 3)));
    assert_eq!(s.land_hold_carried(), Some((SINCE, 3)), "carried on");
    let d = run(&mut s, &land(), &t, at);
    assert_eq!(
        typed(&d).map(|(rule, _)| rule),
        Some(words::RULE_LAND_NUDGE)
    );
    s.stamp_land_hold(7_000);
    assert_eq!(since_and_nudges(&s), Some((SINCE, 4)));
    let mut s = after(&resume, carried);
    assert_eq!(since_and_nudges(&s), Some((SINCE, 3)));
    s.goal_resumed(at, &t);
    s.stamp_land_hold(7_000);
    assert_eq!(since_and_nudges(&s), Some((SINCE, 3)));
    // NEGATIVE CONTROL: none carried.
    let mut s = after(&nudge, None);
    assert_eq!(s.land_held(), None);
    run(&mut s, &land(), &t, at);
    s.stamp_land_hold(7_000);
    assert_eq!(since_and_nudges(&s), Some((7_000, 0)));
    // A goal resumed in place of the owed resume typed no land act: no hold begins until one.
    let mut s = after(&resume, None);
    assert_eq!(s.land_held(), None);
    s.goal_resumed(at, &t);
    s.stamp_land_hold(7_000);
    assert_eq!(s.land_held(), None);
    let mut s = after(&resume, None);
    run(&mut s, &land(), &t, at);
    s.stamp_land_hold(7_000);
    assert_eq!(since_and_nudges(&s), Some((7_000, 0)));
    // NEGATIVE CONTROL: this memory's own hold stands.
    s.seed_land_hold(SINCE, 3);
    assert_eq!(since_and_nudges(&s), Some((7_000, 0)));
    // A held point restored across a re-exec with no owed act keeps the seeded hold.
    let mut held_after = ThreadState::default();
    held_after.seed_land_hold(SINCE, 3);
    held_after.observe_mode(Mode::Land);
    assert_eq!(held_after.land_hold_carried(), Some((SINCE, 3)));
    // A wall point restored via `observe_turn_end` (`restored: true`) after `seed_land_hold`
    // marks the episode typed so `land_held` and a subsequent usage resume keep the seeded hold.
    let mut restored_wall = ThreadState::default();
    restored_wall.observe_resume(true, None, false);
    restored_wall.seed_land_hold(SINCE, 3);
    restored_wall.observe_turn_end(
        TurnEnd {
            restored: true,
            ..walled(
                at,
                Wall::Usage {
                    reset_at: None,
                    reset_label: None,
                },
            )
        },
        &t,
    );
    assert_eq!(since_and_nudges(&restored_wall), Some((SINCE, 3)));
    restored_wall.goal_resumed(at, &t);
    restored_wall.stamp_land_hold(9_000);
    assert_eq!(since_and_nudges(&restored_wall), Some((SINCE, 3)));
    // An owed wall retry of a non-land turn (e.g. a goal continuation) in a seeded Land
    // episode also keeps `land_held` standing.
    let wall_retry = Owed {
        rule: words::RULE_API_RETRY,
        text: "keep going".to_string(),
        resume_goal: false,
        landing: false,
        ..nudge
    };
    let s = after(&wall_retry, carried);
    assert_eq!(since_and_nudges(&s), Some((SINCE, 3)));
    // NEGATIVE CONTROL: landed.
    at += 30 * MIN;
    before.observe_turn_end(ours(at, CLEAN, 10 * MIN), &t);
    assert_eq!(run(&mut before, &land(), &t, at).act, Act::Landed);
    assert_eq!(before.land_hold_carried(), None);
}
