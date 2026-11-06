// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Paced Cruise recovery and explicit Land holds through the pure decider (aterm's
//! `turn_end_tests.rs`, `turn_end_hold_tests.rs` and `same_reply_tests.rs`, the portable
//! subset: codex-alab sees every turn end, so aterm's screen-reading arms have no counterpart).
//! The wall half (stage 2: a wall whose tries are spent is held) is `walls_hold_tests.rs`.

use std::time::Duration;
use std::time::Instant;

use crate::decide::Act;
use crate::decide::Decision;
use crate::decide::Mode;
use crate::decide::Policy;
use crate::decide::Questions;
use crate::decide::Texts;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::decide::decide;
use crate::netwatch::Reach;
use crate::replies::HeldBy;
use crate::replies::Kept;
use crate::replies::NO_WORK_ACTS;
use crate::replies::Replies;
use crate::replies::mark_of;
use crate::repo::RepoWork;
use crate::words;
use pretty_assertions::assert_eq;

const MIN: Duration = Duration::from_secs(60);
const QUICK: Duration = Duration::from_secs(30);
const NO_JITTER: Duration = Duration::ZERO;

fn policy() -> Policy {
    Policy {
        continue_on: true,
        continue_text: Some("keep going".to_string()),
        continue_per_hour: None,
        approve_all: true,
        answer_questions: true,
        answer_text: Some("Pick the recommended option.".to_string()),
        questions: Questions::Recommended,
        retry_api_errors: true,
        probe_api: true,
        resume_limits: true,
        limit_wait: true,
        compact_on_context_wall: true,
        upgrade: true,
        upgrade_ask: false,
        facts: false,
        human_grace: Duration::from_secs(120),
        autopilot: Mode::Cruise,
        texts: Texts::default(),
    }
}

fn land() -> Policy {
    Policy {
        autopilot: Mode::Land,
        ..policy()
    }
}

fn work() -> RepoWork {
    RepoWork::of_git("/r", "?? new.rs\0", "", 1, 1, "origin/main").unwrap_or_default()
}

/// A person's turn.
fn theirs(at: Instant, text: &str, worked: Duration) -> TurnEnd {
    TurnEnd {
        at,
        by_autopilot: false,
        by_person: true,
        text: Some(text.to_string()),
        earlier: None,
        questions: false,
        worked,
        wall: None,
        aborted: false,
        ran_tools: false,
        restored: false,
    }
}

/// codex-alab's own turn (an act's answer).
fn ours(at: Instant, text: &str, worked: Duration) -> TurnEnd {
    TurnEnd {
        by_autopilot: true,
        by_person: false,
        ..theirs(at, text, worked)
    }
}

/// The program's own turn (a goal's continuation): nobody's.
fn own(at: Instant, text: &str, worked: Duration) -> TurnEnd {
    TurnEnd {
        by_autopilot: false,
        by_person: false,
        ..theirs(at, text, worked)
    }
}

fn tooled(end: TurnEnd) -> TurnEnd {
    TurnEnd {
        ran_tools: true,
        ..end
    }
}

fn typed(d: &Decision) -> bool {
    matches!(d.act, Act::Type { .. } | Act::ResumeLimit { .. })
}

/// The decision at the point, the mode folded and a probe answered with `found`; an act that
/// waits is carried out once due, as the runtime's timer does. Returns the decision and when
/// it went.
fn act(
    s: &mut ThreadState,
    p: &Policy,
    t: &Timing,
    now: Instant,
    found: Option<RepoWork>,
) -> (Decision, Instant) {
    s.observe_mode(p.autopilot);
    let mut d = decide(s, p, t, now, NO_JITTER);
    if d.act == Act::Probe {
        s.probed(found);
        d = decide(s, p, t, now, NO_JITTER);
    }
    let at = d.not_before.map_or(now, |due| due.max(now));
    s.acted(&d.act, at, t);
    (d, at)
}

/// A task begun by a person's turn of real work, its first act typed: the state and when.
fn begun(p: &Policy, t: &Timing, found: Option<RepoWork>) -> (ThreadState, Instant) {
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(tooled(theirs(now, "Stage 1 is in.", 10 * MIN)), t);
    let (d, at) = act(&mut s, p, t, now, found);
    assert!(typed(&d), "the act after the work: {d:?}");
    (s, at)
}

/// Each act answered quickly with no work by `answer(k)`, until a point holds: the answers it
/// took and the arm that holds it (aterm's HT `held_after`).
fn held_after(
    p: &Policy,
    found: Option<RepoWork>,
    answer: impl Fn(usize) -> String,
) -> (usize, HeldBy) {
    let t = Timing::default();
    let (mut s, mut at) = begun(p, &t, found.clone());
    for k in 0..4 * NO_WORK_ACTS {
        at += QUICK;
        s.observe_turn_end(ours(at, &answer(k), QUICK), &t);
        let (d, went) = act(&mut s, p, &t, at, found.clone());
        if matches!(&d.act, Act::Type { text, .. } if text == words::FORWARD_RECOVERY) {
            return (k + 1, s.held_by().expect("recovery retains its reason"));
        }
        if !typed(&d) {
            assert_eq!(d.act, Act::Nothing, "{d:?}");
            let held = d.held.unwrap_or_else(|| panic!("held: {d:?}"));
            assert_eq!(s.held_by(), Some(held));
            return (k + 1, held);
        }
        at = went;
    }
    panic!("never held");
}

/// A second unproductive reply changes approach after pacing. Real tool work
/// clears the repeated-reply state and returns to ordinary continuation.
#[test]
fn a_repeated_reply_recovers_after_pacing_and_a_tool_ends_recovery() {
    let t = Timing::default();
    let p = policy();
    let wordless = |at: Instant| TurnEnd {
        text: None,
        ..ours(at, "", QUICK)
    };
    let (mut s, mut at) = begun(&p, &t, None);
    at += QUICK;
    s.observe_turn_end(wordless(at), &t);
    let (d, went) = act(&mut s, &p, &t, at, None);
    assert!(
        typed(&d),
        "the first no-work answer is answered once more: {d:?}"
    );
    at = went + QUICK;
    s.observe_turn_end(wordless(at), &t);
    let (d, _) = act(&mut s, &p, &t, at, None);
    assert!(matches!(&d.act, Act::Type { text, .. } if text == words::FORWARD_RECOVERY));
    assert!(d.not_before.is_some_and(|due| due > at));
    assert_eq!(d.held, None);
    assert!(s.point.is_none(), "the recovery act consumed this point");
    // NEGATIVE CONTROL: the program's own next turn ran a tool.
    at += MIN;
    s.observe_turn_end(
        tooled(TurnEnd {
            text: None,
            ..own(at, "", QUICK)
        }),
        &t,
    );
    assert_eq!(s.held_by(), None);
    let (d, _) = act(&mut s, &p, &t, at, None);
    assert!(typed(&d), "real work ends the hold: {d:?}");
}

