// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! RULING F'S WALL HALF (stage 2 of the port, 2026-10-28): a wall whose own tries are spent at
//! the same answer is held (aterm's `act_at_point` hold, `TurnEndState::repeats`), its dated
//! resets lifting the hold for one more answer, and the walls the owner ruled must be retried
//! for ever (the model's capacity outages, the network, the cyber policy's refusal) never held.
//! Aterm's wall cases from its `turn_end_tests.rs` and `same_reply_tests.rs`, through the pure
//! decider; each with its negative control.

use super::*;
use crate::decide::held_wall_try;
use crate::decide::hold_told;
use crate::replies::FUTILE_AFTER_TRIES;
use crate::replies::LIMIT_STANDS;
use crate::replies::LIMIT_UNREAD;
use crate::replies::Told;
use pretty_assertions::assert_eq;

/// The usage window's wall, its reset `reset` (Codex's `resets_at`) or none.
fn usage(reset: Option<Instant>) -> Wall {
    Wall::Usage {
        reset_at: reset,
        reset_label: reset.map(|_| "16:00".to_string()),
    }
}

fn cut_off(retry_at: Option<Instant>) -> Wall {
    Wall::Server {
        what: "an API error (the reply was cut off)".to_string(),
        cut_off: true,
        retry_at,
    }
}

fn overlong() -> Wall {
    Wall::Overlong {
        what: "a reply past its output maximum".to_string(),
    }
}

/// An act's answer at `wall`, quickly and with no work.
fn met(at: Instant, wall: Wall) -> TurnEnd {
    TurnEnd {
        wall: Some(wall),
        text: None,
        ..ours(at, "", QUICK)
    }
}

/// Each act answered by `answer(at)`, at most `most` answers: the tries typed before a point
/// held, and the decision that held (`None`: never held).
fn tries_until_held(
    s: &mut ThreadState,
    p: &Policy,
    t: &Timing,
    at: &mut Instant,
    answer: &dyn Fn(Instant) -> TurnEnd,
    most: usize,
) -> (usize, Option<Decision>) {
    let mut tries = 0;
    for _ in 0..most {
        *at += QUICK;
        s.observe_turn_end(answer(*at), t);
        let (d, went) = act(s, p, t, *at, None);
        if !typed(&d) {
            assert_eq!(d.act, Act::Nothing, "{d:?}");
            return (tries, Some(d));
        }
        tries += 1;
        *at = went;
    }
    (tries, None)
}

/// A USAGE WALL NAMING NO RESET IS TRIED EXACTLY THREE TIMES, THEN HELD (aterm's SR
/// `a_read_wall_with_no_usable_reset_is_tried_through_its_ladder_then_held`, TT
/// `a_long_answer_at_a_read_wall_is_no_work_…`): its three resumes each met the same, the
/// fourth answer holds, an hour on still, told in aterm's `limit_repeats` words (no reset
/// read); a person's turn that meets it the same lets exactly one more try go (its tries
/// clamped to two), then it holds again. A long quiet answer is no work either. NEGATIVE
/// CONTROL: answers that ran a tool are never held.
#[test]
fn a_usage_wall_naming_no_reset_is_tried_three_times_then_held_and_a_persons_turn_gives_one_more() {
    let t = Timing::default();
    let p = policy();
    for worked in [QUICK, 5 * MIN] {
        let (mut s, mut at) = begun(&p, &t, None);
        let answer = |at: Instant| TurnEnd {
            worked,
            ..met(at, usage(None))
        };
        let (tries, held) = tries_until_held(&mut s, &p, &t, &mut at, &answer, 12);
        assert_eq!(tries, FUTILE_AFTER_TRIES, "{:?}", s.replies);
        let held = held.unwrap_or_else(|| panic!("held"));
        assert_eq!(held.held, Some(HeldBy::SameReply));
        assert_eq!(
            hold_told(&s, &p, HeldBy::SameReply, Instant::now()),
            Told::Limit { reset_read: false }
        );
        assert!(
            Told::Limit { reset_read: false }
                .line()
                .contains(LIMIT_UNREAD)
        );
        assert_eq!(
            decide(&s, &p, &t, at + 60 * MIN, NO_JITTER).held,
            Some(HeldBy::SameReply),
            "an hour on, still held"
        );
        // A person's turn the wall answers the same: one more try.
        at += MIN;
        s.observe_turn_end(
            TurnEnd {
                by_autopilot: false,
                by_person: true,
                ..met(at, usage(None))
            },
            &t,
        );
        let kept = s.replies.last().unwrap_or_else(|| panic!("kept"));
        assert_eq!((kept.answers, kept.tries), (1, FUTILE_AFTER_TRIES - 1));
        let (d, went) = act(&mut s, &p, &t, at, None);
        assert!(matches!(d.act, Act::ResumeLimit { .. }), "one more: {d:?}");
        at = went;
        let (tries, held) = tries_until_held(&mut s, &p, &t, &mut at, &answer, 4);
        assert_eq!(
            (tries, held.map(|d| d.held)),
            (0, Some(Some(HeldBy::SameReply)))
        );
    }
    // NEGATIVE CONTROL: work at each answer.
    let (mut s, mut at) = begun(&p, &t, None);
    let (tries, held) = tries_until_held(
        &mut s,
        &p,
        &t,
        &mut at,
        &|at| tooled(met(at, usage(None))),
        8,
    );
    assert_eq!((tries, held), (8, None));
}

