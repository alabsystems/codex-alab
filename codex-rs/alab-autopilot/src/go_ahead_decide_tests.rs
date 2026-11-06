// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE OWNER'S STANDING GO-AHEAD (go-ahead@v1; owner, 2026-10-29: "how dare you block on me for
//! go ahead! fix the cruise harness so that this cannot happen again"): a park on a person's
//! go-ahead for the agent's own reversible work gets it, under Cruise and Land, and nowhere else.
//! The decider's half of aterm's `turn_end_go_ahead_tests.rs` (aterm `b938f0e959`), cut to what
//! codex-alab's own events say, each rule with its control: the incident replayed, the
//! irreversible guard, the person's hour, answers off, the bound and its refusal, Land's order,
//! the menu bar (§15.6) and the texts.

use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;

use super::*;
use crate::decide::GoAheadVerdict;
use crate::decide::GoAheads;
use crate::decide::Needs;
use crate::decide::Owed;
use crate::decide::hold_told;
use crate::go_ahead::GO_AHEAD_ACTS;
use crate::go_ahead::GO_AHEAD_ANSWER;
use crate::go_ahead::GO_AHEAD_DROP;
use crate::go_ahead::GO_AHEAD_HELD;
use crate::go_ahead::GO_AHEAD_OTHERS;
use crate::go_ahead::GO_AHEAD_ROOM;
use crate::go_ahead::GO_AHEAD_SPAN;
use crate::go_ahead::GO_AHEAD_WAIT;
use crate::go_ahead::GO_AHEAD_WHY;
use crate::go_ahead::NEVER_A_NEED;
use crate::go_ahead::RULE_GO_AHEAD;
use crate::go_ahead::TOKEN_CLOSE;
use crate::go_ahead::TOKEN_OPEN;
use crate::replies::HeldBy;
use crate::replies::Told;
use crate::words::Said;

/// The incident's last reply (2026-10-29), two days into the park.
const INCIDENT: &str = "Nothing has changed. The build is waiting only for your go-ahead.\n\nNEEDS \
                        YOU: say \"rerun the checks\" so I can build and test the upgrade fix \
                        branch, push it to main and remove the worktree.";

/// The incident's park reworded, as its agent wrote each one: the same word to say, other words
/// around it, so no same reply's hold forms.
const REWORDED: &str = "No change since the last check: the build still waits for your \
                        go-ahead.\n\nNEEDS YOU: say \"rerun the checks\" and I'll build and test \
                        the upgrade fix branch, push it to main and remove the worktree.";

/// The incident's memory guard: the verify stopped for low memory, parked on a rerun.
const STOPPED: &str = "The full verify on the merged branch was stopped for low memory, and I'm \
                       not restarting it on my own.\n\nNEEDS YOU: say \"rerun the checks\" when \
                       the Mac has memory to spare.";

const SECS: Duration = Duration::from_secs(1);

/// The go-ahead's words, where `d` types them.
fn go_ahead_text(d: &Decision) -> &str {
    match &d.act {
        Act::Type { rule, text } if *rule == RULE_GO_AHEAD => text,
        other => panic!("the owner's standing go-ahead, not {other:?} ({})", d.why),
    }
}

fn is_go_ahead(d: &Decision) -> bool {
    matches!(&d.act, Act::Type { rule, .. } if *rule == RULE_GO_AHEAD)
}

/// `REVERSIBLE_ANSWER`, whatever clauses follow it.
fn answers_reversibly(d: &Decision) -> bool {
    matches!(&d.act, Act::Type { rule, text } if *rule == words::RULE_ANSWER && text.starts_with(words::REVERSIBLE_ANSWER))
}

/// A thread whose person's turn opened the task an hour before `now` (no key of theirs since).
fn tasked(t: &Timing, now: Instant) -> ThreadState {
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now - 60 * MIN, "On it.", 5 * MIN), t);
    s
}

/// An agent's own turn nobody asked (a goal's continuation, the agent's own wake), said `text`
/// in `worked` with no tool run.
fn own_wake(at: Instant, text: &str, worked: Duration) -> TurnEnd {
    TurnEnd {
        by_autopilot: false,
        by_person: false,
        ..end(at, text, worked)
    }
}

/// The decision at the point as the runtime makes it: the repository probe answered clean, then
/// what it does with the go-ahead's lines judged ([`ThreadState::go_ahead_judge`]).
fn judged(s: &mut ThreadState, p: &Policy, t: &Timing, now: Instant) -> Decision {
    let d = probed(s, p, t, now, None);
    s.go_ahead_judge(&d, p, now);
    d
}

/// Decides at the point until an act goes or nothing is owed, waiting out each wait (a back-off,
/// the person's hour), with the person's key at `key` read before each decision; the act is
/// carried out. Returns the decision and when it was made.
fn until_acted_keyed(
    s: &mut ThreadState,
    p: &Policy,
    t: &Timing,
    mut now: Instant,
    key: Option<Instant>,
) -> (Decision, Instant) {
    for _ in 0..64 {
        if let Some(key) = key {
            s.person_keyed(now.saturating_duration_since(key), now);
        }
        let d = judged(s, p, t, now);
        if d.waits(now) {
            now = d.not_before.unwrap_or(now);
            continue;
        }
        if d.act == Act::Nothing
            && let Some(at) = d.look_again.filter(|at| *at > now)
        {
            now = at;
            continue;
        }
        s.acted(&d.act, now, t);
        return (d, now);
    }
    panic!("never settled");
}

fn until_acted(s: &mut ThreadState, p: &Policy, t: &Timing, now: Instant) -> (Decision, Instant) {
    until_acted_keyed(s, p, t, now, None)
}

/// Cruise with aterm's own continuation, which says the go-ahead is no need.
fn cruise_never_a_need() -> Policy {
    Policy {
        continue_text: Some(format!("Keep going. {NEVER_A_NEED}")),
        ..policy()
    }
}

/// THE INCIDENT'S REPLY GETS THE OWNER'S STANDING GO-AHEAD WITH ITS WORD, at once, and the line
/// it answers leaves the menu bar (§15.6): no ack, since no need is left on screen. On
/// origin/main it got `answer@v1` (`Nobody is here to answer…`) and the line stood.
#[test]
fn the_incident_reply_gets_the_owners_standing_go_ahead_with_its_word() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = tasked(&t, now);
    s.observe_turn_end(ours(now, INCIDENT, 3 * MIN), &t);
    assert_eq!(
        s.needs.shown().len(),
        1,
        "the line stands before the decision"
    );
    let d = judged(&mut s, &policy(), &t, now);
    let text = go_ahead_text(&d);
    assert_eq!(
        text,
        format!("{GO_AHEAD_ANSWER} {TOKEN_OPEN}rerun the checks{TOKEN_CLOSE} {GO_AHEAD_DROP}")
    );
    assert!(!text.contains(GO_AHEAD_ROOM) && !text.contains(GO_AHEAD_OTHERS));
    assert!(
        text.encode_utf16().count() <= words::VERBATIM_UNITS,
        "{text}"
    );
    assert!(
        s.needs.shown().is_empty(),
        "the answered line is off the bar"
    );
    assert_eq!(s.needs.standing.len(), 1, "it still stands for the decider");
    s.acted(&d.act, now, &t);
    assert!(s.needs.shown().is_empty(), "and stays off once typed");
    // A park on a NEEDS YOU line alone, or in an earlier message of the turn, gets it too.
    let need = "NEEDS YOU: say \"rerun the checks\" when the Mac has memory to spare. I'll run the \
                full verify on the tip, then push to main and clean up the worktree and branch.";
    let mut s = tasked(&t, now);
    s.observe_turn_end(ours(now, need, 3 * MIN), &t);
    let text = go_ahead_text(&judged(&mut s, &policy(), &t, now)).to_string();
    assert!(
        text.contains(GO_AHEAD_ROOM) && text.contains(GO_AHEAD_DROP),
        "{text}"
    );
    let mut s = tasked(&t, now);
    s.observe_turn_end(
        TurnEnd {
            earlier: Some(need.to_string()),
            ..ours(now, "Fetched main again; nothing moved.", 3 * MIN)
        },
        &t,
    );
    assert!(is_go_ahead(&judged(&mut s, &policy(), &t, now)));
}

