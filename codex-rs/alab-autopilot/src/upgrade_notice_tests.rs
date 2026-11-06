// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE LIVE UPGRADE'S NOTICE IN THE DECIDER (`upgrade_ask`, CONTRACT §5 row "live upgrade",
//! amended 2026-10-28 after the parity sweep, gap 27): a notice the runtime says is due
//! ([`ThreadState::upgrade_notice`]) is typed at once at a clean point, in place of what was
//! decided there, as aterm's host types its notice at an idle point before its loop decides;
//! never at a wall's point or one a person, a landing, a compaction or an owed act holds; and as
//! the host's turn it spends none of the hour's budget. Each with its negative control.

use pretty_assertions::assert_eq;

use super::*;

const NOTICE: &str = "[aterm harness] Codex /new-build is installed; this session runs 0.1.0. \
     Notice reference: ATERM-UPGRADE-READY-0badc0de.";

fn notice(d: &Decision) -> bool {
    typed(d).is_some_and(|(rule, _)| rule == words::RULE_UPGRADE_NOTICE)
}

/// A person's task, its turn ended clean at `now`, the notice due.
fn due(t: &Timing, now: Instant) -> ThreadState {
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), t);
    s.upgrade_notice = Some(NOTICE.to_string());
    s
}

#[test]
fn a_due_notice_goes_first_at_a_clean_point() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = due(&t, now);
    let d = decide(&s, &policy(), &t, now, NO_JITTER);
    assert_eq!(typed(&d), Some((words::RULE_UPGRADE_NOTICE, NOTICE)));
    assert_eq!(d.not_before, None);
    // NEGATIVE CONTROL: with none due the point is decided as ever.
    s.upgrade_notice = None;
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_CONTINUE, "keep going"))
    );
}

/// The notice goes in place of a wait the point was decided to (here the hour's cap), as
/// aterm's host types it before its loop's back-off.
#[test]
fn a_due_notice_goes_at_once_where_the_point_would_wait() {
    let t = Timing::default();
    let now = Instant::now();
    let capped = Policy {
        continue_per_hour: Some(1),
        ..policy()
    };
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "One.", 5 * MIN), &t);
    run(&mut s, &capped, &t, now);
    let later = now + 10 * MIN;
    s.observe_turn_end(ours(later, "Two.", 5 * MIN), &t);
    assert!(
        decide(&s, &capped, &t, later, NO_JITTER)
            .not_before
            .is_some_and(|at| at > later)
    );
    s.upgrade_notice = Some(NOTICE.to_string());
    let d = decide(&s, &capped, &t, later, NO_JITTER);
    assert!(notice(&d), "{d:?}");
    assert_eq!(d.not_before, None);
}

/// What holds a point, set on its state at its time.
type Hold = fn(&mut ThreadState, Instant);

/// NEVER AT A WALL OR WHERE SOMETHING ELSE HOLDS THE POINT: a wall's point, a stopped turn, a
/// point left to a person, a landing, a compaction and the act owed after it, the act the image
/// before a re-exec owed, and a session with no task type no notice.
#[test]
fn no_notice_at_a_wall_or_where_the_point_is_held() {
    let t = Timing::default();
    let now = Instant::now();
    let held: [(&str, Hold); 8] = [
        ("usage wall", |s, at| {
            s.observe_turn_end(
                walled(
                    at,
                    Wall::Usage {
                        reset_at: None,
                        reset_label: None,
                    },
                ),
                &Timing::default(),
            );
        }),
        ("login wall", |s, at| {
            s.observe_turn_end(walled(at, Wall::Login), &Timing::default());
        }),
        ("stopped", |s, at| {
            s.observe_turn_end(
                TurnEnd {
                    aborted: true,
                    ..ours(at, "Half done.", 5 * MIN)
                },
                &Timing::default(),
            );
        }),
        ("left", |s, _| s.left = true),
        ("landed", |s, _| s.landed = true),
        ("compacting", |s, _| s.compacting = true),
        ("owed", |s, at| {
            s.owed = Some(Owed {
                native_origin: None,
                rule: words::RULE_CONTINUE,
                text: "keep going".into(),
                mode: Mode::Cruise,
                not_before: at,
                repo_owed: None,
                resume_goal: false,
                landing: false,
            });
        }),
        ("no task", |s, _| s.task = false),
    ];
    for (what, hold) in held {
        let mut s = due(&t, now);
        hold(&mut s, now);
        s.upgrade_notice = Some(NOTICE.to_string());
        let d = decide(&s, &policy(), &t, now, NO_JITTER);
        assert!(!notice(&d), "{what}: {d:?}");
    }
}