/// A RESET MOVING FORWARD WITH EACH ANSWER KEEPS THE WALL'S TRIES (aterm's SR
/// `a_reset_moving_forward_with_each_answer_keeps_the_walls_tries`, `…standing_past_its_reset_…`):
/// each resume past the reset it waited for met the window again naming the next: three
/// resumes, then held, told as a limit standing past its reset. NEGATIVE CONTROL: a reset that
/// has not passed keeps every unspent try going, a run of nine no-work answers included,
/// where the same run at a wall naming no reset holds.
#[test]
fn a_reset_moving_forward_keeps_the_walls_tries_and_an_unpassed_reset_keeps_trying() {
    let t = Timing::default();
    let p = policy();
    let (mut s, mut at) = begun(&p, &t, None);
    let (tries, held) = tries_until_held(
        &mut s,
        &p,
        &t,
        &mut at,
        &|at| met(at, usage(Some(at + 5 * MIN))),
        12,
    );
    assert_eq!(tries, FUTILE_AFTER_TRIES, "{:?}", s.replies);
    assert_eq!(held.and_then(|d| d.held), Some(HeldBy::SameReply));
    assert_eq!(
        hold_told(&s, &p, HeldBy::SameReply, Instant::now()),
        Told::Limit { reset_read: true }
    );
    assert!(
        Told::Limit { reset_read: true }
            .line()
            .contains(LIMIT_STANDS)
    );
    // NEGATIVE CONTROL: a run of nine, tries unspent, the reset ahead or none.
    let at_run = |reset: Option<Instant>| {
        let now = Instant::now();
        let point = met(now, usage(reset));
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.seed_replies(
            Replies {
                run: crate::replies::NO_WORK_ACTS + FUTILE_AFTER_TRIES,
                ..Replies::of(&[Kept {
                    mark: mark_of(&point),
                    answers: 9,
                    walled: true,
                    tries: FUTILE_AFTER_TRIES - 1,
                }])
            },
            Some(now),
        );
        s.point = Some(point);
        decide(&s, &p, &t, now, NO_JITTER)
    };
    let ahead = at_run(Some(Instant::now() + 30 * MIN));
    assert!(matches!(ahead.act, Act::ResumeLimit { .. }), "{ahead:?}");
    assert_eq!(at_run(None).held, Some(HeldBy::SameReply));
}