/// THE INCIDENT REPLAYED (2026-10-26 to 2026-10-29): two days of the same park, reworded each
/// time, beside the memory guard's stops and the agent's own wakes. Each park gets the go-ahead
/// within [`GO_AHEAD_WAIT`] of its point, and the line it answers is never on the menu bar while
/// it is answered; an hour since the first go-ahead with no real work since, or three acts on one
/// standing park, is the bound, after which nothing is typed, the person is told now
/// ([`Told::GoAhead`]) and the line is back on the bar; real work restarts the hour, never the
/// acts. A reply that
/// parks no more (the agent worked and landed) starts the count again, so the next day's park is
/// answered as the first was.
#[test]
fn the_incident_replayed_over_two_days_gets_the_go_ahead_within_its_wait() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let mut s = tasked(&t, start);
    // A day of short quiet turns behind it, as the incident's tab had: the ladder is at its top.
    let mut now = start;
    for i in 0..8 {
        now += 5 * SECS;
        s.observe_turn_end(
            ours(
                now,
                &format!("Checked the queue ({i}): the build still waits its turn."),
                5 * SECS,
            ),
            &t,
        );
        (_, now) = until_acted(&mut s, &p, &t, now);
    }
    // Each day: a stop, the agent's own wakes on reworded parks, a person-free park after work.
    let replies: [(&str, bool, Duration); 6] = [
        (STOPPED, true, 3 * MIN),
        (INCIDENT, false, 30 * SECS),
        (REWORDED, false, 30 * SECS),
        (INCIDENT, false, 30 * SECS),
        (REWORDED, false, 30 * SECS),
        (STOPPED, true, 3 * MIN),
    ];
    let mut episodes = 0;
    for day in 0..2 {
        let mut acts = 0;
        let mut refused = false;
        for (k, (said, tooled_turn, worked)) in replies.iter().enumerate() {
            now += 59 * MIN;
            let point = now;
            let end = own_wake(point, said, *worked);
            let end = if *tooled_turn { tooled(end) } else { end };
            s.observe_turn_end(end, &t);
            let (d, at) = until_acted(&mut s, &p, &t, point);
            now = at;
            if is_go_ahead(&d) {
                acts += 1;
                assert!(acts <= GO_AHEAD_ACTS, "day {day} reply {k}: past the bound");
                assert!(
                    at <= point + GO_AHEAD_WAIT,
                    "day {day} reply {k}: the go-ahead waited {:?}",
                    at - point
                );
                assert!(
                    s.needs.shown().is_empty(),
                    "day {day} reply {k}: line shown"
                );
            } else {
                // The bound: its acts, or an hour since the first with no real work since.
                assert!(
                    acts > 0,
                    "day {day} reply {k}: no go-ahead before the bound: {d:?}"
                );
                assert_eq!(d.act, Act::Nothing, "day {day} reply {k}");
                // Told now as the go-ahead's refusal: spent, or held by the same reply.
                let told = match d.held {
                    Some(arm) => hold_told(&s, &p, arm, at),
                    None if d.refused => Told::GoAhead { arm: None },
                    None => panic!("day {day} reply {k}: neither refused nor held: {d:?}"),
                };
                assert!(
                    matches!(told, Told::GoAhead { .. }),
                    "day {day} reply {k}: {told:?}"
                );
                assert_eq!(
                    s.needs.shown(),
                    s.needs.standing,
                    "day {day} reply {k}: the line is back on the bar"
                );
                refused = true;
            }
        }
        assert!(refused, "day {day}: the bound came");
        // A stop after real work restarted the span; its acts stood: the day's go-aheads are
        // bounded all the same.
        assert_eq!(acts, GO_AHEAD_ACTS, "day {day}");
        episodes += 1;
        // The agent worked, landed and parked no more: the count starts again.
        now += 30 * MIN;
        s.observe_turn_end(
            tooled(own_wake(
                now,
                "Rebuilt, tested and pushed to main; the worktree is gone.",
                20 * MIN,
            )),
            &t,
        );
        (_, now) = until_acted(&mut s, &p, &t, now);
    }
    assert_eq!(episodes, 2);
}

/// THE AGENT'S OWN WAKES STARVE NO GO-AHEAD (aterm's review 2): the incident's first twenty
/// hours, each wake a new point that started the short-turn ladder's 60-minute wait again. The
/// park's wait runs from the last act typed, at most [`GO_AHEAD_WAIT`].
#[test]
fn the_agents_own_wakes_starve_no_go_ahead() {
    let t = Timing::default();
    let p = policy();
    let mut now = Instant::now();
    let mut s = tasked(&t, now);
    for k in 0..6 {
        now += 59 * MIN;
        s.observe_turn_end(
            own_wake(
                now,
                &format!("Checked again ({k}); nothing moved."),
                30 * SECS,
            ),
            &t,
        );
        let d = judged(&mut s, &p, &t, now);
        if !d.waits(now) {
            s.acted(&d.act, now, &t);
        }
    }
    now += 59 * MIN;
    s.observe_turn_end(
        own_wake(
            now,
            "The build is waiting only for your go-ahead (wake 7).",
            30 * SECS,
        ),
        &t,
    );
    let (d, then) = until_acted(&mut s, &p, &t, now);
    assert!(
        is_go_ahead(&d) && then <= now + GO_AHEAD_WAIT,
        "{d:?} at +{:?}",
        then - now
    );
    let next = now + 59 * MIN;
    s.observe_turn_end(
        own_wake(
            next,
            "The build is waiting only for your go-ahead (wake 8).",
            30 * SECS,
        ),
        &t,
    );
    let d = judged(&mut s, &p, &t, next);
    assert!(is_go_ahead(&d) && !d.waits(next), "{d:?}");
}

/// THE IRREVERSIBLE NEGATIVE CONTROL: an ask to delete, force-push, release or publish is never
/// answered with the go-ahead, whatever marker it parks on; it keeps today's act (the
/// continuation and the ack over a need, `REVERSIBLE_ANSWER` over an irreversible ask in prose)
/// and its line stays on the menu bar. `push origin +main` beside a stop phrase is an
/// irreversible ask (aterm's `force_push`, a6a1e0d67a); on origin/main it read as a plain stop.
#[test]
fn an_irreversible_ask_is_never_answered_with_the_go_ahead() {
    let t = Timing::default();
    let now = Instant::now();
    let p = policy();
    for need in [
        "NEEDS YOU: say go so I can delete the stale release tags on the remote.",
        "NEEDS YOU: say go so I can cut the v2.0 release and publish it to crates.io.",
        "NEEDS YOU: say \"go\" so I can force-push my branch over main.",
        "NEEDS YOU: say go so I can reset main to the tag and push it.",
    ] {
        let mut s = tasked(&t, now);
        s.observe_turn_end(ours(now, need, 3 * MIN), &t);
        let d = judged(&mut s, &p, &t, now);
        assert!(!is_go_ahead(&d), "{need}: {d:?}");
        assert!(typed(&d).is_some(), "it never blocks: {need}: {d:?}");
        assert_eq!(s.needs.shown(), s.needs.standing, "{need}: the line stands");
        assert!(!s.needs.shown().is_empty(), "{need}");
    }
    for prose in [
        "No force-push and nothing published. I'm still waiting for your go-ahead before \
         rebuilding.",
        "Should I force-push the old branch to the fork? Separately, the docs are fine. The \
         build is waiting for your go-ahead.",
        "The branch is rebased. I'm waiting for your go-ahead to run git push origin +main.",
        "I'll wait for your go-ahead before deleting the old release tags.",
    ] {
        let mut s = tasked(&t, now);
        s.observe_turn_end(ours(now, prose, 3 * MIN), &t);
        let d = judged(&mut s, &p, &t, now);
        assert!(answers_reversibly(&d), "{prose}: {d:?}");
    }
    assert!(matches!(
        words::classify_said(Some(
            "I'm waiting for your go-ahead to run git push origin +main."
        )),
        Said::Irreversible(_)
    ));
}

/// EVERY PARK ROW AT A CRUISE TURN END (aterm's review 2 and 3): each gets the owner's standing
/// go-ahead, except the rows [`IRREVERSIBLE_PARKS`] names, whose prose is an irreversible ask
/// and keeps `REVERSIBLE_ANSWER`, as written and as each wrap of [`DRAWN`] breaks its lines.
#[test]
fn every_park_row_gets_the_go_ahead_or_the_reversible_answer() {
    use crate::go_ahead::tests::DRAWN;
    use crate::go_ahead::tests::Row;
    use crate::go_ahead::tests::drawn;
    use crate::go_ahead::tests::rendered;
    use crate::go_ahead::tests::table;
    const IRREVERSIBLE_PARKS: &[&str] = &[
        "No force-push and nothing published. I'm still waiting for your go-ahead before \
         rebuilding.",
    ];
    let t = Timing::default();
    let now = Instant::now();
    let mut wrong = Vec::new();
    let mut rows = 0;
    for (row, reply) in table() {
        if row != Row::Park {
            continue;
        }
        rows += 1;
        for (columns, bare) in DRAWN.into_iter().flat_map(|c| [(c, false), (c, true)]) {
            let said = if bare {
                rendered(&reply)
            } else {
                reply.clone()
            };
            let mut s = tasked(&t, now);
            s.observe_turn_end(ours(now, &drawn(&said, columns), 3 * MIN), &t);
            let (d, _) = until_acted(&mut s, &policy(), &t, now);
            let kept = if IRREVERSIBLE_PARKS.contains(&reply.as_str()) {
                answers_reversibly(&d)
            } else {
                is_go_ahead(&d)
            };
            if !kept {
                wrong.push(format!("{reply} at {columns:?}, bare {bare}: {d:?}"));
            }
        }
    }
    assert!(rows >= 10, "{rows} park rows");
    assert!(
        wrong.is_empty(),
        "{}\n{} wrong",
        wrong.join("\n"),
        wrong.len()
    );
}

/// A reading's strength as the guard acts on it: an irreversible ask over a stop over a question
/// over nothing asked (aterm's `guard_strength`).
fn guard_strength(said: &Said) -> u8 {
    match said {
        Said::Irreversible(_) => 3,
        Said::Stop(_) => 2,
        Said::Question => 1,
        Said::Offer | Said::Plain => 0,
    }
}