#[test]
fn repeated_recovery_keeps_running_and_respects_the_hourly_budget() {
    let timing = Timing::default();
    let policy = policy();
    let (mut state, mut at) = begun(&policy, &timing, None);
    for turn in 0..20 {
        at += QUICK;
        state.observe_turn_end(ours(at, "Still waiting for approval.", QUICK), &timing);
        let (decision, due) = act(&mut state, &policy, &timing, at, None);
        assert!(typed(&decision), "turn {turn}: {decision:?}");
        if turn > 0 {
            assert!(
                matches!(&decision.act, Act::Type { text, .. } if text == words::FORWARD_RECOVERY)
            );
            assert!(due > at, "recovery must be paced");
        }
        assert_eq!(decision.held, None);
        at = due;
    }
    let capped = Policy {
        continue_per_hour: Some(1),
        ..policy.clone()
    };
    at += QUICK;
    state.observe_turn_end(ours(at, "Still waiting for approval.", QUICK), &timing);
    let decision = decide(&state, &capped, &timing, at, NO_JITTER);
    assert!(
        decision
            .not_before
            .is_some_and(|due| due >= at + timing.window - QUICK)
    );
    assert!(typed(&decision));
    let off = Policy {
        continue_on: false,
        ..policy
    };
    assert_eq!(
        decide(&state, &off, &timing, at, NO_JITTER).act,
        Act::Nothing
    );
}

/// WHAT IS REAL WORK (aterm's TT
/// `a_short_turn_counts_its_earlier_words_and_a_tool_or_new_words_after_min_work_is_real_work`):
/// the acts typed, out of four answers, before one is held. Earlier words that change are
/// another reply; a turn that ran a tool did real work however short (a busy spell of
/// `min_work` in new words is the next test's). NEGATIVE CONTROLS: a count is no word,
/// the same words before and last are one reply, and `min_work` that ran no tool said again
/// is no work (Codex's events are tool rows, as aterm's Claude reader reads them).
#[test]
fn a_tool_or_new_words_is_real_work_and_a_count_or_a_long_quiet_spell_said_again_is_not() {
    const FAILS: &str = "The build still fails: `parse_expr` is missing a lifetime.";
    const RAN: Duration = Duration::from_secs(90);
    let t = Timing::default();
    let p = land();
    let run = |said_before: &dyn Fn(usize) -> Option<String>, worked: Duration, tools: bool| {
        let (mut s, mut at) = begun(&p, &t, None);
        for k in 1..=4 {
            at += worked;
            let end = TurnEnd {
                earlier: said_before(k),
                ran_tools: tools,
                ..ours(at, FAILS, worked)
            };
            s.observe_turn_end(end, &t);
            let (d, went) = act(&mut s, &p, &t, at, None);
            if !typed(&d) {
                assert_eq!(d.act, Act::Nothing, "{d:?}");
                assert_eq!(
                    decide(&s, &p, &t, went + 60 * MIN, NO_JITTER).act,
                    Act::Nothing
                );
                return k - 1;
            }
            at = went;
        }
        4
    };
    let fix = |k: usize| {
        [
            "a named lifetime",
            "an elided one",
            "a 'static bound",
            "a clone",
        ][k - 1]
    };
    assert_eq!(
        run(&|k| Some(format!("Trying {}.", fix(k))), RAN, false),
        4,
        "earlier words that change"
    );
    assert_eq!(
        run(&|k| Some(format!("Trying fix {k} of 4.")), RAN, false),
        1,
        "the control: a count is no word"
    );
    assert_eq!(
        run(&|_| Some("Rebuilding.".to_string()), RAN, false),
        1,
        "the control: the same words"
    );
    assert_eq!(
        run(&|_| Some("Rebuilding.".to_string()), RAN, true),
        4,
        "a tool run, however short"
    );
    assert_eq!(
        run(&|_| None, t.min_work, false),
        1,
        "the control: min_work with no tool, said again"
    );
}

/// A BUSY SPELL OF `min_work` IN NEW WORDS IS KEPT BUT ENDS THE RUN (aterm's `idle` is under
/// `min_work`, its `kept_long` keeps a long quiet answer): five short quiet answers, each in
/// its own words, then one of `min_work` in new words, then one more short quiet answer: each
/// is typed into, the run at one after the last, and the `min_work` answer was kept. NEGATIVE
/// CONTROL: a 90 s answer in its place (under `min_work`) is the sixth no-work answer in a row
/// and holds as no work.
#[test]
fn a_busy_spell_of_min_work_in_new_words_is_kept_but_ends_the_run() {
    const LONG: &str = "Reworked the lexer's token table.";
    let t = Timing::default();
    let p = land();
    let after = |worked: Duration| {
        let (mut s, mut at) = begun(&p, &t, None);
        let short = ["alpha", "beta", "gamma", "delta", "epsilon"]
            .map(|w| format!("Checked the {w} case."));
        let mut said: Vec<(String, Duration)> = short.iter().map(|w| (w.clone(), QUICK)).collect();
        said.push((LONG.to_string(), worked));
        said.push(("Checked the zeta case.".to_string(), QUICK));
        let mut held = None;
        let mut long = None;
        for (words, took) in said {
            at += took;
            s.observe_turn_end(ours(at, &words, took), &t);
            if words == LONG {
                long = s.replies.last();
            }
            let (d, went) = act(&mut s, &p, &t, at, None);
            if !typed(&d) {
                held = d.held;
                break;
            }
            at = went;
        }
        (held, s.replies.run, long)
    };
    let (held, run, long) = after(t.min_work);
    assert_eq!((held, run), (None, 1));
    assert_eq!(
        long.map(|k| (k.mark, k.answers)),
        Some((mark_of(&ours(Instant::now(), LONG, QUICK)), 1)),
        "the min_work answer is kept"
    );
    // NEGATIVE CONTROL: under min_work.
    let (held, _, _) = after(Duration::from_secs(90));
    assert_eq!(held, Some(HeldBy::NoWork));
}