/// A DATED RESET THE LAST ANSWER DID NOT SHOW LIFTS A SPENT WALL'S HOLD FOR ONE MORE ANSWER
/// (aterm's `repeats`: `since(r) && (tries < FUTILE || !same_reset(replies_reset, r))`, and its
/// block after the arms): a usage wall held at its spent tries, its point naming the reset
/// the last answer showed, holds, as does the same reset read again 20 s off; a point naming
/// another reset, after the last answer, waits for it and resumes once; that resume met the
/// limit standing again holds. NEGATIVE CONTROL: a reset before the last answer lifts nothing.
#[test]
fn a_dated_reset_not_the_last_answers_lifts_a_spent_wall_for_exactly_one_more_answer() {
    let t = Timing::default();
    let p = policy();
    let now = Instant::now();
    let shown = now + 60 * MIN;
    let held_at = |reset: Instant| {
        let point = met(now, usage(Some(reset)));
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.seed_replies(
            Replies::of(&[Kept {
                mark: mark_of(&point),
                answers: 3,
                walled: true,
                tries: FUTILE_AFTER_TRIES,
            }]),
            Some(now),
        );
        s.replies_reset = Some(shown);
        s.point = Some(point);
        s
    };
    for reset in [shown, shown + Duration::from_secs(20)] {
        let s = held_at(reset);
        assert_eq!(
            decide(&s, &p, &t, now, NO_JITTER).held,
            Some(HeldBy::SameReply),
            "the reset the last answer showed"
        );
    }
    // NEGATIVE CONTROL: a reset before the last answer.
    let s = held_at(now - MIN);
    assert_eq!(
        decide(&s, &p, &t, now, NO_JITTER).held,
        Some(HeldBy::SameReply)
    );
    // Another reset: one more answer.
    let next = shown + 5 * 60 * MIN;
    let mut s = held_at(next);
    let (d, went) = act(&mut s, &p, &t, now, None);
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
    assert!(went >= next, "waited for the reset");
    let mut at = went;
    let (tries, held) = tries_until_held(
        &mut s,
        &p,
        &t,
        &mut at,
        &|at| met(at, usage(Some(at + 5 * 60 * MIN))),
        4,
    );
    assert_eq!(
        (tries, held.and_then(|d| d.held)),
        (0, Some(HeldBy::SameReply))
    );
}

/// A RELATIVE RESET LIFTS NOTHING ONCE THE WALL'S TRIES ARE SPENT (aterm's `relative =
/// since(r) && tries < FUTILE`, which lifts the hold only while the tries last, where the
/// hold of a wall's reply waits for its tries to be spent anyway): a reply cut off with the
/// provider's retry advice (Codex's `Retry-After`, a duration) each time is tried three times,
/// then held; held at its spent tries with the advice ahead, it stays held. NEGATIVE CONTROL: a
/// dated reset ahead that the last answer did not show lifts the same spent hold.
#[test]
fn a_relative_reset_keeps_trying_until_the_tries_are_spent_and_lifts_nothing_after() {
    let t = Timing::default();
    let p = policy();
    let (mut s, mut at) = begun(&p, &t, None);
    let (tries, held) = tries_until_held(
        &mut s,
        &p,
        &t,
        &mut at,
        &|at| met(at, cut_off(Some(at + QUICK))),
        12,
    );
    assert_eq!(
        (tries, held.and_then(|d| d.held)),
        (FUTILE_AFTER_TRIES, Some(HeldBy::SameReply))
    );
    let now = Instant::now();
    let spent_at = |wall: Wall| {
        let point = met(now, wall);
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.seed_replies(
            Replies::of(&[Kept {
                mark: mark_of(&point),
                answers: 3,
                walled: true,
                tries: FUTILE_AFTER_TRIES,
            }]),
            Some(now),
        );
        s.point = Some(point);
        decide(&s, &p, &t, now, NO_JITTER)
    };
    assert_eq!(
        spent_at(cut_off(Some(now + QUICK))).held,
        Some(HeldBy::SameReply)
    );
    // NEGATIVE CONTROL: a dated reset ahead.
    let d = spent_at(usage(Some(now + 30 * MIN)));
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
}