/// THE GUARD READS EVERY REPLY AT LEAST AS THE BASE DID (aterm's reviews 4 to 6): codex-alab's
/// reading of each text of the go-ahead's corpus is at least as strong as the one aterm's guard
/// made of it at 246d1ddc2f (`fixtures/go-ahead-guard-base.txt`), which codex-alab's words and
/// needs readers port; where it reads an irreversible ask the reply gets `REVERSIBLE_ANSWER`, or
/// the go-ahead only where the ask rests on the agent's own cleanup on rows a parking need took
/// ([`crate::go_ahead::go_ahead_answers`]); where it reads any ask, answers off tell it.
#[test]
fn the_guard_reads_every_reply_as_the_base_did() {
    use crate::go_ahead::go_ahead_answers;
    use crate::go_ahead::tests::guard_base;
    use crate::go_ahead::tests::guard_corpus;
    let t = Timing::default();
    let now = Instant::now();
    let off = Policy {
        answer_questions: false,
        ..policy()
    };
    let base = guard_base();
    let mut wrong = Vec::new();
    let (mut irreversible, mut asks) = (0, 0);
    for text in guard_corpus() {
        let Some((_, base_said)) = base.get(&text) else {
            wrong.push(format!("no base record: {text:?}"));
            continue;
        };
        let read = words::classify_said(Some(&text));
        if guard_strength(&read) < guard_strength(base_said) {
            wrong.push(format!(
                "{read:?} weaker than the base's {base_said:?}: {text:?}"
            ));
        }
        if guard_strength(&read) == 0 {
            continue;
        }
        asks += 1;
        let mut s = tasked(&t, now);
        s.observe_turn_end(ours(now, &text, 3 * MIN), &t);
        if !matches!(judged(&mut s, &off, &t, now).act, Act::Attention { .. }) {
            wrong.push(format!("not told with answers off: {text:?}"));
        }
        if !matches!(read, Said::Irreversible(_)) {
            continue;
        }
        irreversible += 1;
        let mut s = tasked(&t, now);
        s.observe_turn_end(ours(now, &text, 3 * MIN), &t);
        let d = judged(&mut s, &policy(), &t, now);
        if !(answers_reversibly(&d) || (is_go_ahead(&d) && go_ahead_answers(Some(&text)))) {
            wrong.push(format!("{d:?}: {text:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{}\n{} wrong",
        wrong.join("\n"),
        wrong.len()
    );
    assert!(
        asks > irreversible && irreversible > 0,
        "{asks} {irreversible}"
    );
}

/// THE PERSON'S HOUR (aterm's review 5 to 9): their last typed key holds the go-ahead for
/// [`GO_AHEAD_HELD`] from the end of the turn it came in (they may just have said to hold),
/// Cruise's continuation then leaves out the sentence that says the go-ahead is no need, and the
/// point is decided again at the hour's end, where the go-ahead goes though the parked agent
/// ends no other turn. NEGATIVE CONTROL: with no key the same park gets it at once.
#[test]
fn a_typed_key_holds_the_go_ahead_for_the_hour_then_it_goes_at_its_end() {
    let t = Timing::default();
    let p = cruise_never_a_need();
    let start = Instant::now();
    let need = "NEEDS YOU: say \"rerun the checks\" so I can build and test my branch and push it \
                to main.";
    // The person's turn, their key 8 s before its end, ends on the park.
    let key = start - 8 * SECS;
    let mut s = ThreadState::default();
    s.observe_turn_end(end(start, need, 5 * MIN), &t);
    s.person_keyed(start.saturating_duration_since(key), start);
    let d = judged(&mut s, &p, &t, start);
    assert!(!is_go_ahead(&d), "within the hour: {d:?}");
    let (rule, text) = typed(&d).expect("today's act: it never blocks");
    assert_eq!(rule, words::RULE_CONTINUE);
    assert!(!text.contains(NEVER_A_NEED), "cut within the hour: {text}");
    assert_eq!(s.needs.shown(), s.needs.standing, "the line stands");
    s.acted(&d.act, start, &t);
    // The agent answers the same, quietly, until the same reply holds; the hold waits for the
    // hour's end, where the point is decided again and the go-ahead goes.
    let hour = start + GO_AHEAD_HELD;
    let mut now = start;
    let mut held = None;
    for _ in 0..8 {
        now += 5 * MIN;
        s.observe_turn_end(ours(now, need, 5 * SECS), &t);
        s.person_keyed(now.saturating_duration_since(key), now);
        let mut d = judged(&mut s, &p, &t, now);
        while d.waits(now) && d.not_before.is_some_and(|at| at < hour) {
            now = d.not_before.unwrap_or(now);
            s.person_keyed(now.saturating_duration_since(key), now);
            d = judged(&mut s, &p, &t, now);
        }
        assert!(!is_go_ahead(&d), "within the hour: {d:?}");
        if d.act == Act::Nothing {
            held = Some(d);
            break;
        }
        if !d.waits(now) {
            s.acted(&d.act, now, &t);
        }
    }
    let held = held.expect("the same reply holds within the hour");
    assert_eq!(
        held.look_again,
        Some(hour),
        "decided again at the hour's end"
    );
    s.person_keyed(hour.saturating_duration_since(key), hour);
    let d = judged(&mut s, &p, &t, hour);
    assert!(is_go_ahead(&d), "at the hour's end: {d:?}");
    assert!(s.needs.shown().is_empty());
    // The continuation says it whole outside the hour.
    let mut s = tasked(&t, start);
    s.observe_turn_end(ours(start, "Stage 2 is in.", 5 * MIN), &t);
    let d = judged(&mut s, &p, &t, start);
    let (_, text) = typed(&d).expect("continued");
    assert!(text.contains(NEVER_A_NEED), "{text}");
    // NEGATIVE CONTROL: no key, the same park gets the go-ahead at once.
    let mut s = tasked(&t, start);
    s.observe_turn_end(ours(start, need, 5 * MIN), &t);
    assert!(is_go_ahead(&judged(&mut s, &p, &t, start)));
}

/// A KEY IN A LONG TURN HOLDS FROM THE TURN'S END, a key read again within [`KEY_DRIFT`] is the
/// same key, and a new key of the person's starts the go-ahead's count again.
#[test]
fn a_key_in_a_long_turn_holds_from_its_end_and_a_new_key_starts_the_count_again() {
    use crate::go_ahead::KEY_DRIFT;
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    // A key forty minutes into a turn the person opened that worked fifty: the hour runs from
    // the turn's end, not the key.
    let key = start - 10 * MIN;
    let mut s = ThreadState::default();
    s.observe_turn_end(end(start, INCIDENT, 50 * MIN), &t);
    s.person_keyed(start.saturating_duration_since(key), start);
    let at = key + GO_AHEAD_HELD + MIN;
    s.observe_turn_end(ours(at, REWORDED, 5 * MIN), &t);
    s.person_keyed(at.saturating_duration_since(key), at);
    assert!(
        !is_go_ahead(&judged(&mut s, &p, &t, at)),
        "an hour from the key is not an hour from the turn's end"
    );
    // Three go-aheads spend the bound; the same key read again later (within the drift) resets
    // nothing, a new key does.
    let mut s = tasked(&t, start);
    let mut now = start;
    for n in 0..GO_AHEAD_ACTS {
        now += 20 * MIN;
        s.observe_turn_end(tooled(ours(now, INCIDENT, 3 * MIN)), &t);
        let (d, then) = until_acted(&mut s, &p, &t, now);
        assert!(is_go_ahead(&d), "act {n}: {d:?}");
        now = then;
    }
    now += 20 * MIN;
    s.observe_turn_end(tooled(ours(now, INCIDENT, 3 * MIN)), &t);
    let d = judged(&mut s, &p, &t, now);
    assert!(d.refused, "{d:?}");
    // A key typed over an hour ago (it holds nothing now) changes nothing.
    s.person_keyed(2 * GO_AHEAD_HELD, now);
    assert!(judged(&mut s, &p, &t, now).refused);
    // A fresh key holds the hour and starts the count again: past the hour, the go-ahead.
    s.person_keyed(5 * SECS, now);
    let typed_key = now - 5 * SECS;
    let later = now + GO_AHEAD_HELD + MIN;
    s.observe_turn_end(tooled(ours(later, INCIDENT, 3 * MIN)), &t);
    s.person_keyed(
        later.saturating_duration_since(typed_key) + KEY_DRIFT / 2,
        later,
    );
    let (d, _) = until_acted(&mut s, &p, &t, later);
    assert!(is_go_ahead(&d), "{d:?}");
}

/// THE PERSON'S HOUR AND THE GO-AHEAD'S COUNT OUTLIVE A RE-EXEC (review of the port, findings 3
/// and 9; aterm's `an_hour_from_a_turns_end_is_booked_once_and_seeded_after_a_restart`): the
/// person types at T inside a turn they opened that runs fifty minutes, so their hour runs to
/// T+110; a live upgrade re-execs at T+55, and the new image, whose restored point has no turn's
/// work for the key to have come in, reads the key as a key alone. With the hour the record
/// carried seeded, the park at T+61 gets no go-ahead, and at T+110 it does. And a held park that
/// had its go-ahead before the re-exec gets none through the same reply's hold after it, with the
/// count the record carried seeded. NEGATIVE CONTROLS: unseeded, the hour runs from the key and
/// the go-ahead goes at T+61, and the count starts over and lets the park through again.
#[test]
fn the_persons_hour_and_the_go_ahead_count_outlive_a_re_exec() {
    let t = Timing::default();
    let p = policy();
    let key = Instant::now();
    let ended = key + 50 * MIN;
    let mut s = ThreadState::default();
    s.observe_turn_end(end(ended, INCIDENT, 50 * MIN), &t);
    s.person_keyed(ended.saturating_duration_since(key), ended);
    let exec = key + 55 * MIN;
    let hour = s.go_ahead_hour_carried(exec);
    assert_eq!(hour, Some(ended), "the hour runs from the turn's end");
    for seeded in [true, false] {
        let mut image = tasked(&t, key);
        image.observe_turn_end(
            TurnEnd {
                restored: true,
                ..ours(exec, INCIDENT, Duration::ZERO)
            },
            &t,
        );
        if seeded && let Some(from) = hour {
            image.seed_go_ahead_hour(from, exec);
        }
        let at = key + 61 * MIN;
        image.person_keyed(at.saturating_duration_since(key), at);
        let d = judged(&mut image, &p, &t, at);
        assert_eq!(is_go_ahead(&d), !seeded, "seeded {seeded}, at T+61: {d:?}");
        if seeded {
            let at = ended + GO_AHEAD_HELD;
            image.person_keyed(at.saturating_duration_since(key), at);
            assert!(is_go_ahead(&judged(&mut image, &p, &t, at)), "at T+110");
        }
    }
    // THE COUNT: the go-ahead on the park, then the same quiet reply until it holds.
    let start = key + 3 * GO_AHEAD_HELD;
    let mut s = tasked(&t, start);
    let mut at = start;
    let held = loop {
        at += 5 * SECS;
        s.observe_turn_end(ours(at, INCIDENT, 5 * SECS), &t);
        let (d, then) = until_acted(&mut s, &p, &t, at);
        at = then;
        if d.act == Act::Nothing {
            break d;
        }
        assert!(is_go_ahead(&d), "{d:?}");
    };
    assert!(held.held.is_some(), "the same reply holds: {held:?}");
    let carried = s.go_aheads_carried().expect("the go-aheads counted");
    assert!(carried.acts >= 1, "{carried:?}");
    for seeded in [true, false] {
        // The new image's memory: the replies and the held point the record carried, no count.
        let mut image = ThreadState {
            go_aheads: GoAheads::default(),
            ..s.clone()
        };
        if seeded {
            image.seed_go_aheads(carried);
        }
        let d = judged(&mut image, &p, &t, at + MIN);
        assert_eq!(
            is_go_ahead(&d),
            !seeded,
            "seeded {seeded}: the held park after the re-exec: {d:?}"
        );
    }
}

/// A GO-AHEAD THE IMAGE BEFORE A RE-EXEC OWED IS STILL THE GO-AHEAD (review of the port, finding
/// 4): decided at T and waiting out its back-off, it crosses a live upgrade's re-exec at T+3 as
/// the record's owed act; the person types in the tab at T+5 without sending, so when it is due
/// it is not typed, and the point is decided again at the hour's end, where it goes. With answers
/// switched off since, it is told as the question it is, and once its bound is spent it is
/// refused. NEGATIVE CONTROL: with no key and answers on, it goes when due, as decided.
#[test]
fn an_owed_go_ahead_waits_out_the_persons_hour_answers_off_and_the_bound() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    let mut before = tasked(&t, start);
    before.observe_turn_end(ours(start, INCIDENT, 30 * SECS), &t);
    before.short_streak = 3;
    let d = judged(&mut before, &p, &t, start);
    assert!(
        is_go_ahead(&d) && d.waits(start),
        "a go-ahead backed off: {d:?}"
    );
    let due = d.not_before.unwrap_or(start);
    let owed = Owed {
        native_origin: None,
        rule: RULE_GO_AHEAD,
        text: go_ahead_text(&d).to_string(),
        mode: Mode::Cruise,
        not_before: due,
        repo_owed: None,
        resume_goal: false,
        landing: false,
    };
    let image = |owed: &Owed| {
        let mut s = ThreadState::default();
        s.observe_resume(true, Some(INCIDENT), false);
        s.observe_owed(Some(owed.clone()), false, start + 3 * MIN);
        s
    };
    // NEGATIVE CONTROL: no key since, answers on: typed at its time as decided.
    let mut s = image(&owed);
    let d = judged(&mut s, &p, &t, due);
    assert_eq!(
        typed(&d),
        Some((RULE_GO_AHEAD, owed.text.as_str())),
        "{d:?}"
    );
    // The person's key at T+5: not typed within their hour, decided again at its end.
    let key = start + 5 * MIN;
    let mut s = image(&owed);
    s.person_keyed(due.saturating_duration_since(key), due);
    let d = judged(&mut s, &p, &t, due);
    assert!(!is_go_ahead(&d), "within the person's hour: {d:?}");
    assert_eq!(d.look_again, Some(key + GO_AHEAD_HELD), "{d:?}");
    let at = key + GO_AHEAD_HELD;
    s.person_keyed(at.saturating_duration_since(key), at);
    assert!(
        is_go_ahead(&judged(&mut s, &p, &t, at)),
        "at the hour's end"
    );
    // Answers off since: told, not typed.
    let off = Policy {
        answer_questions: false,
        ..policy()
    };
    let mut s = image(&owed);
    let d = judged(&mut s, &off, &t, due);
    assert_eq!(
        d.act,
        Act::Attention {
            key: "question",
            text: format!("question: {GO_AHEAD_WHY}; answer_questions is off"),
        },
        "{d:?}"
    );
    // The bound spent: refused, nothing typed.
    let mut s = image(&owed);
    s.go_aheads = GoAheads {
        acts: GO_AHEAD_ACTS,
        first: Some(start),
    };
    let d = judged(&mut s, &p, &t, due);
    assert!(d.act == Act::Nothing && d.refused, "{d:?}");
}

/// A CONTROL WRITE NEVER HOLDS THE GO-AHEAD (aterm's review 7: the hour is timed from measured
/// keys, never from reading who opened a turn; aterm's `a_control_write_never_holds_the_go_ahead`
/// and the keyless control of `every_persons_turn_holds_the_go_ahead_for_the_hour_whatever_it_says`):
/// a turn opened by a message no key of the person's typed (a lead's `aterm ctl @tab send`, a
/// worker's task prompt: Codex calls every message not codex-alab's a person's, and the window
/// stamps no `human_typed_ms` for a control write) ends on a park, and the go-ahead goes at once,
/// as it does ten minutes on. The control: the same turn with their key typed in it holds the
/// hour, and so does the park ten minutes on.
#[test]
fn a_control_write_never_holds_the_go_ahead() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    let parked = "The build is waiting only for your go-ahead to rerun the checks.";
    for keyed in [false, true] {
        let key = keyed.then_some(start - 90 * SECS);
        let mut s = ThreadState::default();
        s.observe_turn_end(end(start, parked, 3 * MIN), &t);
        if let Some(key) = key {
            s.person_keyed(start.saturating_duration_since(key), start);
        }
        let d = judged(&mut s, &p, &t, start);
        assert_eq!(is_go_ahead(&d), !keyed, "keyed {keyed}: {d:?}");
        s.acted(&d.act, start, &t);
        // The park ten minutes on, no key since: the key's hour still holds it.
        let at = start + 10 * MIN;
        s.observe_turn_end(tooled(ours(at, parked, 3 * MIN)), &t);
        if let Some(key) = key {
            s.person_keyed(at.saturating_duration_since(key), at);
        }
        let d = judged(&mut s, &p, &t, at);
        assert_eq!(
            is_go_ahead(&d),
            !keyed,
            "keyed {keyed}, ten minutes on: {d:?}"
        );
    }
}

/// ANSWERS OFF: a park is told to the person as a question with the go-ahead's why, nothing is
/// typed, and its line stays on the menu bar; continue off with answers on still types it (it is
/// an answer); Off types nothing (the runtime parks it before any decision).
#[test]
fn answers_off_tell_a_park_and_continue_off_still_answers_it() {
    let t = Timing::default();
    let now = Instant::now();
    let off = Policy {
        answer_questions: false,
        ..policy()
    };
    let mut s = tasked(&t, now);
    s.observe_turn_end(ours(now, INCIDENT, 3 * MIN), &t);
    let d = judged(&mut s, &off, &t, now);
    assert_eq!(
        d.act,
        Act::Attention {
            key: "question",
            text: format!("question: {GO_AHEAD_WHY}; answer_questions is off"),
        }
    );
    assert_eq!(
        s.needs.shown(),
        s.needs.standing,
        "the line goes to the person"
    );
    // The NEEDS YOU line alone is told so too.
    let mut s = tasked(&t, now);
    s.observe_turn_end(
        ours(
            now,
            "NEEDS YOU: say \"rerun the checks\" so I can rebuild my branch.",
            3 * MIN,
        ),
        &t,
    );
    assert!(matches!(
        judged(&mut s, &off, &t, now).act,
        Act::Attention {
            key: "question",
            ..
        }
    ));
    // Continue off, answers on: the go-ahead is an answer, and goes.
    let quiet = Policy {
        continue_on: false,
        ..policy()
    };
    let mut s = tasked(&t, now);
    s.observe_turn_end(ours(now, INCIDENT, 3 * MIN), &t);
    assert!(is_go_ahead(&judged(&mut s, &quiet, &t, now)));
}

/// THE BOUND: three go-aheads on the standing park whatever word each reply asks for (a token,
/// another token, prose), each after real work so no same reply holds it; then nothing is typed,
/// the person is told now ([`Told::GoAhead`], an ask of codex-alab's own), and the line is back
/// on the bar. An hour since the first act (no real work since) is the bound too. A reply that
/// parks no more starts the count again.
#[test]
fn three_go_aheads_on_a_standing_park_then_the_refusal_is_told_now() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    let parks = [
        "NEEDS YOU: say \"rerun the checks\" so I can rebuild my branch.",
        "NEEDS YOU: say \"go\" so I can rebuild my branch and push it to main.",
        "The rebuild of my branch is waiting for your go-ahead.",
    ];
    let mut s = tasked(&t, start);
    let mut now = start;
    for (n, said) in parks.iter().enumerate() {
        now += 20 * MIN;
        s.observe_turn_end(tooled(ours(now, said, 3 * MIN)), &t);
        let (d, then) = until_acted(&mut s, &p, &t, now);
        assert!(is_go_ahead(&d), "act {n}: {d:?}");
        now = then;
    }
    now += 20 * MIN;
    s.observe_turn_end(tooled(ours(now, parks[0], 3 * MIN)), &t);
    let d = judged(&mut s, &p, &t, now);
    assert_eq!(d.act, Act::Nothing, "{d:?}");
    assert!(d.refused, "{d:?}");
    assert_eq!(
        s.needs.shown(),
        s.needs.standing,
        "the line is back on the bar"
    );
    let told = hold_told(&s, &p, HeldBy::SameReply, now);
    assert_eq!(
        told,
        Told::GoAhead {
            arm: Some(HeldBy::SameReply)
        }
    );
    let ask = Told::GoAhead { arm: None }
        .ask()
        .expect("told now, as an ask");
    assert!(ask.contains(crate::go_ahead::REFUSES_GO_AHEAD), "{ask}");
    assert!(ask.len() <= 200, "{ask}");
    // A reply that parks no more starts the count again.
    now += 20 * MIN;
    s.observe_turn_end(
        tooled(ours(now, "Rebuilt and pushed my branch to main.", 9 * MIN)),
        &t,
    );
    (_, now) = until_acted(&mut s, &p, &t, now);
    now += 20 * MIN;
    s.observe_turn_end(tooled(ours(now, parks[1], 3 * MIN)), &t);
    let (d, _) = until_acted(&mut s, &p, &t, now);
    assert!(is_go_ahead(&d), "{d:?}");
    // The span: a park standing an hour after its first go-ahead, no real work since.
    let mut s = tasked(&t, start);
    s.observe_turn_end(ours(start, INCIDENT, 3 * MIN), &t);
    let (d, first) = until_acted(&mut s, &p, &t, start);
    assert!(is_go_ahead(&d));
    let at_hour = first + GO_AHEAD_SPAN;
    s.observe_turn_end(ours(at_hour, REWORDED, 30 * SECS), &t);
    let d = judged(&mut s, &p, &t, at_hour);
    assert!(d.act == Act::Nothing && d.refused, "{d:?}");
}