/// SIX NO-WORK ANSWERS, EACH WORDED ITS OWN WAY, HOLD AS NO WORK (aterm's HT
/// `six_no_work_answers_each_worded_its_own_way_hold_as_no_work`), under Cruise's done check
/// and under Land's nudge over a repository with work (which keeps the defense from landing).
/// NEGATIVE CONTROL: two wordings by turns hold at the first repeat (aterm's SR
/// `…_in_two_wordings_by_turns_is_held_at_its_first_repeat`), as the same reply.
#[test]
fn six_no_work_answers_each_worded_its_own_way_hold_as_no_work() {
    let worded = |k: usize| {
        let case = char::from(b'a' + u8::try_from(k % 26).unwrap_or(0));
        format!("Nothing left: the parser is committed and pushed, case {case}.")
    };
    for (p, found) in [(policy(), None), (land(), Some(work()))] {
        assert_eq!(
            held_after(&p, found, worded),
            (NO_WORK_ACTS, HeldBy::NoWork),
            "{:?}",
            p.autopilot
        );
    }
    let two = |k: usize| ["Still on the parser.", "Still on the lexer."][k % 2].to_string();
    assert_eq!(held_after(&policy(), None, two), (3, HeldBy::SameReply));
}

/// A SWITCH TO LAND OVER A STANDING HOLD TYPES NOTHING (aterm's HT
/// `a_switch_to_land_over_a_standing_hold_types_nothing_and_off_then_land_types_lands_act`):
/// Land's instruction, which waits no back-off, is held as every act is. Off's park clears
/// the replies; the short streak stands across it, and Land's instruction goes at once.
/// NEGATIVE CONTROL: before the park the same point under Land types nothing.
#[test]
fn a_switch_to_land_over_a_hold_types_nothing_and_off_then_land_types_lands_instruction() {
    let t = Timing::default();
    let p = land();
    let (mut s, mut at) = begun(&p, &t, None);
    while s.held_by().is_none() {
        at += QUICK;
        s.observe_turn_end(ours(at, "Still on the parser.", QUICK), &t);
        at = act(&mut s, &p, &t, at, None).1;
    }
    let (d, _) = act(&mut s, &land(), &t, at + MIN, None);
    assert_eq!(
        (d.act, d.held),
        (Act::Nothing, Some(HeldBy::SameReply)),
        "a switch to Land restarts no typing"
    );
    s.park_off();
    assert_eq!(s.held_by(), None, "Off's park clears the replies");
    assert!(
        s.short_streak > 0,
        "the short streak stands: {}",
        s.short_streak
    );
    let (d, _) = act(&mut s, &land(), &t, at + 2 * MIN, None);
    assert_eq!(
        (&d.act, d.not_before),
        (
            &Act::Type {
                rule: words::RULE_LAND,
                text: words::LAND.to_string(),
            },
            None
        ),
        "Off, then Land, types Land's instruction at once"
    );
}

/// A PERSON'S TURN LIFTS THE HOLD, ONE MORE ACT AT THE SAME REPLY (aterm's arm 7; SR
/// `a_vendor_row_no_reader_names_is_typed_into_at_most_once_more_then_held`): their message
/// arriving touches nothing; their turn's end saying the newest reply again, idle, lets
/// exactly one more act go, and its same answer holds again. A person's turn in new words
/// clears the replies. NEGATIVE CONTROL: no person's turn, the hold stands.
#[test]
fn a_persons_turn_lets_one_more_act_go_at_the_same_reply_and_new_words_clear_it() {
    const SAME: &str = "Waiting on the review.";
    let t = Timing::default();
    let p = land();
    let held = || {
        let (mut s, mut at) = begun(&p, &t, None);
        while s.held_by().is_none() {
            at += QUICK;
            s.observe_turn_end(ours(at, SAME, QUICK), &t);
            at = act(&mut s, &p, &t, at, None).1;
        }
        (s, at)
    };
    let (mut s, mut at) = held();
    let before = s.replies;
    s.observe_person_turn();
    assert_eq!(
        s.replies, before,
        "the person's message arriving touches nothing"
    );
    at += QUICK;
    s.observe_turn_end(theirs(at, SAME, QUICK), &t);
    let (d, went) = act(&mut s, &p, &t, at, None);
    assert!(typed(&d), "one more act: {d:?}");
    at = went + QUICK;
    s.observe_turn_end(ours(at, SAME, QUICK), &t);
    let (d, _) = act(&mut s, &p, &t, at, None);
    assert_eq!(
        (d.act, d.held),
        (Act::Nothing, Some(HeldBy::SameReply)),
        "then held again"
    );
    // A person's turn in new words clears them.
    s.observe_turn_end(theirs(at + MIN, "Merged the review; what next?", QUICK), &t);
    assert_eq!(s.replies, Replies::default());
    // NEGATIVE CONTROL: no person's turn.
    let (s, at) = held();
    assert_eq!(
        decide(&s, &p, &t, at + 30 * MIN, NO_JITTER).held,
        Some(HeldBy::SameReply)
    );
}

/// Repeated questions switch to autonomous recovery; a new question is answered.
#[test]
fn repeated_questions_switch_to_recovery_and_new_questions_are_answered() {
    const ASKS: &str = "Should I rename the module?";
    let t = Timing::default();
    let p = policy();
    let (mut s, mut at) = begun(&p, &t, None);
    let mut rules = Vec::new();
    for _ in 0..2 {
        at += QUICK;
        s.observe_turn_end(ours(at, ASKS, QUICK), &t);
        let (d, went) = act(&mut s, &p, &t, at, None);
        rules.push(d.act.rule());
        at = went;
    }
    assert_eq!(rules, [words::RULE_ANSWER, words::RULE_CONTINUE]);
    assert!(s.point.is_none(), "the recovery act consumed this point");
    at += QUICK;
    s.observe_turn_end(own(at, "Should I split the module instead?", QUICK), &t);
    assert_eq!(
        act(&mut s, &p, &t, at, None).0.act.rule(),
        words::RULE_ANSWER
    );
}