/// THE NOTICE IS THE HOST'S TURN: typed, it is awaited by no reply's count and spends none of
/// the hour's budget, so the point after its answer is decided as the one before it would have
/// been. NEGATIVE CONTROL: a continuation typed in its place spends the budget.
#[test]
fn the_notice_spends_none_of_the_hours_budget() {
    let t = Timing::default();
    let now = Instant::now();
    let capped = Policy {
        continue_per_hour: Some(1),
        ..policy()
    };
    let answered = |noticed: bool| {
        let mut s = due(&t, now);
        if !noticed {
            s.upgrade_notice = None;
        }
        let d = decide(&s, &capped, &t, now, NO_JITTER);
        assert_eq!(notice(&d), noticed, "{d:?}");
        s.acted(&d.act, now, &t);
        assert_eq!(s.upgrade_notice, None);
        let later = now + 2 * MIN;
        s.observe_turn_end(ours(later, "Ready for the restart.", 5 * MIN), &t);
        decide(&s, &capped, &t, later, NO_JITTER)
    };
    let after_notice = answered(true);
    assert_eq!(after_notice.not_before, None, "{after_notice:?}");
    assert!(typed(&after_notice).is_some(), "{after_notice:?}");
    let after_continue = answered(false);
    assert!(
        after_continue
            .not_before
            .is_some_and(|at| at > now + 2 * MIN),
        "{after_continue:?}"
    );
}

/// NOR WHERE THE POINT'S OWN DECISION KEEPS IT (review of gap 27, finding 12): a compaction's
/// continuation (typed in its place, the act that clears `after_compact` would lose it), an
/// attention left to a person, and a Land yield's landing, which the repository's read decides:
/// the notice waits for that read instead of going in the probe's place. NEGATIVE CONTROLS: the
/// compaction's continuation answered, the question answered by the policy's words, and a
/// Cruise done claim's read type the notice.
#[test]
fn no_notice_over_a_compactions_continuation_an_attention_or_a_landing() {
    let t = Timing::default();
    let now = Instant::now();
    // A full context, compacted: its continuation is owed at the compaction's end.
    let mut s = due(&t, now);
    s.upgrade_notice = None;
    let d = run(&mut s, &policy(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_CONTINUE, "keep going")));
    s.observe_turn_end(walled(now + MIN, Wall::Context { auto_compact: true }), &t);
    assert_eq!(run(&mut s, &policy(), &t, now + MIN).act, Act::Compact);
    s.observe_turn_end(ours(now + 2 * MIN, "", Duration::from_secs(20)), &t);
    assert!(s.after_compact);
    s.upgrade_notice = Some(NOTICE.to_string());
    let d = decide(&s, &policy(), &t, now + 2 * MIN, NO_JITTER);
    assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_COMPACT));
    s.acted(&d.act, now + 2 * MIN, &t);
    s.observe_turn_end(ours(now + 3 * MIN, "Stage 2 is in.", 5 * MIN), &t);
    s.upgrade_notice = Some(NOTICE.to_string());
    assert!(notice(&decide(&s, &policy(), &t, now + 3 * MIN, NO_JITTER)));
    // A question left to a person (answer_questions off) is theirs to answer.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Should I rename the module?", 5 * MIN), &t);
    s.upgrade_notice = Some(NOTICE.to_string());
    let off = Policy {
        answer_questions: false,
        ..policy()
    };
    let d = decide(&s, &off, &t, now, NO_JITTER);
    assert!(
        matches!(
            d.act,
            Act::Attention {
                key: "question",
                ..
            }
        ),
        "{d:?}"
    );
    assert!(notice(&decide(&s, &policy(), &t, now, NO_JITTER)));
    // Land's yield lands once the repository reads clean: no notice before or over it.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Started on it.", 5 * MIN), &t);
    let d = run(&mut s, &land(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_LAND, words::LAND)));
    s.observe_turn_end(
        ours(
            now + 3 * MIN,
            "Nothing left: every item is merged and pushed to origin/main.",
            3 * MIN,
        ),
        &t,
    );
    s.upgrade_notice = Some(NOTICE.to_string());
    assert_eq!(
        decide(&s, &land(), &t, now + 3 * MIN, NO_JITTER).act,
        Act::Probe,
        "the read comes first"
    );
    assert_eq!(
        probed(&mut s, &land(), &t, now + 3 * MIN, None).act,
        Act::Landed
    );
    // Cruise's done claim: read first, then the notice in place of the defend prompt.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Everything is done.", 5 * MIN), &t);
    s.upgrade_notice = Some(NOTICE.to_string());
    assert_eq!(
        decide(&s, &policy(), &t, now, NO_JITTER).act,
        Act::Probe,
        "the read comes first"
    );
    assert!(notice(&probed(&mut s, &policy(), &t, now, None)));
}

/// THE NOTICE'S ANSWER COUNTS TOWARD NO RULING F STREAK (aterm's replies pass over the host's
/// turn, `act_in_flight` and `untaken`; review of gap 27, finding 11): the replies kept before
/// the notice stand after its answer. NEGATIVE CONTROL: the same answer to a continuation is
/// counted.
#[test]
fn the_notices_answer_leaves_the_reply_streak_standing() {
    let t = Timing::default();
    let now = Instant::now();
    let kept = crate::replies::Replies::of(&[crate::replies::Kept {
        mark: 7,
        answers: 2,
        walled: false,
        tries: 0,
    }]);
    let answered = |noticed: bool| {
        let mut s = due(&t, now);
        if !noticed {
            s.upgrade_notice = None;
        }
        s.replies = kept;
        let d = decide(&s, &policy(), &t, now, NO_JITTER);
        assert_eq!(notice(&d), noticed, "{d:?}");
        s.acted(&d.act, now, &t);
        s.observe_turn_end(ours(now + MIN, "Ready for the restart.", 5 * MIN), &t);
        s.replies
    };
    assert_eq!(answered(true), kept);
    assert_ne!(answered(false), kept);
}