/// THE FIRST GO-AHEAD ON A PARK PASSES THE SAME-REPLY STATE (aterm's review 8): the answers that
/// formed it were other acts' (here Cruise's forward-recovery continuation while the person's
/// hour held the park), and the go-ahead may be what the agent waits for. Current main recovers
/// an ordinary Cruise repeat instead of holding its act; the go-ahead still passes the repeated
/// state at the hour. The same reply after it is held and told as the go-ahead's refusal.
#[test]
fn the_first_go_ahead_on_a_park_passes_the_same_reply_hold() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    let hour = start + GO_AHEAD_HELD;
    // The person's own turn ends on the park, their key typed in it: their hour holds it from
    // its end.
    let key = start - 8 * SECS;
    let mut s = ThreadState::default();
    s.observe_turn_end(end(start, INCIDENT, 5 * MIN), &t);
    let mut now = start;
    let held = loop {
        s.person_keyed(now.saturating_duration_since(key), now);
        let d = judged(&mut s, &p, &t, now);
        if d.act == Act::Nothing {
            break d;
        }
        assert!(!is_go_ahead(&d), "within the hour: {d:?}");
        if d.waits(now) {
            now = d.not_before.unwrap_or(now);
            continue;
        }
        s.acted(&d.act, now, &t);
        now += 5 * SECS;
        s.observe_turn_end(ours(now, INCIDENT, 5 * SECS), &t);
    };
    assert_eq!(
        held.held, None,
        "Cruise changes approach before the hour: {held:?}"
    );
    assert_eq!(held.look_again, Some(hour));
    let point = s.point.clone().expect("the held point");
    assert!(s.repeats(&point), "the hold stands");
    s.person_keyed(hour.saturating_duration_since(key), hour);
    let d = judged(&mut s, &p, &t, hour);
    assert!(is_go_ahead(&d), "the first go-ahead passes the hold: {d:?}");
    s.acted(&d.act, hour, &t);
    // The same quiet reply after it: held, and told as the go-ahead's refusal, now.
    let at = hour + 5 * SECS;
    s.observe_turn_end(ours(at, INCIDENT, 5 * SECS), &t);
    let (d, at) = until_acted(&mut s, &p, &t, at);
    assert_eq!(d.act, Act::Nothing, "{d:?}");
    let arm = d.held.expect("held");
    assert_eq!(hold_told(&s, &p, arm, at), Told::GoAhead { arm: Some(arm) });
    // NEGATIVE CONTROL: with answers off the hold is told as the quiet stop.
    let off = Policy {
        answer_questions: false,
        ..policy()
    };
    assert_eq!(hold_told(&s, &off, arm, at), Told::Stop(arm));
}