/// THE RUN HOLDS ONLY WHILE THE POINT SHOWS ITS NEWEST REPLY (aterm's TT
/// `the_run_of_no_work_answers_holds_only_while_the_screen_shows_its_newest_reply`).
/// NEGATIVE CONTROL: a point in other words is no run's.
#[test]
fn the_run_of_no_work_answers_holds_only_at_its_newest_reply() {
    let now = Instant::now();
    let shown = ours(now, "This week's allowance is gone.", QUICK);
    let moved = ours(
        now,
        "Fixed the parser's lifetime; the suite passes now.",
        QUICK,
    );
    let mut s = ThreadState::default();
    s.seed_replies(
        Replies {
            run: NO_WORK_ACTS,
            ..Replies::of(&[Kept {
                mark: mark_of(&shown),
                answers: 1,
                walled: false,
                tries: 0,
            }])
        },
        Some(now),
    );
    assert!(s.repeats(&shown), "held at its newest reply");
    assert!(!s.repeats(&moved), "a point in other words is no run's");
}

/// THE PROGRAM'S OWN TURN THAT RAN NO TOOL COUNTS AS AN ACT'S ANSWER DOES (aterm's TT
/// `the_programs_own_long_turn_that_ran_no_tool_counts_as_an_acts_answer_does`; arm 5): its
/// own words again after a long spell, one more in the run and held; at a wall, no work
/// whatever its words. NEGATIVE CONTROLS: in new words it is work and the run starts afresh;
/// an outage's answer is no answer at all, the run standing where it was; with nothing kept,
/// the program's own turn counts nothing.
#[test]
fn the_programs_own_long_turn_that_ran_no_tool_counts_as_an_acts_answer_does() {
    const KEPT: &str = "Still waiting on the background suite.";
    let t = Timing::default();
    let now = Instant::now();
    let after = |end: TurnEnd| {
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.seed_replies(
            Replies {
                run: 2,
                ..Replies::of(&[Kept {
                    mark: mark_of(&ours(now, KEPT, QUICK)),
                    answers: 1,
                    walled: false,
                    tries: 0,
                }])
            },
            Some(now),
        );
        s.observe_turn_end(end, &t);
        s
    };
    let at = now + MIN;
    let s = after(own(at, KEPT, 4 * MIN));
    assert_eq!(s.replies.run, 3, "{:?}", s.replies);
    assert!(s.repeats(&ours(at, KEPT, QUICK)), "{:?}", s.replies);
    assert_eq!(
        decide(&s, &land(), &t, at, NO_JITTER).held,
        Some(HeldBy::SameReply)
    );
    assert_eq!(
        after(own(
            at,
            "Reworked the parser; the suite is building.",
            4 * MIN
        ))
        .replies
        .run,
        0
    );
    let walled = |wall: Wall| TurnEnd {
        wall: Some(wall),
        text: None,
        ..own(at, "", 4 * MIN)
    };
    let refused = walled(Wall::Fatal {
        why: "an invalid request".to_string(),
    });
    assert_eq!(
        after(refused).replies.run,
        3,
        "a wall's quiet answer is no work"
    );
    let overload = walled(Wall::Server {
        what: "an overload (529)".to_string(),
        cut_off: false,
        retry_at: None,
    });
    assert_eq!(
        after(overload).replies.run,
        2,
        "an outage's answer counts nothing"
    );
    // NEGATIVE CONTROL: nothing kept, the program's own quiet turn counts nothing.
    let mut s = ThreadState::default();
    s.observe_turn_end(own(at, KEPT, 4 * MIN), &t);
    assert_eq!(s.replies, Replies::default());
}

/// THE PROGRAM'S OWN TURNS IN NEW WORDS COUNT INTO THE RUN (aterm's SR
/// `a_no_work_turn_the_program_opens_itself_counts_as_an_acts_answer`): one act's no-work
/// answer kept, five of its goal's own quiet turns each in new words (the first of them the
/// act's answer) make the run of six,
/// and the next act is held as no work. NEGATIVE CONTROL: a goal turn that ran a tool lifts
/// the run.
#[test]
fn a_goals_own_quiet_turns_in_new_words_make_the_run_of_six() {
    let t = Timing::default();
    let p = land();
    let (mut s, mut at) = begun(&p, &t, None);
    at += QUICK;
    s.observe_turn_end(ours(at, "Event 0 seen.", QUICK), &t);
    assert!(typed(&act(&mut s, &p, &t, at, None).0));
    let words = [
        "Lexer event seen.",
        "Parser event seen.",
        "Linker event seen.",
        "Loader event seen.",
        "Runner event seen.",
    ];
    for (k, said) in words.iter().enumerate() {
        at += QUICK;
        s.observe_turn_end(own(at, said, QUICK), &t);
        assert_eq!(s.replies.run, k + 2, "{said}");
    }
    assert_eq!(
        decide(&s, &p, &t, at + 90 * MIN, NO_JITTER).held,
        Some(HeldBy::NoWork)
    );
    s.observe_turn_end(tooled(own(at + MIN, "Runner event seen.", QUICK)), &t);
    assert_eq!(s.replies, Replies::default());
}

/// A COMMAND'S OWED CONTINUATION IS HELD AT A REPLY THAT CAME AGAIN (aterm's TT
/// `a_reply_that_came_again_holds_a_commands_owed_continuation_and_a_resets_own`): the
/// compaction's continuation. NEGATIVE CONTROL: with no reply kept it is typed.
#[test]
fn a_repeated_reply_after_compaction_changes_approach() {
    let t = Timing::default();
    let now = Instant::now();
    let compacted = |replies: bool| {
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.observe_turn_end(
            TurnEnd {
                wall: Some(Wall::Context { auto_compact: true }),
                text: None,
                ..ours(now, "", QUICK)
            },
            &t,
        );
        let d = decide(&s, &policy(), &t, now, NO_JITTER);
        assert_eq!(d.act, Act::Compact, "{d:?}");
        s.acted(&d.act, now, &t);
        let end = ours(now + MIN, "Conversation compacted.", QUICK);
        if replies {
            s.observe_turn_end(end.clone(), &t);
            s.seed_replies(
                Replies::of(&[Kept {
                    mark: mark_of(&end),
                    answers: 2,
                    walled: false,
                    tries: 0,
                }]),
                Some(now),
            );
        } else {
            s.observe_turn_end(end, &t);
        }
        assert!(s.after_compact);
        decide(&s, &policy(), &t, now + MIN, NO_JITTER)
    };
    let d = compacted(true);
    assert!(matches!(&d.act, Act::Type { text, .. } if text == words::FORWARD_RECOVERY));
    let d = compacted(false);
    assert_eq!(
        d.act.rule(),
        words::RULE_COMPACT,
        "the control: typed: {d:?}"
    );
}

