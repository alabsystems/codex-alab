// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! NO NEEDS YOU LINE IS LOST, AND LAND LEAVES A PERSON ONLY THE MESSAGE THAT OPENS THE SESSION
//! (owner, 2026-10-19, restated 2026-10-26 and 2026-10-29: "if you need something from the
//! user, be LOUDER ABOUT IT ... but NEVER let that block you"; "the user question exception is
//! only for the first message of a session", 2026-10-27). The decider's half: a turn a wall or
//! an interrupt ended still names the needs its complete messages said; one message's open
//! code fence hides no later message's need; and an exchange an Off left to the person past
//! the session's opening is Land's to carry once the word moves to Land.

use std::time::Instant;

use pretty_assertions::assert_eq;

use super::*;
use crate::extension::turn_said;

const NEED: &str = "the Developer ID certificate password";
const SAID: &str = "The release build needs signing.\n\
                    NEEDS YOU: the Developer ID certificate password\n\
                    Carrying on with the docs meanwhile.";

/// The turn ends that cut a turn's work short: a usage window, a server's error, a full
/// context, a lost login, and the person's interrupt.
fn cut_short(now: Instant) -> Vec<(&'static str, Option<Wall>, bool)> {
    vec![
        (
            "a usage window",
            Some(Wall::Usage {
                reset_at: Some(now + 30 * MIN),
                reset_label: None,
            }),
            false,
        ),
        (
            "a server's error",
            Some(Wall::Server {
                what: "a 500".to_string(),
                cut_off: false,
                retry_at: None,
            }),
            false,
        ),
        (
            "a full context",
            Some(Wall::Context { auto_compact: true }),
            false,
        ),
        ("a lost login", Some(Wall::Login), false),
        ("an interrupt", None, true),
    ]
}

/// A NEED SAID IN A TURN A WALL OR AN INTERRUPT ENDED STANDS (needs-wall-turn-unfolded): Codex
/// hands each agent message whole, and the wall comes from a later request of the turn, so the
/// complete messages are read whatever ended it: the need goes on the menu bar and owes its
/// ack. The usage window's resume types its one cleared line under Cruise and under Land past
/// its instruction ([`resume_text`]: a prompt into a turn the agent did not end is spam), and
/// that line carries no ack, so the ack stays owed for the act at the agent's own next turn
/// end, which carries it. NEGATIVE CONTROL: a walled turn whose words name no need folds none,
/// and leaves what stood before it standing.
#[test]
fn a_need_said_in_a_turn_a_wall_or_an_interrupt_ended_stands() {
    let t = Timing::default();
    let now = Instant::now();
    let at = now + 20 * MIN;
    for (case, wall, aborted) in cut_short(now) {
        for p in [policy(), land()] {
            let mut s = ThreadState::default();
            s.observe_turn_end(end(now, "Ship the release.", 5 * MIN), &t);
            run(&mut s, &p, &t, now);
            s.observe_turn_end(
                TurnEnd {
                    wall: wall.clone(),
                    aborted,
                    ..ours(at, SAID, 10 * MIN)
                },
                &t,
            );
            assert_eq!(s.needs.standing, [NEED], "{case} under {:?}", p.autopilot);
            assert!(s.needs.ack_owed(), "{case} under {:?}", p.autopilot);
            // NEGATIVE CONTROL: the same end, its words naming no need, folds none.
            let mut quiet = ThreadState::default();
            quiet.observe_turn_end(end(now, "Ship the release.", 5 * MIN), &t);
            run(&mut quiet, &p, &t, now);
            quiet.observe_turn_end(
                TurnEnd {
                    wall: wall.clone(),
                    aborted,
                    ..ours(at, "Stage 2 is half in.", 10 * MIN)
                },
                &t,
            );
            assert_eq!(quiet.needs, Needs::default(), "{case}: no need named");
        }
    }
    let usage = |at: Instant| TurnEnd {
        wall: Some(Wall::Usage {
            reset_at: Some(at + 30 * MIN),
            reset_label: None,
        }),
        ..ours(at, SAID, 10 * MIN)
    };
    // Under either word the resume types the cleared line alone; the reply to it names no
    // need, the need stands, and the act at the agent's own next turn end carries the ack.
    for (p, rule) in [
        (policy(), words::RULE_CONTINUE),
        (land(), words::RULE_LAND_NUDGE),
    ] {
        let mode = p.autopilot;
        let mut s = ThreadState::default();
        s.observe_turn_end(end(now, "Ship the release.", 5 * MIN), &t);
        run(&mut s, &p, &t, now);
        s.observe_turn_end(usage(at), &t);
        let d = probed(&mut s, &p, &t, at, None);
        assert_eq!(
            d.act,
            Act::ResumeLimit {
                text: words::LIMIT_CLEARED.to_string()
            },
            "{mode:?}"
        );
        let back = at + 31 * MIN;
        s.acted(&d.act, back, &t);
        assert!(s.needs.ack_owed(), "{mode:?}: the cleared line gave no ack");
        s.observe_turn_end(
            ours(back + 20 * MIN, "Docs updated and pushed.", 20 * MIN),
            &t,
        );
        assert_eq!(s.needs.standing, [NEED], "{mode:?}");
        let d = probed(&mut s, &p, &t, back + 20 * MIN, None);
        let (said, text) = typed(&d).unwrap_or_else(|| panic!("{mode:?}: the point's act: {d:?}"));
        assert_eq!(said, rule, "{mode:?}");
        assert!(text.contains(NEEDS_ACK), "{mode:?}: {text}");
        s.acted(&d.act, back + 20 * MIN, &t);
        assert!(!s.needs.ack_owed(), "{mode:?}: the act gave the ack");
        assert_eq!(
            s.needs.standing,
            [NEED],
            "{mode:?}: the need stays on the menu bar"
        );
    }
}