/// LAND'S ORDER: Land's instruction first over a park (no back-off), then the go-ahead over
/// work by the park's own words (a branch, a worktree, main) or work git found; over a clean
/// folder a park that names no landing work keeps Land's answer. With answers off it is told.
#[test]
fn land_types_its_instruction_first_then_the_go_ahead_over_unlanded_work() {
    let t = Timing::default();
    let p = land();
    let now = Instant::now();
    let mut s = tasked(&t, now);
    s.observe_mode(Mode::Land);
    s.observe_turn_end(ours(now, INCIDENT, 3 * MIN), &t);
    let first = judged(&mut s, &p, &t, now);
    assert_eq!(
        typed(&first).map(|(rule, _)| rule),
        Some(words::RULE_LAND),
        "the instruction comes first: {first:?}"
    );
    assert!(
        s.needs.shown().is_empty(),
        "Land's instruction over the park answers its line"
    );
    s.acted(&first.act, now, &t);
    let at = now + 10 * MIN;
    s.observe_turn_end(ours(at, REWORDED, 3 * MIN), &t);
    let (d, _) = until_acted(&mut s, &p, &t, at);
    assert!(is_go_ahead(&d), "the park names its branch: {d:?}");
    // A park naming no landing work over a clean folder: Land's own answer.
    let bare = "Nothing has changed. The build is waiting only for your go-ahead.";
    let at = at + 20 * MIN;
    s.observe_turn_end(tooled(ours(at, bare, 3 * MIN)), &t);
    let (d, _) = until_acted(&mut s, &p, &t, at);
    assert!(!is_go_ahead(&d), "nothing unlanded: {d:?}");
    // Over work git found, it gets the go-ahead.
    let at = at + 20 * MIN;
    s.observe_turn_end(tooled(ours(at, bare, 3 * MIN)), &t);
    s.observe_mode(Mode::Land);
    let mut d = decide(&s, &p, &t, at, NO_JITTER);
    if d.act == Act::Probe {
        s.probed(Some(work()));
        d = decide(&s, &p, &t, at, NO_JITTER);
    }
    while d.waits(at) {
        let due = d.not_before.unwrap_or(at);
        d = decide(&s, &p, &t, due, NO_JITTER);
    }
    assert!(is_go_ahead(&d), "over work git found: {d:?}");
    // Answers off: told, as under Cruise.
    let off = Policy {
        answer_questions: false,
        ..land()
    };
    let mut s = tasked(&t, now);
    s.observe_mode(Mode::Land);
    s.observe_turn_end(ours(now, INCIDENT, 3 * MIN), &t);
    assert!(matches!(
        judged(&mut s, &off, &t, now).act,
        Act::Attention { .. }
    ));
}

/// [`judged`] over a repository the probe finds `found` in.
fn judged_over(
    s: &mut ThreadState,
    p: &Policy,
    t: &Timing,
    now: Instant,
    found: Option<RepoWork>,
) -> Decision {
    let d = probed(s, p, t, now, found);
    s.go_ahead_judge(&d, p, now);
    d
}

/// UNDER LAND A PARK THAT NAMES NO LANDING WORK IS READ OFF THE REPOSITORY AT EVERY POINT IT
/// PARKS (review of the port, finding 5; aterm's `repo_read`: `parks` probes every Land reading
/// that parks), whatever the instruction or the person's hour. Within the hour, the same reply's
/// hold is decided again at the hour's end over the work found, where the go-ahead goes; before
/// the instruction, the instruction goes first over the work found, and the go-ahead after it.
/// NEGATIVE CONTROL: over a clean folder the bare park before the instruction keeps Land's
/// answer.
#[test]
fn a_bare_park_under_land_is_read_off_the_repository_within_the_hour_and_before_the_instruction() {
    let t = Timing::default();
    let p = land();
    let start = Instant::now();
    let bare = "Nothing has changed. The build is waiting only for your go-ahead.";
    // (a) The instruction typed, then the person's key: the quiet park until the same reply
    // holds, decided again at the hour's end.
    let mut s = tasked(&t, start);
    s.observe_mode(Mode::Land);
    s.observe_turn_end(ours(start, "Stage 2 is in.", 5 * MIN), &t);
    let d = judged_over(&mut s, &p, &t, start, Some(work()));
    assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_LAND));
    s.acted(&d.act, start, &t);
    let key = start + MIN;
    let hour = key + GO_AHEAD_HELD;
    let mut now = start;
    let mut held = None;
    for _ in 0..8 {
        now += 2 * MIN;
        s.observe_turn_end(ours(now, bare, 5 * SECS), &t);
        s.person_keyed(now.saturating_duration_since(key), now);
        let mut d = judged_over(&mut s, &p, &t, now, Some(work()));
        while d.waits(now) && d.not_before.is_some_and(|at| at < hour) {
            now = d.not_before.unwrap_or(now);
            s.person_keyed(now.saturating_duration_since(key), now);
            d = judged_over(&mut s, &p, &t, now, Some(work()));
        }
        assert!(!is_go_ahead(&d), "within the hour: {d:?}");
        if d.act == Act::Nothing {
            held = Some(d);
            break;
        }
        s.acted(&d.act, now, &t);
    }
    let held = held.expect("the same reply holds within the hour");
    assert_eq!(
        held.look_again,
        Some(hour),
        "decided again at the hour's end: {held:?}"
    );
    s.person_keyed(hour.saturating_duration_since(key), hour);
    let d = judged_over(&mut s, &p, &t, hour, Some(work()));
    assert!(
        is_go_ahead(&d),
        "at the hour's end, over the work found: {d:?}"
    );
    // (b) Before the instruction: the instruction first, then the go-ahead.
    let mut s = tasked(&t, start);
    s.observe_mode(Mode::Land);
    s.observe_turn_end(ours(start, bare, 3 * MIN), &t);
    let d = judged_over(&mut s, &p, &t, start, Some(work()));
    assert_eq!(
        typed(&d).map(|(rule, _)| rule),
        Some(words::RULE_LAND),
        "the instruction first: {d:?}"
    );
    s.acted(&d.act, start, &t);
    let at = start + 10 * MIN;
    s.observe_turn_end(ours(at, bare, 3 * MIN), &t);
    let (d, _) = {
        let mut now = at;
        loop {
            let d = judged_over(&mut s, &p, &t, now, Some(work()));
            if !d.waits(now) {
                break (d, now);
            }
            now = d.not_before.unwrap_or(now);
        }
    };
    assert!(is_go_ahead(&d), "then the go-ahead: {d:?}");
    // NEGATIVE CONTROL: a clean folder keeps Land's answer before the instruction.
    let mut s = tasked(&t, start);
    s.observe_mode(Mode::Land);
    s.observe_turn_end(ours(start, bare, 3 * MIN), &t);
    let d = judged_over(&mut s, &p, &t, start, None);
    assert_eq!(
        typed(&d).map(|(rule, _)| rule),
        Some(words::RULE_LAND_ANSWER),
        "{d:?}"
    );
}