/// A TOOL EACH TIME IS NO REPEAT (aterm's SR
/// `the_same_reply_after_a_tool_run_each_time_is_no_repeat`):
/// the same words a dozen times, each after a tool run, are answered every time. NEGATIVE
/// CONTROL: the same words with no tool are held at the second.
#[test]
fn the_same_reply_after_a_tool_run_each_time_is_no_repeat() {
    const SAID: &str = "The suite still fails in the parser.";
    let t = Timing::default();
    let p = land();
    let acts = |tools: bool| {
        let (mut s, mut at) = begun(&p, &t, None);
        let mut acts = 0;
        for _ in 0..12 {
            at += QUICK;
            s.observe_turn_end(
                TurnEnd {
                    ran_tools: tools,
                    ..ours(at, SAID, QUICK)
                },
                &t,
            );
            let (d, went) = act(&mut s, &p, &t, at, None);
            if !typed(&d) {
                break;
            }
            acts += 1;
            at = went;
        }
        acts
    };
    assert_eq!(acts(true), 12);
    assert_eq!(acts(false), 1);
}

/// AN OUTAGE IS NEVER HELD AND ITS ANSWERS COUNT NOTHING (aterm's TT
/// `a_sustained_overload_is_tried_blind_on_its_doubling_ladder_and_never_held`, SR
/// `a_529_…_never_held`): the server's overload answered the same eight times is retried
/// every time, no reply kept. A WALL'S QUIET ANSWERS ARE COUNTED WITH THEIR OWN TRIES: a reply
/// past its output maximum, retried and met again, keeps its tries, and is held once three
/// are spent (stage 2). NEGATIVE CONTROL: that wall, the hold's own.
#[test]
fn an_overload_is_never_held_and_a_walls_quiet_answers_count_their_own_tries() {
    let t = Timing::default();
    let p = policy();
    let tries = |wall: Wall| {
        let (mut s, mut at) = begun(&p, &t, None);
        let mut rules = Vec::new();
        for _ in 0..8 {
            at += QUICK;
            s.observe_turn_end(
                TurnEnd {
                    wall: Some(wall.clone()),
                    text: None,
                    ..ours(at, "", QUICK)
                },
                &t,
            );
            let (d, went) = act(&mut s, &p, &t, at, None);
            rules.push(d.act.rule());
            if d.act == Act::Nothing {
                break;
            }
            at = went;
        }
        (rules, s)
    };
    let (rules, s) = tries(Wall::Server {
        what: "an overload (529)".to_string(),
        cut_off: false,
        retry_at: None,
    });
    assert_eq!(rules, [words::RULE_API_RETRY; 8]);
    assert_eq!(
        s.replies,
        Replies::default(),
        "an outage's answers count nothing"
    );
    // NEGATIVE CONTROL: a wall that holds.
    let (rules, s) = tries(Wall::Overlong {
        what: "a reply past its output maximum".to_string(),
    });
    assert_eq!(
        rules,
        [
            words::RULE_API_CUTOFF,
            words::RULE_API_CUTOFF,
            words::RULE_API_CUTOFF,
            "none"
        ],
        "three tries, then held"
    );
    let kept = s.replies.last().unwrap_or_else(|| panic!("kept"));
    // The first answered the continuation, the rest the wall's own tries.
    assert_eq!((kept.answers, kept.walled, kept.tries), (4, true, 3));
    assert_eq!(s.held_by(), Some(HeldBy::SameReply));
}

/// A RESTORED TURN END NEITHER COUNTS NOR LIFTS (the recovery's and a resumed history's
/// synthetic point): the replies kept stand across it. NEGATIVE CONTROL: the same end seen
/// live lifts them.
#[test]
fn a_restored_turn_end_neither_counts_nor_lifts() {
    let t = Timing::default();
    let now = Instant::now();
    let kept = Replies::of(&[Kept {
        mark: 7,
        answers: 2,
        walled: false,
        tries: 0,
    }]);
    let after = |restored: bool| {
        let mut s = ThreadState::default();
        s.seed_replies(kept, Some(now));
        s.observe_turn_end(
            TurnEnd {
                restored,
                ..theirs(now, "Back.", Duration::ZERO)
            },
            &t,
        );
        s.replies
    };
    assert_eq!(after(true), kept);
    assert_eq!(after(false), Replies::default());
}

/// A DATED RESET PASSED STARTS THE RUN AFRESH (aterm's block after its arms; SR
/// `a_read_walls_reset_lets_the_same_reply_be_answered_once_more`): a usage window resumed
/// through its tries, each resume met by the window again past its reset, leaves a run of
/// none, so the first quiet reply past the window counts one. NEGATIVE CONTROL: a window that
/// names no reset leaves the run of its no-work answers, the quiet reply's one more (the
/// wall's own hold at its spent tries is `walls_hold_tests.rs`'s).
#[test]
fn a_usage_windows_reset_passed_starts_the_run_afresh_and_one_naming_none_keeps_it() {
    let t = Timing::default();
    let p = policy();
    let after_resumes = |dated: bool| {
        let (mut s, mut at) = begun(&p, &t, None);
        for _ in 0..crate::replies::FUTILE_AFTER_TRIES {
            at += QUICK;
            let wall = Wall::Usage {
                reset_at: dated.then(|| at + 5 * MIN),
                reset_label: dated.then(|| "5:11 PM".to_string()),
            };
            s.observe_turn_end(
                TurnEnd {
                    wall: Some(wall),
                    text: None,
                    ..ours(at, "", QUICK)
                },
                &t,
            );
            let (d, went) = act(&mut s, &p, &t, at, None);
            assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
            at = went;
        }
        at += QUICK;
        s.observe_turn_end(ours(at, "Resumed; carrying on.", QUICK), &t);
        let (d, _) = act(&mut s, &p, &t, at, None);
        (d, s)
    };
    let (d, s) = after_resumes(true);
    assert!(typed(&d), "{d:?} {:?}", s.replies);
    assert_eq!(s.replies.run, 1, "{:?}", s.replies);
    // NEGATIVE CONTROL: no reset named.
    let (d, s) = after_resumes(false);
    assert!(typed(&d), "{d:?} {:?}", s.replies);
    assert_eq!(
        s.replies.run,
        crate::replies::FUTILE_AFTER_TRIES + 1,
        "{:?}",
        s.replies
    );
}