/// The README snippet a worker shows in one message: a ```markdown block that nests a ```bash
/// block with the same three backticks, so the reader's fence opens again at its last row and
/// never closes in that message.
const NESTED: &str = "Here is the README's install section now:\n\
                      ```markdown\n\
                      ## Install\n\
                      ```bash\n\
                      pip install foo\n\
                      ```\n\
                      ```";
const PYPI: &str = "the PyPI token to publish foo 0.3.0";

/// ONE MESSAGE'S OPEN CODE FENCE HIDES NO LATER MESSAGE'S NEED (needs-fence-crosses-messages):
/// each agent message is its own Markdown item, which the TUI renders on its own, so a fence a
/// message leaves open ends with that message ([`turn_said`] closes it, and the turn's whole
/// reading closes what earlier words a record carries leave open before the reply): a NEEDS YOU
/// line in a later message of the turn stands, and keeps Land's clean defense from landing. NEGATIVE
/// CONTROLS: the need inside the same message's open fence is still code, and the turn with no
/// need in its later message lands.
#[test]
fn one_messages_open_fence_hides_no_later_messages_need() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let at = now + 20 * MIN;
    let need = format!("NEEDS YOU: {PYPI}");
    let said = [NESTED.to_string(), need.clone()];
    let earlier = turn_said(&said, Some(said.len()));
    assert_eq!(
        crate::needs::needs_you(earlier.as_deref().unwrap_or_default()),
        [PYPI],
        "{earlier:?}"
    );
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Publish foo 0.3.0.", 5 * MIN), &t);
    run(&mut s, &p, &t, now);
    let mut clean = s.clone();
    s.observe_turn_end(
        TurnEnd {
            earlier,
            ..ours(at, CLEAN, 10 * MIN)
        },
        &t,
    );
    assert_eq!(s.needs.standing, [PYPI]);
    let d = probed(&mut s, &p, &t, at, None);
    assert!(!s.landed, "{d:?}");
    let (rule, text) = typed(&d).unwrap_or_else(|| panic!("Land's act: {d:?}"));
    assert_eq!(rule, words::RULE_LAND_NUDGE);
    assert!(
        text.contains(NEEDS_ACK),
        "the act carries the need's ack: {text}"
    );
    // The earlier words a record carries whole, joined before each message's fence was closed
    // (an older build's): their open fence ends before the reply, whose need stands.
    let mut older = clean.clone();
    older.observe_turn_end(
        TurnEnd {
            earlier: Some(NESTED.to_string()),
            ..ours(at, &need, 10 * MIN)
        },
        &t,
    );
    assert_eq!(older.needs.standing, [PYPI]);
    // NEGATIVE CONTROL: within one message, the line under its open fence is code.
    let one = format!("{NESTED}\n{need}");
    assert_eq!(crate::needs::needs_you(&one), Vec::<String>::new());
    // NEGATIVE CONTROL: the later message names no need; the defense lands.
    let said = [NESTED.to_string(), "Published foo 0.3.0.".to_string()];
    clean.observe_turn_end(
        TurnEnd {
            earlier: turn_said(&said, Some(said.len())),
            ..ours(at, CLEAN, 10 * MIN)
        },
        &t,
    );
    assert_eq!(probed(&mut clean, &p, &t, at, None).act, Act::Landed);
}