/// Cruise with aterm's own continuation and done check, each saying the go-ahead is no need.
fn cruise_checked() -> Policy {
    let mut p = cruise_never_a_need();
    p.texts.done_check = format!("{} {NEVER_A_NEED}", words::DONE_CHECK);
    p
}

/// Whether the act's words say [`NEVER_A_NEED`].
fn says_no_need(d: &Decision) -> bool {
    match &d.act {
        Act::Type { text, .. } | Act::ResumeLimit { text } => text.contains(NEVER_A_NEED),
        other => panic!("words typed: {other:?} ({})", d.why),
    }
}

/// EVERY CONTINUATION AND CHECK IS CUT WITHIN THE PERSON'S HOUR, wherever it is typed (review of
/// the port, findings 6 and 9; aterm's `continue_as`, `compact_owed_text` and the relaunch's
/// `carried_text`): the compaction's continuation, the check a full context cut typed again and
/// the carry-on after aterm's relaunch (its continuation and its check) each leave
/// [`NEVER_A_NEED`] out while the person's key holds the go-ahead, and say it with no key. The
/// usage limit's resume types its one cleared line (aterm `77161d6432`), which says neither the
/// continuation nor the sentence, key or none (it was cut as the continuation it typed before).
#[test]
fn every_continuation_and_check_is_cut_within_the_persons_hour() {
    let t = Timing::default();
    let p = cruise_checked();
    let start = Instant::now();
    let context = || Wall::Context { auto_compact: true };
    let keyed = |s: &mut ThreadState, key: Option<Instant>, now: Instant| {
        if let Some(key) = key {
            s.person_keyed(now.saturating_duration_since(key), now);
        }
    };
    // A compaction after `first`, the person's key (if any) read once it ends.
    let compacted = |s: &mut ThreadState, key: Option<Instant>| {
        let at = start + 2 * MIN;
        s.observe_turn_end(walled(at, context()), &t);
        assert_eq!(run(s, &p, &t, at).act, Act::Compact);
        let at = at + MIN;
        s.observe_turn_end(ours(at, "", 20 * SECS), &t);
        keyed(s, key, at);
        let d = probed(s, &p, &t, at, None);
        assert_eq!(
            typed(&d).map(|(rule, _)| rule),
            Some(words::RULE_COMPACT),
            "{d:?}"
        );
        d
    };
    for (key, says) in [(None, true), (Some(start - MIN), false)] {
        // The compaction's continuation.
        let mut s = tasked(&t, start);
        s.observe_turn_end(ours(start, "Stage 2 is in.", 5 * MIN), &t);
        let d = run(&mut s, &p, &t, start);
        assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_CONTINUE));
        let d = compacted(&mut s, key);
        assert_eq!(
            says_no_need(&d),
            says,
            "the compaction's continuation, {key:?}: {d:?}"
        );
        // The check a full context cut, typed whole before the key.
        let mut s = tasked(&t, start);
        s.observe_turn_end(end(start, "Everything is done.", 5 * MIN), &t);
        let d = run(&mut s, &p, &t, start);
        assert_eq!(
            typed(&d).map(|(rule, _)| rule),
            Some(words::RULE_DONE_CHECK)
        );
        assert!(says_no_need(&d), "the check, no key yet: {d:?}");
        let d = compacted(&mut s, key);
        assert_eq!(
            says_no_need(&d),
            says,
            "the check typed again, {key:?}: {d:?}"
        );
        // The carry-on after aterm's relaunch: the continuation, and the check over a done
        // claim.
        let mut s = ThreadState::default();
        s.observe_resume(true, Some("Working through stage 2."), true);
        keyed(&mut s, key, start);
        let d = decide(&s, &p, &t, start, NO_JITTER);
        assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_HOST_TURN));
        assert_eq!(says_no_need(&d), says, "the carry-on, {key:?}: {d:?}");
        let mut s = ThreadState::default();
        s.observe_resume(true, Some("All done, nothing left to do."), true);
        keyed(&mut s, key, start);
        let d = probed(&mut s, &p, &t, start, None);
        assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_HOST_TURN));
        assert_eq!(says_no_need(&d), says, "the carried check, {key:?}: {d:?}");
        // The usage limit's resume.
        let mut s = tasked(&t, start);
        s.observe_turn_end(
            walled(
                start,
                Wall::Usage {
                    reset_at: None,
                    reset_label: None,
                },
            ),
            &t,
        );
        keyed(&mut s, key, start);
        let d = decide(&s, &p, &t, start, NO_JITTER);
        assert_eq!(
            d.act,
            Act::ResumeLimit {
                text: words::LIMIT_MAY_HAVE_CLEARED.to_string()
            },
            "the usage resume, {key:?}"
        );
        assert!(!says_no_need(&d), "the usage resume, {key:?}: {d:?}");
    }
}

/// A CHECK A FULL CONTEXT CUT IS TYPED AGAIN AS THE HOUR HAS IT THEN, from its whole words
/// (aterm's `kept_words`): typed within the person's hour, without [`NEVER_A_NEED`], and cut by a
/// full context, it is typed again past the hour whole; the record carries it whole
/// ([`ThreadState::cut`]). NEGATIVE CONTROL: typed again within the hour, it is cut.
#[test]
fn a_check_cut_within_the_hour_is_typed_again_whole_past_it() {
    let t = Timing::default();
    let p = cruise_checked();
    let start = Instant::now();
    let key = start - 8 * SECS;
    for (again, says) in [
        (start + GO_AHEAD_HELD + 10 * MIN, true),
        (start + 10 * MIN, false),
    ] {
        let mut s = tasked(&t, start);
        s.observe_turn_end(end(start, "Everything is done.", 5 * MIN), &t);
        s.person_keyed(start.saturating_duration_since(key), start);
        let d = probed(&mut s, &p, &t, start, None);
        assert_eq!(
            typed(&d).map(|(rule, _)| rule),
            Some(words::RULE_DONE_CHECK)
        );
        assert!(!says_no_need(&d), "the check within the hour: {d:?}");
        s.acted_on(&d, start, &t);
        assert!(
            s.cut
                .as_deref()
                .is_some_and(|cut| cut.contains(NEVER_A_NEED)),
            "kept whole: {:?}",
            s.cut
        );
        let at = start + 5 * MIN;
        s.observe_turn_end(walled(at, Wall::Context { auto_compact: true }), &t);
        s.person_keyed(at.saturating_duration_since(key), at);
        let d = probed(&mut s, &p, &t, at, None);
        assert_eq!(d.act, Act::Compact);
        s.acted_on(&d, at, &t);
        s.observe_turn_end(ours(again, "", 20 * SECS), &t);
        s.person_keyed(again.saturating_duration_since(key), again);
        let d = probed(&mut s, &p, &t, again, None);
        assert_eq!(
            typed(&d).map(|(rule, _)| rule),
            Some(words::RULE_COMPACT),
            "{d:?}"
        );
        assert_eq!(
            says_no_need(&d),
            says,
            "typed again at {:?}: {d:?}",
            again - start
        );
    }
}

/// UNDER LAND THE HOLD NAMES NO LINE THE GO-AHEAD ANSWERED (review of the port, finding 11;
/// aterm's `land_held` and its `under_land_the_hold_has_no_needs_leg_for_an_answered_line`,
/// §15.6): the Land row aterm shows (`meta set harness_land`) would otherwise say a need holds
/// the landing while the menu bar shows none. NEGATIVE CONTROL: a person's need beside it keeps
/// the leg.
#[test]
fn under_land_the_hold_has_no_needs_leg_for_an_answered_line() {
    use codex_alab_aterm::LandLeg;
    let t = Timing::default();
    let p = land();
    let start = Instant::now();
    let held = |said: &str| {
        let mut s = tasked(&t, start);
        s.observe_mode(Mode::Land);
        s.observe_turn_end(ours(start, "Started on the parser.", 3 * MIN), &t);
        let first = judged_over(&mut s, &p, &t, start, Some(work()));
        assert_eq!(
            typed(&first).map(|(rule, _)| rule),
            Some(words::RULE_LAND),
            "{first:?}"
        );
        s.acted(&first.act, start, &t);
        s.stamp_land_hold(1_000);
        let at = start + 10 * MIN;
        s.observe_turn_end(ours(at, said, 3 * MIN), &t);
        let d = judged_over(&mut s, &p, &t, at, Some(work()));
        assert!(is_go_ahead(&d), "{d:?}");
        s.land_held().expect("Land holds")
    };
    let alone = held(INCIDENT);
    let read = alone.read.expect("the yield was read");
    assert_eq!(
        read.legs,
        vec![LandLeg::Defense],
        "the rest of the read stands: {read:?}"
    );
    let beside = held(&format!(
        "{INCIDENT}\nNEEDS YOU: the App Store signing credential."
    ));
    assert!(
        beside
            .read
            .as_ref()
            .is_some_and(|read| read.legs.contains(&LandLeg::Needs)),
        "NEGATIVE CONTROL: a person's need: {beside:?}"
    );
}