/// A RESTORED ANSWER NEITHER COUNTS NOR TAKES THE ACT IT WOULD ANSWER (the recovery's, a
/// resumed history's and the owed point's synthetic end, [`TurnEnd::restored`]; parity review
/// of the stage-1 port, test lens): a reply kept once, an act typed, and the same quiet reply
/// restored leaves its answers at one and the act still awaiting its answer. NEGATIVE CONTROL:
/// the same end seen live is the act's answer, the reply's second, and holds.
#[test]
fn a_restored_answer_neither_counts_nor_takes_the_act_awaited() {
    const SAID: &str = "Still on the parser.";
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.seed_replies(
        Replies::of(&[Kept {
            mark: mark_of(&ours(now, SAID, QUICK)),
            answers: 1,
            walled: false,
            tries: 0,
        }]),
        Some(now),
    );
    s.acted(
        &Act::Type {
            rule: words::RULE_CONTINUE,
            text: "keep going".to_string(),
        },
        now,
        &t,
    );
    assert_eq!(s.awaited, Some(false));
    let at = now + MIN;
    s.observe_turn_end(
        TurnEnd {
            restored: true,
            ..ours(at, SAID, QUICK)
        },
        &t,
    );
    assert_eq!(
        (s.replies.last().map(|k| k.answers), s.awaited),
        (Some(1), Some(false)),
        "a restored end counts nothing and answers nothing"
    );
    // NEGATIVE CONTROL: seen live.
    s.observe_turn_end(ours(at, SAID, QUICK), &t);
    assert_eq!(s.replies.last().map(|k| k.answers), Some(2));
    assert_eq!(s.held_by(), Some(HeldBy::SameReply));
}

/// AN ACT'S ANSWER THAT RAN A TOOL CLEARS A KEPT REPLY AND ENDS THE RUN (aterm's arm `Some(_) =>
/// self.replies = Replies::default()`): a quiet answer kept, the same words after a tool run,
/// then the same quiet words again are each typed into, the reply at one answer again; five
/// quiet answers each in its own words, one that ran a tool, then a sixth quiet one is typed
/// into, the run at one. NEGATIVE CONTROLS: with no tool run, the second same quiet answer
/// holds as the same reply, and the sixth quiet answer in a row as no work.
#[test]
fn an_acts_answer_that_ran_a_tool_clears_a_kept_reply_and_ends_the_run() {
    const SAID: &str = "The suite still fails in the parser.";
    let t = Timing::default();
    let p = land();
    // Each answer's words and whether it ran a tool, until one is held: the acts typed, the
    // arm that held, and the replies then.
    let answers = |said: &[(&str, bool)]| {
        let (mut s, mut at) = begun(&p, &t, None);
        let mut typed_into = 0;
        for (words, tools) in said {
            at += QUICK;
            s.observe_turn_end(
                TurnEnd {
                    ran_tools: *tools,
                    ..ours(at, words, QUICK)
                },
                &t,
            );
            let (d, went) = act(&mut s, &p, &t, at, None);
            if !typed(&d) {
                return (typed_into, d.held, s.replies);
            }
            typed_into += 1;
            at = went;
        }
        (typed_into, None, s.replies)
    };
    let (n, held, replies) = answers(&[(SAID, false), (SAID, true), (SAID, false)]);
    assert_eq!((n, held), (3, None));
    assert_eq!(replies.last().map(|k| k.answers), Some(1), "{replies:?}");
    // NEGATIVE CONTROL: no tool run.
    let (n, held, _) = answers(&[(SAID, false), (SAID, false), (SAID, false)]);
    assert_eq!((n, held), (1, Some(HeldBy::SameReply)));
    let worded = [
        "Looked at the alpha case.",
        "Looked at the beta case.",
        "Looked at the gamma case.",
        "Looked at the delta case.",
        "Looked at the epsilon case.",
        "Looked at the zeta case.",
        "Looked at the eta case.",
    ];
    let run = |tooled: bool| {
        let said: Vec<(&str, bool)> = worded
            .iter()
            .enumerate()
            .map(|(k, w)| (*w, tooled && k == 5))
            .collect();
        answers(&said)
    };
    let (n, held, replies) = run(true);
    assert_eq!((n, held, replies.run), (7, None, 1));
    // NEGATIVE CONTROL: the sixth quiet too.
    let (n, held, _) = run(false);
    assert_eq!((n, held), (NO_WORK_ACTS - 1, Some(HeldBy::NoWork)));
}

/// EVERY OUTAGE'S ANSWER COUNTS NOTHING, A CUT-OFF REPLY ONLY WHERE THE API IS MEASURED DOWN
/// (aterm's `outage_shown`: the API never reached, the server's own failure, or a reply cut
/// off where the reach is down; parity review of the stage-1 port, test lens): answered quickly
/// to the wall's own tries, the network's wall, and a cut-off reply with the API measured
/// down, keep no reply. NEGATIVE CONTROL: a cut-off reply with the API measured up, or not
/// measured, is counted, walled, with its tries.
#[test]
fn every_outages_answer_counts_nothing_and_a_cut_off_counts_unless_the_api_is_down() {
    let t = Timing::default();
    let p = policy();
    let cut = Wall::Server {
        what: "a reply cut off".to_string(),
        cut_off: true,
        retry_at: None,
    };
    let answered = |wall: &Wall, reach: &dyn Fn(Instant) -> Reach| {
        let (mut s, mut at) = begun(&p, &t, None);
        for _ in 0..4 {
            at += QUICK;
            s.reach = reach(at);
            s.observe_turn_end(
                TurnEnd {
                    wall: Some(wall.clone()),
                    text: None,
                    ..ours(at, "", QUICK)
                },
                &t,
            );
            s.acted(
                &Act::Type {
                    rule: words::RULE_API_RETRY,
                    text: "retry".to_string(),
                },
                at,
                &t,
            );
        }
        s.replies
    };
    let network = Wall::Network {
        what: "the API unreachable".to_string(),
    };
    assert_eq!(
        answered(&network, &|_| Reach::Unknown),
        Replies::default(),
        "the network's wall"
    );
    assert_eq!(
        answered(&cut, &|since| Reach::Down { since }),
        Replies::default(),
        "cut off, the API measured down"
    );
    // NEGATIVE CONTROL: the API measured up, or not measured.
    let up = |since| Reach::Up { since };
    let unknown = |_| Reach::Unknown;
    for reach in [&up as &dyn Fn(Instant) -> Reach, &unknown] {
        let kept = answered(&cut, reach)
            .last()
            .unwrap_or_else(|| panic!("counted"));
        // The first answered the continuation, the rest the wall's own tries.
        assert_eq!((kept.answers, kept.walled, kept.tries), (4, true, 3));
    }
}