/// A REPLY CUT OFF IS HELD AFTER ITS THREE TRIES, TOLD QUIETLY (aterm's exception_tests
/// `an_api_errors_wall_held_after_its_notice_left_is_told_stalled_not_a_limit`): three
/// continuations of the cut reply, each answered cut off again with no work, then held, told
/// as the quiet stop (never a limit's words, never loud), Land included. NEGATIVE CONTROL: the
/// API measured down makes the cut-off an outage, retried for ever.
#[test]
fn a_cut_off_reply_is_held_after_its_three_tries_and_told_quietly() {
    let t = Timing::default();
    for p in [policy(), land()] {
        let (mut s, mut at) = begun(&p, &t, Some(work()));
        let (tries, held) =
            tries_until_held(&mut s, &p, &t, &mut at, &|at| met(at, cut_off(None)), 12);
        assert_eq!(tries, FUTILE_AFTER_TRIES, "{:?}", p.autopilot);
        assert_eq!(held.and_then(|d| d.held), Some(HeldBy::SameReply));
        assert_eq!(
            hold_told(&s, &p, HeldBy::SameReply, Instant::now()),
            Told::Stop(HeldBy::SameReply)
        );
    }
    // NEGATIVE CONTROL: the API measured down.
    let p = policy();
    let (mut s, mut at) = begun(&p, &t, None);
    let mut typed_into = 0;
    for _ in 0..12 {
        at += QUICK;
        s.reach = Reach::Down { since: at };
        s.observe_turn_end(met(at, cut_off(None)), &t);
        let d = decide(&s, &p, &t, at, NO_JITTER);
        assert!(typed(&d), "{d:?}");
        let went = d.not_before.map_or(at, |due| due.max(at));
        s.acted(&d.act, went, &t);
        typed_into += 1;
        at = went;
    }
    assert_eq!((typed_into, s.replies), (12, Replies::default()));
}

/// AN OVERLONG REPLY THAT RAN TOOLS CLIMBS ITS LADDER ACROSS ITS WORK; WITHOUT TOOLS IT GETS
/// EXACTLY THREE TRIES (aterm's TT `an_overlong_reply_climbs_its_ladder_across_work`): the
/// work each try leads to is the long reply the wall cuts, so it is real work, and the tries
/// go on. NEGATIVE CONTROL: the same wall answered with no tool run is tried three times, then
/// held.
#[test]
fn an_overlong_reply_that_ran_tools_keeps_retrying_and_without_tools_gets_three_tries() {
    let t = Timing::default();
    let p = policy();
    let (mut s, mut at) = begun(&p, &t, None);
    let (tries, held) = tries_until_held(
        &mut s,
        &p,
        &t,
        &mut at,
        &|at| TurnEnd {
            worked: 8 * MIN,
            ..tooled(met(at, overlong()))
        },
        8,
    );
    assert_eq!((tries, held), (8, None));
    // NEGATIVE CONTROL: no tool.
    let (mut s, mut at) = begun(&p, &t, None);
    let (tries, held) = tries_until_held(
        &mut s,
        &p,
        &t,
        &mut at,
        &|at| TurnEnd {
            worked: 8 * MIN,
            ..met(at, overlong())
        },
        8,
    );
    assert_eq!(
        (tries, held.and_then(|d| d.held)),
        (FUTILE_AFTER_TRIES, Some(HeldBy::SameReply))
    );
}

/// THE MODEL'S CAPACITY, THE NETWORK AND THE CYBER POLICY ARE NEVER HELD, AND THEIR ANSWERS
/// NEVER COUNT (the owner's ruling, 2026-10-28; aterm's TT
/// `a_sustained_overload_is_tried_blind_on_its_doubling_ladder_and_never_held`, exception_tests
/// `an_outage_walls_same_answers_raise_nothing_however_long_it_stands`): an overload, a 500, a
/// 429, a request that timed out and the network, each answered the same with no work twelve
/// times, are tried twelve times, nothing kept; the cyber policy's refusal answered keeps
/// nothing, and over replies kept spent at its own mark its point is still continued.
/// NEGATIVE CONTROL: the same spent replies at a reply cut off hold.
#[test]
fn capacity_network_and_the_cyber_policy_are_never_held_however_long_they_repeat() {
    let t = Timing::default();
    let p = policy();
    let server = |what: &str| Wall::Server {
        what: what.to_string(),
        cut_off: false,
        retry_at: None,
    };
    for wall in [
        server("an API error (overloaded)"),
        server("an API error (500)"),
        server("an API error (429)"),
        server("an API error (the request timed out)"),
        Wall::Network {
            what: "an API error (the API could not be reached)".to_string(),
        },
    ] {
        let (mut s, mut at) = begun(&p, &t, None);
        let (tries, held) =
            tries_until_held(&mut s, &p, &t, &mut at, &|at| met(at, wall.clone()), 12);
        assert_eq!((tries, held), (12, None), "{wall:?}");
        assert_eq!(s.replies, Replies::default(), "{wall:?}");
    }
    // The cyber policy's refusal answers an act: nothing counted.
    let (mut s, at) = begun(&p, &t, None);
    s.observe_turn_end(met(at + QUICK, Wall::Policy), &t);
    assert_eq!(s.replies, Replies::default());
    let spent_at = |wall: Wall| {
        let now = Instant::now();
        let point = met(now, wall);
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.seed_replies(
            Replies {
                run: 4 * crate::replies::NO_WORK_ACTS,
                ..Replies::of(&[Kept {
                    mark: mark_of(&point),
                    answers: 12,
                    walled: true,
                    tries: 12,
                }])
            },
            Some(now),
        );
        s.point = Some(point.clone());
        (s.repeats(&point), decide(&s, &p, &t, now, NO_JITTER))
    };
    let (repeats, d) = spent_at(Wall::Policy);
    assert!(!repeats);
    assert_eq!(
        (d.held, d.act.rule()),
        (None, words::RULE_POLICY_CONTINUE),
        "{d:?}"
    );
    // NEGATIVE CONTROL: a reply cut off.
    let (repeats, d) = spent_at(cut_off(None));
    assert!(repeats);
    assert_eq!(d.held, Some(HeldBy::SameReply));
}