/// A HOST'S STEP KEEPS THE POINT'S GO-AHEAD DECIDED (review of the port, finding 12; aterm's
/// review 9, §15.6: "a point judges its decision before a host's step"): the live upgrade's
/// notice, typed in the go-ahead's place, leaves the line it answers off the menu bar, and where
/// the go-ahead was spent the refusal is still judged under the notice, its line back on the
/// menu bar. NEGATIVE CONTROL: with no notice due, the go-ahead itself is typed.
#[test]
fn the_upgrade_notice_keeps_the_go_ahead_decided_behind_it() {
    let t = Timing::default();
    let p = policy();
    let now = Instant::now();
    let notice = "[aterm harness] Codex /new-build is installed; this session runs 0.1.0.";
    let parked = |due: bool| {
        let mut s = tasked(&t, now);
        s.observe_turn_end(ours(now, INCIDENT, 5 * MIN), &t);
        if due {
            s.upgrade_notice = Some(notice.to_string());
        }
        s
    };
    let mut s = parked(true);
    let d = judged(&mut s, &p, &t, now);
    assert_eq!(
        typed(&d),
        Some((words::RULE_UPGRADE_NOTICE, notice)),
        "the notice goes first"
    );
    assert!(
        s.needs.shown().is_empty(),
        "the go-ahead decided behind it holds its line: {:?}",
        s.needs
    );
    assert_eq!(
        s.go_ahead_verdict(&d, &p, now),
        GoAheadVerdict::Answers(s.needs.standing.clone())
    );
    // The go-ahead spent: the refusal is judged under the notice.
    let mut s = parked(true);
    s.go_aheads = GoAheads {
        acts: GO_AHEAD_ACTS,
        first: Some(now),
    };
    let d = judged(&mut s, &p, &t, now);
    assert_eq!(
        typed(&d).map(|(rule, _)| rule),
        Some(words::RULE_UPGRADE_NOTICE)
    );
    assert_eq!(s.go_ahead_verdict(&d, &p, now), GoAheadVerdict::Refused);
    assert!(d.refuses(), "the refusal is told under the notice: {d:?}");
    assert_eq!(s.needs.shown(), s.needs.standing, "its line is back");
    // NEGATIVE CONTROL: no notice due.
    let mut s = parked(false);
    let d = judged(&mut s, &p, &t, now);
    assert!(is_go_ahead(&d), "{d:?}");
    assert!(!d.refuses());
}

/// A LEAD'S WORKER GETS THE GO-AHEAD AT ONCE AT ITS FIRST SHORT TURNS (review of the port,
/// finding 13; aterm's `continue_policy`: `waited_on` is `managed && (done || gate_ask)` within
/// [`MANAGED_SHORT_TURNS`], its `gate_ask` the go-ahead's whenever a park is owed): a park made of
/// a NEEDS YOU line alone, which reads as no question, is answered with no back-off on the first,
/// second and third short turn in a row, so the lead's `await task` hears it at once. NEGATIVE
/// CONTROLS: with no lead waiting it backs off, and the fourth short turn backs off under a lead
/// too.
#[test]
fn a_leads_worker_gets_the_go_ahead_at_once_at_its_first_short_turns() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    let park = "NEEDS YOU: say \"rerun the checks\" so I can build and push my branch.";
    let point = |n: u32, managed: bool| {
        let mut s = ThreadState {
            short_streak: n - 1,
            ..ThreadState::default()
        };
        s.observe_person_turn();
        s.observe_turn_end(end(start, park, 10 * SECS), &t);
        assert_eq!(s.short_streak, n);
        s.managed = managed;
        s
    };
    let backoff = |n: u32| start + (2 * MIN * (1 << (n - 1))).min(GO_AHEAD_WAIT);
    for n in 1..=MANAGED_SHORT_TURNS {
        let mut s = point(n, true);
        let d = judged(&mut s, &p, &t, start);
        assert!(is_go_ahead(&d), "park {n}: {d:?}");
        assert_eq!(d.not_before, None, "park {n}, a lead waits: {d:?}");
        let mut s = point(n, false);
        let d = judged(&mut s, &p, &t, start);
        assert!(is_go_ahead(&d), "park {n}, no lead: {d:?}");
        assert_eq!(d.not_before, Some(backoff(n)), "park {n}, no lead: {d:?}");
    }
    let mut s = point(MANAGED_SHORT_TURNS + 1, true);
    let d = judged(&mut s, &p, &t, start);
    assert!(is_go_ahead(&d), "{d:?}");
    assert_eq!(
        d.not_before,
        Some(backoff(MANAGED_SHORT_TURNS + 1)),
        "the fourth: {d:?}"
    );
}

/// THE TURN'S END FORESEES THE GO-AHEAD (review of the port, finding 14;
/// [`ThreadState::go_ahead_foreseen`], which the runtime's finish runs before it surfaces the
/// needs): the park's line is held off the menu bar from the turn's end where the go-ahead is
/// owed, as the decision then judges it, and is never held where the decision will not type
/// it: with answers off, its bound spent, within the person's hour, or over an irreversible
/// ask. NEGATIVE CONTROL: a reply that names a person's need alone holds nothing.
#[test]
fn the_turns_end_foresees_the_go_ahead_only_where_it_goes() {
    let t = Timing::default();
    let p = policy();
    let now = Instant::now();
    let ended = |said: &str| {
        let mut s = tasked(&t, now);
        s.observe_turn_end(ours(now, said, 5 * MIN), &t);
        s
    };
    let mut s = ended(INCIDENT);
    s.go_ahead_foreseen(&p, now);
    assert!(
        s.needs.shown().is_empty(),
        "held from the turn's end: {:?}",
        s.needs
    );
    let d = judged(&mut s, &p, &t, now);
    assert!(is_go_ahead(&d), "{d:?}");
    assert!(
        s.needs.shown().is_empty(),
        "the decision judges it the same"
    );
    // Answers off.
    let off = Policy {
        answer_questions: false,
        ..policy()
    };
    let mut s = ended(INCIDENT);
    s.go_ahead_foreseen(&off, now);
    assert_eq!(s.needs.shown(), s.needs.standing, "answers off");
    // The bound spent.
    let mut s = ended(INCIDENT);
    s.go_aheads = GoAheads {
        acts: GO_AHEAD_ACTS,
        first: Some(now),
    };
    s.go_ahead_foreseen(&p, now);
    assert_eq!(s.needs.shown(), s.needs.standing, "spent");
    // The person's hour.
    let mut s = ended(INCIDENT);
    s.person_keyed(30 * SECS, now);
    s.go_ahead_foreseen(&p, now);
    assert_eq!(
        s.needs.shown(),
        s.needs.standing,
        "within the person's hour"
    );
    // An irreversible ask.
    let mut s = ended(
        "NEEDS YOU: say \"go\" so I can force-push my branch over main and delete the old tags.",
    );
    s.go_ahead_foreseen(&p, now);
    assert_eq!(s.needs.shown(), s.needs.standing, "irreversible");
    assert!(!s.needs.standing.is_empty());
    // NEGATIVE CONTROL: a person's need alone.
    let mut s = ended("Stage 2 is in.\nNEEDS YOU: the App Store signing credential.");
    s.go_ahead_foreseen(&p, now);
    assert_eq!(s.needs.shown(), s.needs.standing, "a person's need");
    assert!(!s.needs.standing.is_empty());
}

/// THE MENU BAR (§15.6): beside a person's needs only theirs show, and the go-ahead acks them;
/// a reply that drops the line keeps it off; an act that was not submitted puts it back.
#[test]
fn beside_a_persons_needs_only_theirs_show() {
    let t = Timing::default();
    let now = Instant::now();
    let mixed = "The Mac is still swapping.\n\nNEEDS YOU: say \"rerun the checks\" when the Mac \
                 has memory to spare.\nNEEDS YOU: sign agy and Muse in on this Mac (browser \
                 OAuth).";
    let mut s = tasked(&t, now);
    s.observe_turn_end(ours(now, mixed, 3 * MIN), &t);
    let d = judged(&mut s, &policy(), &t, now);
    let text = go_ahead_text(&d).to_string();
    assert!(
        text.contains(GO_AHEAD_OTHERS) && text.contains(&policy().texts.needs_ack),
        "{text}"
    );
    assert_eq!(s.needs.standing.len(), 2);
    assert_eq!(
        s.needs.shown(),
        ["sign agy and Muse in on this Mac (browser OAuth)."]
    );
    // Not submitted: the line is back.
    let before = s.clone();
    assert!(s.go_ahead_unwritten());
    assert_eq!(s.needs.shown().len(), 2);
    s = before;
    s.acted(&d.act, now, &t);
    assert_eq!(s.needs.shown().len(), 1);
    // A reply that names no need keeps the answered line off; the person's need stands.
    let at = now + 20 * MIN;
    s.observe_turn_end(tooled(ours(at, "Rebuilding now.", 9 * MIN)), &t);
    assert_eq!(
        s.needs.shown(),
        ["sign agy and Muse in on this Mac (browser OAuth)."]
    );
    assert_eq!(
        s.needs,
        Needs {
            named: false,
            ..s.needs.clone()
        }
    );
}