/// A PERSON'S IDLE TURN OVER A RUN OF NO WORK LETS ONE MORE ACT GO AND STARTS THE RUN AFRESH
/// (aterm's arm 7, `Replies::of(&[Kept { answers: 1, .. }])`, its run none): six quiet answers
/// each in its own words hold as no work; the person's idle turn saying the newest reply again
/// is typed into once more, and the next quiet answer in new words is typed into too, the run
/// at one. NEGATIVE CONTROL: with no person's turn, the hold stands.
#[test]
fn a_persons_idle_turn_over_a_run_of_no_work_lets_one_more_act_go_and_starts_the_run_afresh() {
    let t = Timing::default();
    let p = land();
    let worded = |k: usize| {
        let case = char::from(b'a' + u8::try_from(k % 26).unwrap_or(0));
        format!("Nothing changed in case {case}.")
    };
    let (mut s, mut at) = begun(&p, &t, None);
    let mut k = 0;
    while s.held_by().is_none() {
        at += QUICK;
        s.observe_turn_end(ours(at, &worded(k), QUICK), &t);
        at = act(&mut s, &p, &t, at, None).1;
        k += 1;
    }
    assert_eq!((k, s.held_by()), (NO_WORK_ACTS, Some(HeldBy::NoWork)));
    // NEGATIVE CONTROL: no person's turn.
    assert_eq!(
        decide(&s, &p, &t, at + 30 * MIN, NO_JITTER).held,
        Some(HeldBy::NoWork)
    );
    at += QUICK;
    s.observe_turn_end(theirs(at, &worded(k - 1), QUICK), &t);
    let (d, went) = act(&mut s, &p, &t, at, None);
    assert!(typed(&d), "one more act: {d:?}");
    at = went + QUICK;
    s.observe_turn_end(ours(at, &worded(k), QUICK), &t);
    let (d, _) = act(&mut s, &p, &t, at, None);
    assert!(typed(&d), "the run starts afresh: {d:?}");
    assert_eq!(s.replies.run, 1, "{:?}", s.replies);
}

/// AN INTERRUPTED TURN ANSWERS NOTHING (`count_reply`'s first rule: the person's next turn
/// decides): a reply kept once, an act typed, and the same quiet words in an interrupted turn
/// leave the replies as they were. NEGATIVE CONTROL: the same end not interrupted is the
/// reply's second answer, and holds.
#[test]
fn an_interrupted_turn_answers_nothing() {
    const SAID: &str = "Still on the parser.";
    let t = Timing::default();
    let p = policy();
    let (mut s, mut at) = begun(&p, &t, None);
    at += QUICK;
    s.observe_turn_end(ours(at, SAID, QUICK), &t);
    let (d, went) = act(&mut s, &p, &t, at, None);
    assert!(typed(&d), "{d:?}");
    let before = s.replies;
    let mut live = s.clone();
    at = went + QUICK;
    s.observe_turn_end(
        TurnEnd {
            aborted: true,
            ..ours(at, SAID, QUICK)
        },
        &t,
    );
    assert_eq!(s.replies, before);
    // NEGATIVE CONTROL: not interrupted.
    live.observe_turn_end(ours(at, SAID, QUICK), &t);
    assert_eq!(live.replies.last().map(|k| k.answers), Some(2));
    assert_eq!(live.held_by(), Some(HeldBy::SameReply));
}

/// A DATED RESET PASSED KEEPS THE WALL'S REPLIES, WHATEVER THE RESUME'S ANSWER SAYS, AND THAT
/// ANSWER IS JUDGED OVER WHAT IT LEAVES (aterm's block after its arms runs at the point read
/// past the reset, which still shows the wall, before the resume is typed; SR
/// `two_wordings_of_a_wall_by_turns_at_resets_moving_forward_keep_their_tries`; parity review of
/// the stage-1 port, findings 3 and 6): a quiet reply and a usage wall kept, the wall's resume
/// answered past its reset by the quiet reply again after a long spell. The quiet reply was
/// cleared, so that answer says no kept reply again and starts no run; the wall stays kept,
/// its tries standing and its answers none. NEGATIVE CONTROL: before the reset, the same answer
/// is the kept reply said again, no work, and the run grows.
#[test]
fn a_dated_reset_passed_keeps_the_walls_replies_and_judges_the_answer_over_what_it_leaves() {
    const SAID: &str = "Waiting on the review.";
    let t = Timing::default();
    let now = Instant::now();
    let usage = TurnEnd {
        wall: Some(Wall::Usage {
            reset_at: Some(now + 5 * MIN),
            reset_label: Some("5:11 PM".to_string()),
        }),
        text: None,
        ..ours(now, "", QUICK)
    };
    let quiet = Kept {
        mark: mark_of(&ours(now, SAID, QUICK)),
        answers: 1,
        walled: false,
        tries: 0,
    };
    let wall = Kept {
        mark: mark_of(&usage),
        answers: 2,
        walled: true,
        tries: 1,
    };
    let answered = |at: Instant| {
        let mut s = ThreadState::default();
        s.seed_replies(
            Replies {
                run: 2,
                ..Replies::of(&[quiet, wall])
            },
            Some(now),
        );
        s.replies_reset = Some(now + 5 * MIN);
        s.acted(
            &Act::ResumeLimit {
                text: "resume".to_string(),
            },
            now,
            &t,
        );
        s.observe_turn_end(ours(at, SAID, 5 * MIN), &t);
        s.replies
    };
    let past = answered(now + 6 * MIN);
    let kept: Vec<(u64, usize, bool, usize)> = past
        .kept()
        .map(|k| (k.mark, k.answers, k.walled, k.tries))
        .collect();
    assert_eq!(
        (kept, past.run),
        (vec![(wall.mark, 0, true, 1), (quiet.mark, 1, false, 1)], 0)
    );
    // NEGATIVE CONTROL: before the reset.
    let before = answered(now + 4 * MIN);
    assert_eq!(
        (before.last().map(|k| (k.mark, k.answers)), before.run),
        (Some((quiet.mark, 2)), 3)
    );
    // A reset the host slept through is past on the awake clock too, even where the sleep is
    // longer than the reset's lead over `replies_at`.
    let answered_after_sleep = |slept: Duration, at: Instant| {
        let base = Duration::from_secs(5_000);
        let mut s = ThreadState::default();
        s.seed_replies(
            Replies {
                run: 2,
                ..Replies::of(&[quiet, wall])
            },
            Some(now),
        );
        s.replies_reset = Some(now + 5 * MIN);
        assert_eq!(s.slept_to(Some(base), now), None);
        assert_eq!(s.slept_to(Some(base + slept), now + MIN), Some(slept));
        s.acted(
            &Act::ResumeLimit {
                text: "resume".to_string(),
            },
            now + MIN,
            &t,
        );
        s.observe_turn_end(ours(at, SAID, 5 * MIN), &t);
        s.replies
    };
    let slept_past = answered_after_sleep(120 * MIN, now + 2 * MIN);
    let kept: Vec<(u64, usize, bool, usize)> = slept_past
        .kept()
        .map(|k| (k.mark, k.answers, k.walled, k.tries))
        .collect();
    assert_eq!(
        (kept, slept_past.run),
        (vec![(wall.mark, 0, true, 1), (quiet.mark, 1, false, 1)], 0)
    );
    // Partial sleep (2 min): 2 min awake is still 1 min short of the 5 min reset; 3 min awake reaches it.
    let slept_short = answered_after_sleep(2 * MIN, now + 2 * MIN);
    assert_eq!(
        (
            slept_short.last().map(|k| (k.mark, k.answers)),
            slept_short.run
        ),
        (Some((quiet.mark, 2)), 3)
    );
    let slept_reached = answered_after_sleep(2 * MIN, now + 3 * MIN);
    assert_eq!(slept_reached.run, 0);
}