/// THE QUOTA WALL HOLDS AT ITS THIRD TRY, TOLD AS A LIMIT THAT NAMES NO RESET (aterm's
/// `limit_repeats` for its spend wall). NEGATIVE CONTROL: a person's turn that worked lifts it,
/// and its next try goes.
#[test]
fn a_quota_wall_holds_at_its_spent_tries_and_a_persons_work_lifts_it() {
    let t = Timing::default();
    let p = policy();
    let (mut s, mut at) = begun(&p, &t, None);
    let (tries, held) = tries_until_held(
        &mut s,
        &p,
        &t,
        &mut at,
        &|at| met(at, Wall::Entitlement),
        12,
    );
    assert_eq!(tries, FUTILE_AFTER_TRIES);
    assert_eq!(held.and_then(|d| d.held), Some(HeldBy::SameReply));
    assert_eq!(
        hold_told(&s, &p, HeldBy::SameReply, Instant::now()),
        Told::Limit { reset_read: false }
    );
    // NEGATIVE CONTROL: a person's turn that ran a tool, met by the wall again.
    at += MIN;
    s.observe_turn_end(
        tooled(TurnEnd {
            by_autopilot: false,
            by_person: true,
            ..met(at, Wall::Entitlement)
        }),
        &t,
    );
    assert_eq!(s.held_by(), None);
    let (d, _) = act(&mut s, &p, &t, at, None);
    assert_eq!(d.act.rule(), words::RULE_LIMIT_RESUME, "{d:?}");
}