/// Every text codex-alab types about the go-ahead: none parks, stops, names a need or claims
/// done, so an agent that quotes it asks nothing; none holds a stop phrase (aterm's review 3).
/// The refusal's words go to the person ([`Told::GoAhead`]), never into the agent.
#[test]
fn no_go_ahead_text_holds_a_stop_phrase_or_reads_as_an_ask() {
    use crate::go_ahead::QUOTE_CLOSE;
    use crate::go_ahead::QUOTE_OPEN;
    use crate::go_ahead::go_ahead_answer;
    use crate::go_ahead::parked;
    let quoted = go_ahead_answer(
        &parked("", INCIDENT).expect("parks"),
        Some(crate::needs::NEEDS_ACK),
        None,
    );
    let texts: Vec<String> = [
        GO_AHEAD_ANSWER,
        GO_AHEAD_ROOM,
        GO_AHEAD_DROP,
        GO_AHEAD_OTHERS,
        NEVER_A_NEED,
        TOKEN_OPEN,
        TOKEN_CLOSE,
        QUOTE_OPEN,
        QUOTE_CLOSE,
        crate::needs::NEEDS_ACK,
        crate::needs::NEEDS_HINT,
        &quoted,
    ]
    .iter()
    .map(|t| (*t).to_string())
    .collect();
    let mut held = Vec::new();
    for said in &texts {
        let flat = said
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        held.extend(
            words::STOP_PHRASES
                .iter()
                .filter(|p| flat.contains(*p))
                .map(|p| format!("{p:?} in {said:?}")),
        );
        assert!(parked("", said).is_none(), "parks: {said}");
        assert!(
            !matches!(
                words::classify_said(Some(said)),
                Said::Stop(_) | Said::Irreversible(_)
            ),
            "stops: {said}"
        );
        assert!(crate::needs::needs_you(said).is_empty(), "a need: {said}");
        assert!(!words::says_done(Some(said)), "done: {said}");
    }
    assert!(held.is_empty(), "{}", held.join("\n"));
    // The cut leaves no double space.
    let whole = format!("Keep going. {NEVER_A_NEED} Then land.");
    assert_eq!(
        crate::go_ahead::unless_held(&whole, true),
        "Keep going. Then land."
    );
}

/// A BACK-OFF NEVER OUTLASTS THE PERSON'S HOUR (aterm's `a_back_off_inside_the_hour_ends_at_the_
/// hours_end`, its review 9; review of the port, finding 14): the tab, a run of short turns
/// behind it, backs off for the ladder's top, an hour. The person's key comes in a turn that
/// ends on the incident's park, so their hour holds it; half an hour on, the agent's own wake
/// ends a new point on a reworded park, whose back-off, counted from that point, would run half
/// an hour past the hour's end. It ends at the hour's end instead, where the park is owed and the
/// go-ahead goes, nobody typing.
#[test]
fn a_back_off_inside_the_hour_ends_at_the_hours_end() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    let mut s = tasked(&t, start);
    let mut now = start;
    for i in 0..8 {
        now += 5 * SECS;
        s.observe_turn_end(
            ours(
                now,
                &format!("Checked the queue ({i}): the build still waits its turn."),
                5 * SECS,
            ),
            &t,
        );
        (_, now) = until_acted(&mut s, &p, &t, now);
    }
    let doublings = s.short_streak.saturating_sub(1).min(16);
    assert!(
        t.short_backoff.saturating_mul(1 << doublings) >= t.short_backoff_max,
        "PRECONDITION: the ladder's top ({} short turns)",
        s.short_streak
    );
    let p1 = now + MIN;
    let key = p1 - 8 * SECS;
    let hour = p1 + GO_AHEAD_HELD;
    s.observe_turn_end(ours(p1, INCIDENT, 5 * SECS), &t);
    s.person_keyed(p1.saturating_duration_since(key), p1);
    let mut now = p1;
    let mut d = judged(&mut s, &p, &t, now);
    while d.waits(now)
        && let Some(at) = d.not_before.filter(|at| *at < p1 + 30 * MIN)
    {
        now = at;
        s.person_keyed(now.saturating_duration_since(key), now);
        d = judged(&mut s, &p, &t, now);
    }
    assert!(!is_go_ahead(&d) || d.waits(now), "within the hour: {d:?}");
    let p2 = p1 + 30 * MIN;
    s.observe_turn_end(own_wake(p2, REWORDED, 5 * SECS), &t);
    s.person_keyed(p2.saturating_duration_since(key), p2);
    let d = judged(&mut s, &p, &t, p2);
    assert!(!is_go_ahead(&d) || d.waits(p2), "the hour holds it: {d:?}");
    let until = if d.act == Act::Nothing {
        d.look_again
    } else {
        d.not_before
    }
    .unwrap_or_else(|| panic!("the hour holds the reworded park: {d:?}"));
    assert!(
        until <= hour,
        "the back-off ends at the hour's end, not {:?} past it: {d:?}",
        until.saturating_duration_since(hour)
    );
    s.person_keyed(until.saturating_duration_since(key), until);
    let d = judged(&mut s, &p, &t, until);
    assert!(is_go_ahead(&d) && !d.waits(until), "{d:?}");
}

/// THE PERSON'S HOUR COUNTS THE MAC'S SLEEP (aterm's `the_persons_hour_counts_the_macs_sleep`,
/// its review 9; review of the port, finding 14): the person types as their turn opens, the turn
/// ends on the incident's park, and their hour holds it. The lid closes a minute on and opens two
/// hours later, where `Instant` stood still: the wake's reading of the host's time asleep moves
/// the hour back by it ([`ThreadState::slept_to`]), and the window's typing age counts the sleep.
/// The park the agent ends at the wake gets the go-ahead then. POSITIVE CONTROL: the sleep
/// uncounted, the same park waits for the hour's end in awake time.
#[test]
fn the_persons_hour_counts_the_macs_sleep() {
    let t = Timing::default();
    let p = policy();
    // Headroom for the moves back.
    let p1 = Instant::now() + 12 * 60 * MIN;
    let asleep = 2 * 60 * MIN;
    let base = Duration::from_secs(5000);
    let key = p1 - 3 * MIN;
    let woke = p1 + 2 * MIN;
    for counted in [true, false] {
        let mut s = tasked(&t, p1);
        s.observe_turn_end(end(p1, INCIDENT, 3 * MIN), &t);
        s.person_keyed(p1.saturating_duration_since(key), p1);
        let d = judged(&mut s, &p, &t, p1);
        assert!(!is_go_ahead(&d), "within the hour: {d:?}");
        assert!(s.go_ahead_held(p1 + MIN), "the hour holds");
        assert_eq!(s.slept_to(Some(base), p1), None, "the first reading marks");
        let typed_key = if counted {
            assert_eq!(s.slept_to(Some(base + asleep), woke), Some(asleep));
            // The window's typing age counts the sleep.
            key - asleep
        } else {
            key
        };
        s.observe_turn_end(ours(woke, INCIDENT, MIN), &t);
        if counted {
            let (d, at) = until_acted_keyed(&mut s, &p, &t, woke, Some(typed_key));
            assert!(is_go_ahead(&d), "{d:?}");
            assert!(
                at < p1 + GO_AHEAD_HELD,
                "at the wake, not {:?} on in awake time",
                at.saturating_duration_since(woke)
            );
        } else {
            s.person_keyed(woke.saturating_duration_since(typed_key), woke);
            assert!(s.go_ahead_held(woke), "POSITIVE CONTROL: the hour holds");
            let d = judged(&mut s, &p, &t, woke);
            assert!(!is_go_ahead(&d), "POSITIVE CONTROL: {d:?}");
        }
    }
}

/// A SLEEP IS NO PARK STANDING (aterm's `a_sleep_after_a_go_ahead_is_no_park_standing`, its
/// review 9; review of the port, finding 14): the go-ahead went once, the agent answered it in
/// forty seconds with the park reworded (no same reply holds it), and the lid closed inside the
/// back-off and opened nine hours later. The sleep is the Mac's wait, not the agent's on its
/// park: the wake's decision types the second go-ahead, never the refusal, and the span runs
/// again from that act ([`ThreadState::slept_to`]), so a park 58 minutes after it gets the third,
/// though it ends more than [`GO_AHEAD_SPAN`] after the first. NEGATIVE CONTROL: with no sleep,
/// that park is past the span and is refused.
#[test]
fn a_sleep_after_a_go_ahead_is_no_park_standing() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now() + 12 * 60 * MIN;
    let base = Duration::from_secs(5000);
    let reworded = "Memory still low. NEEDS YOU: say \"rerun the checks\" when memory frees.";
    let third = "NEEDS YOU: say \"go\" so I can rebuild my branch and push it to main.";
    for slept in [true, false] {
        let mut s = tasked(&t, start);
        s.observe_turn_end(ours(start, INCIDENT, 3 * MIN), &t);
        let d = judged(&mut s, &p, &t, start);
        assert!(is_go_ahead(&d), "{d:?}");
        s.acted(&d.act, start, &t);
        let then = start + 40 * SECS;
        s.observe_turn_end(ours(then, reworded, 30 * SECS), &t);
        assert_eq!(
            s.slept_to(Some(base), then),
            None,
            "the first reading marks"
        );
        let d = judged(&mut s, &p, &t, then);
        assert!(
            is_go_ahead(&d) && d.waits(then),
            "PRECONDITION: the back-off after the act: {d:?}"
        );
        let woke = start + 5 * MIN;
        if slept {
            assert_eq!(
                s.slept_to(Some(base + 9 * 60 * MIN), woke),
                Some(9 * 60 * MIN)
            );
        }
        let d = judged(&mut s, &p, &t, woke);
        assert!(
            is_go_ahead(&d) && !d.waits(woke),
            "the second go-ahead at the wake: {d:?}"
        );
        assert!(!d.refused, "nothing refused: {d:?}");
        s.acted(&d.act, woke, &t);
        let later = woke + 58 * MIN;
        s.observe_turn_end(tooled(ours(later, third, 30 * SECS)), &t);
        let d = judged(&mut s, &p, &t, later);
        if slept {
            let (d, _) = until_acted(&mut s, &p, &t, later);
            assert!(is_go_ahead(&d), "the span runs from the wake's act: {d:?}");
        } else {
            assert!(
                d.act == Act::Nothing && d.refused,
                "NEGATIVE CONTROL: {d:?}"
            );
        }
    }
}