/// A RE-EXEC OVER A HELD POINT STANDS AT IT AGAIN, SO AN OFF AND BACK TYPES THERE (parity
/// review of the stage-1 port, finding 1; aterm's SR takeover pin, "a person's Off ends the
/// hold"): the image after the re-exec, its replies and the held point's words seeded from the
/// record and its history resumed with no owed act, holds the act at that point; Off's park
/// lifts the hold, and the act is typed at once. NEGATIVE CONTROLS: with no held point seeded
/// no point stands, and the Off and back types nothing; a seeded point whose words are not the
/// newest kept reply's is not stood at (it would type at once).
#[test]
fn a_re_exec_over_a_held_point_stands_at_it_so_an_off_and_back_types_there() {
    use serde_json::json;
    const SAID: &str = "Still on the parser.";
    const EARLIER: &str = "Rebuilt the lexer.";
    let t = Timing::default();
    let p = land();
    let (mut before, mut at) = begun(&p, &t, None);
    while before.held_by().is_none() {
        at += QUICK;
        before.observe_turn_end(
            TurnEnd {
                earlier: Some(EARLIER.to_string()),
                ..ours(at, SAID, QUICK)
            },
            &t,
        );
        at = act(&mut before, &p, &t, at, None).1;
    }
    let point = before
        .held_point()
        .cloned()
        .unwrap_or_else(|| panic!("held: {before:?}"));
    assert_eq!(
        (point.text.as_deref(), point.earlier.as_deref()),
        (Some(SAID), Some(EARLIER))
    );
    let event = |value: serde_json::Value| {
        codex_history::RolloutItem::EventMsg(
            serde_json::from_value::<codex_protocol::protocol::EventMsg>(value)
                .unwrap_or_else(|err| panic!("{err}")),
        )
    };
    let items = [
        event(
            json!({"type":"task_started", "turn_id":"t1", "model_context_window":258400,
            "collaboration_mode_kind":"default",
            "turn_attribution": {"turn_id": "t1", "turn_trigger": "user"}}),
        ),
        event(json!({"type":"task_complete", "turn_id":"t1", "last_agent_message":SAID})),
    ];
    let after = |earlier: Option<&str>| {
        let now = Instant::now();
        let mut s = ThreadState::default();
        s.seed_replies(before.replies, Some(now));
        if let Some(earlier) = earlier {
            s.seed_held_point(
                Some(SAID.to_string()),
                Some(earlier.to_string()),
                false,
                now,
            );
        }
        crate::extension::resume_bound(&mut s, &items, false, None, 0, now);
        (s, now)
    };
    let (mut s, now) = after(Some(EARLIER));
    assert!(s.point.is_some(), "stood at again");
    let (d, _) = act(&mut s, &p, &t, now, None);
    assert_eq!((d.act, d.held), (Act::Nothing, Some(HeldBy::SameReply)));
    s.park_off();
    let (d, _) = act(&mut s, &p, &t, now + MIN, None);
    assert!(typed(&d), "Off and back types: {d:?}");
    // NEGATIVE CONTROL: no held point seeded.
    let (mut s, now) = after(None);
    assert!(s.point.is_none());
    s.park_off();
    assert_eq!(act(&mut s, &p, &t, now + MIN, None).0.act, Act::Nothing);
    // NEGATIVE CONTROL: words not the newest kept reply's.
    let (s, _) = after(Some("Rebuilt the parser."));
    assert!(s.point.is_none(), "{:?}", s.point);
    // NEGATIVE CONTROLS: a history that moved past the held turn (a newer reply, a user message
    // superseding the turn, or a running turn) neither stands at the stale point nor keeps its
    // replies.
    let moved_items = [
        items[0].clone(),
        event(
            json!({"type":"task_complete", "turn_id":"t1", "last_agent_message":"Fixed the parser."}),
        ),
    ];
    let superseded_items = [
        items[0].clone(),
        items[1].clone(),
        event(json!({"type":"user_message", "message":"Try the other branch."})),
    ];
    let running_items = [
        items[0].clone(),
        items[1].clone(),
        event(
            json!({"type":"task_started", "turn_id":"t2", "model_context_window":258400,
            "collaboration_mode_kind":"default",
            "turn_attribution": {"turn_id": "t2", "turn_trigger": "user"}}),
        ),
    ];
    for past in [&moved_items[..], &superseded_items[..], &running_items[..]] {
        let now = Instant::now();
        let mut s = ThreadState::default();
        s.seed_replies(before.replies, Some(now));
        s.seed_held_point(
            Some(SAID.to_string()),
            Some(EARLIER.to_string()),
            false,
            now,
        );
        crate::extension::resume_bound(&mut s, past, false, None, 0, now);
        assert!(s.point.is_none(), "{past:?} -> {:?}", s.point);
        assert_eq!(s.held_by(), None, "{past:?} -> {:?}", s.replies);
    }
}

#[path = "walls_hold_tests.rs"]
mod walls_hold_tests;