/// A HELD WALL OUTLIVES A RE-EXEC AND IS DROPPED AT ATERM'S RELAUNCH, as the arms at no wall
/// are: the try its hold sets aside ([`held_wall_try`]) is the record's recovery, so the image
/// after a re-exec, its replies seeded from the record's pace, stands at the wall and holds it,
/// and an Off and back types the try. NEGATIVE CONTROL: aterm's relaunch, whose memory seeds no
/// replies, stands at the wall and types the try at once.
#[test]
fn a_held_wall_survives_a_re_exec_and_clears_at_a_relaunch() {
    use codex_protocol::protocol::CodexErrorInfo;
    const EPOCH: u64 = 1_800_000_000_000;
    const FAILED: &str = "the-held-wall-turn";
    let t = Timing::default();
    let p = policy();
    let (mut before, mut at) = begun(&p, &t, None);
    let (_, held) = tries_until_held(&mut before, &p, &t, &mut at, &|at| met(at, usage(None)), 12);
    assert_eq!(held.and_then(|d| d.held), Some(HeldBy::SameReply));
    // NEGATIVE CONTROL for the capture: a point that does not hold carries no held try.
    let (fresh, _) = begun(&p, &t, None);
    assert_eq!(held_wall_try(&fresh, &p, &t, at, NO_JITTER), None);
    let held_try = held_wall_try(&before, &p, &t, at, NO_JITTER)
        .unwrap_or_else(|| panic!("the held wall's try"));
    assert!(
        matches!(held_try.act, Act::ResumeLimit { .. }),
        "{held_try:?}"
    );
    let recovery =
        crate::recovery::capture("the-thread", FAILED, &before, &held_try, false, at, EPOCH)
            .unwrap_or_else(|| panic!("captured"));
    let items = [codex_history::RolloutItem::EventMsg(
        serde_json::from_value::<codex_protocol::protocol::EventMsg>(serde_json::json!({
            "type": "task_complete",
            "turn_id": FAILED,
            "last_agent_message": null,
            "error": {
                "message": "the provider ended this turn",
                "codex_error_info": CodexErrorInfo::UsageLimitExceeded,
            },
        }))
        .unwrap_or_else(|err| panic!("{err}")),
    )];
    let after = |reexec: bool| {
        let mut s = ThreadState::default();
        s.observe_resume(true, None, !reexec);
        s.observe_mode(Mode::Cruise);
        if reexec {
            s.seed_replies(before.replies, before.replies_at);
            s.seed_replies_reset(before.replies_reset);
        }
        assert!(crate::recovery::restore(
            &mut s, &items, &recovery, at, EPOCH, &t
        ));
        s
    };
    let mut s = after(true);
    let (d, _) = act(&mut s, &p, &t, at, None);
    assert_eq!((d.act, d.held), (Act::Nothing, Some(HeldBy::SameReply)));
    s.park_off();
    let d = decide(&s, &p, &t, at, NO_JITTER);
    assert!(
        matches!(d.act, Act::ResumeLimit { .. }),
        "Off and back: {d:?}"
    );
    // NEGATIVE CONTROL: aterm's relaunch.
    let s = after(false);
    let d = decide(&s, &p, &t, at, NO_JITTER);
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "relaunch: {d:?}");
}

/// A RESET SLEPT THROUGH IS THE RESET IT WAS AWAKE (review of 4c0168cedd, finding 6): a usage
/// wall held at its spent tries, its point naming the reset its last answer showed, stays held
/// once that reset has passed, whether the host was awake as it passed or slept through it (the
/// point's reset and the one the answer showed move back by the same time slept). NEGATIVE
/// CONTROL: a point naming another reset lifts the hold for one more answer, awake or asleep.
#[test]
fn a_spent_walls_reset_is_one_reset_whether_or_not_the_host_slept_through_it() {
    let t = Timing::default();
    let p = policy();
    // Far from the clock's start, so the moments before it are on the clock.
    let now = Instant::now() + 300 * MIN;
    let answered = now - 120 * MIN;
    let shown = now + 10 * MIN;
    let held_at = |reset: Instant| {
        let point = met(answered, usage(Some(reset)));
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.seed_replies(
            Replies::of(&[Kept {
                mark: mark_of(&point),
                answers: 3,
                walled: true,
                tries: FUTILE_AFTER_TRIES,
            }]),
            Some(answered),
        );
        s.replies_reset = Some(shown);
        s.point = Some(point);
        s
    };
    let asleep = |mut s: ThreadState| {
        let base = Duration::from_secs(5_000);
        assert_eq!(s.slept_to(Some(base), now), None);
        assert_eq!(s.slept_to(Some(base + 60 * MIN), now), Some(60 * MIN));
        s
    };
    // Awake, decided 20 min past the reset; asleep, an hour slept through it.
    let awake = decide(&held_at(shown), &p, &t, shown + 20 * MIN, NO_JITTER);
    assert_eq!(
        (awake.act, awake.held),
        (Act::Nothing, Some(HeldBy::SameReply))
    );
    let slept = decide(&asleep(held_at(shown)), &p, &t, now, NO_JITTER);
    assert_eq!(
        (slept.act, slept.held),
        (Act::Nothing, Some(HeldBy::SameReply))
    );
    // NEGATIVE CONTROL: another reset.
    let next = shown + 5 * 60 * MIN;
    let awake = decide(&held_at(next), &p, &t, now, NO_JITTER);
    assert!(matches!(awake.act, Act::ResumeLimit { .. }), "{awake:?}");
    let slept = decide(&asleep(held_at(next)), &p, &t, now, NO_JITTER);
    assert!(matches!(slept.act, Act::ResumeLimit { .. }), "{slept:?}");
}