/// AN EXCHANGE AN OFF LEFT TO THE PERSON IS LAND'S TO CARRY ONCE THE SESSION HAS WORKED (edges-4;
/// the skeptic who called it designed is overruled by the owner's 2026-10-27 words, "the user
/// question exception is only for the first message of a session"): their quick messages under
/// the Off are theirs while the Off stands, and Land read after it takes the point back, as
/// aterm's `mode_takes_exchange` does (aterm 2242dc8b91). NEGATIVE CONTROLS: the
/// session's opening message under the Off stays theirs under Land, a person's hold over Off
/// takes nothing back, and Cruise takes either back as before.
#[test]
fn land_takes_back_an_exchange_an_off_left_once_the_session_has_worked() {
    let t = Timing::default();
    let now = Instant::now();
    let p = policy();
    // After work, the Off having ended the carry (`park_off`).
    let worked = || {
        let mut s = ThreadState::default();
        s.observe_turn_end(end(now, "Stage 1 is in.", 12 * MIN), &t);
        run(&mut s, &p, &t, now);
        s.observe_turn_end(ours(now + 20 * MIN, "Stage 2 is in.", 15 * MIN), &t);
        s.park_off();
        let at = now + 40 * MIN;
        by_hand(&mut s, &t, None, at, "Committed it; not pushed yet.", MIN);
        assert!(s.left, "theirs while the Off stands");
        s
    };
    let at = now + 40 * MIN;
    let mut s = worked();
    let d = probed(&mut s, &land(), &t, at, None);
    assert!(!s.left && !s.exchange, "{s:?}");
    assert_eq!(
        typed(&d).map(|(rule, _)| rule),
        Some(words::RULE_LAND),
        "{d:?}"
    );
    // NEGATIVE CONTROL: a person's hold over Off takes nothing back.
    let mut held = worked();
    held.observe_mode_under(Mode::Land, true);
    assert!(held.left);
    // NEGATIVE CONTROL: the session's opening message under the Off stays theirs under Land.
    let mut opening = ThreadState::default();
    by_hand(&mut opening, &t, None, now, "Here is the plan.", 3 * MIN);
    assert!(opening.left);
    assert_eq!(
        probed(&mut opening, &land(), &t, now, None).act,
        Act::Nothing
    );
    assert!(opening.left);
    // NEGATIVE CONTROL: Cruise takes either back.
    for mut s in [worked(), {
        let mut s = ThreadState::default();
        by_hand(&mut s, &t, None, now, "Here is the plan.", 3 * MIN);
        s
    }] {
        s.observe_mode(Mode::Cruise);
        assert!(!s.left);
    }
}
