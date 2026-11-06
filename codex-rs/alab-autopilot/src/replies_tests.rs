// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Ruling F's arithmetic (aterm's `Replies`, `reply_mark` and `HeldBy`), each rule with its
//! negative control.

use std::time::Duration;
use std::time::Instant;

use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::replies::HeldBy;
use crate::replies::Kept;
use crate::replies::NO_WORK_ACTS;
use crate::replies::Replies;
use crate::replies::STOPPED_TYPING;
use crate::replies::TYPES_AGAIN;
use crate::replies::mark_of;
use crate::replies::stop_line;
use pretty_assertions::assert_eq;

fn said(earlier: Option<&str>, text: Option<&str>) -> TurnEnd {
    TurnEnd {
        at: Instant::now(),
        by_autopilot: true,
        by_person: false,
        text: text.map(str::to_string),
        earlier: earlier.map(str::to_string),
        questions: false,
        worked: Duration::from_secs(5),
        wall: None,
        aborted: false,
        ran_tools: false,
        restored: false,
    }
}

fn mark(text: &str) -> u64 {
    mark_of(&said(None, Some(text)))
}

/// THE REPLY'S MARK (aterm's `mark_of`): a word with a digit is one `#`, so a count, a
/// countdown and a clock (its `AM`/`PM` dropped) are the same reply; punctuation and spacing
/// are no words; an empty reply and a reply of no words are one mark; the turn's earlier words
/// count before its last; at a wall the wall's class and own words are the reply, whatever
/// the turn said. NEGATIVE CONTROLS: other letters, another case, other earlier words, a
/// wall's other class or words, and an `AM` that follows no number are other replies.
#[test]
fn a_replys_mark_folds_numbers_and_clocks_and_a_walls_is_its_class_and_own_words() {
    assert_eq!(mark("Trying fix 1 of 4."), mark("Trying fix 2 of 4."));
    assert_eq!(mark("Resets in 45s."), mark("Resets in 12s."));
    assert_eq!(
        mark("Back at 5:11 PM today."),
        mark("Back at 6:02 am today.")
    );
    assert_eq!(mark("Back at 5:11 PM today."), mark("Back at 6:02 today."));
    assert_eq!(mark("Still  waiting,  on it."), mark("Still waiting on it"));
    assert_eq!(mark_of(&said(None, None)), mark(""));
    assert_eq!(mark_of(&said(None, None)), mark("  ... --  "));
    assert_ne!(mark("Still waiting."), mark("Still working."));
    assert_ne!(
        mark("Still waiting."),
        mark("still waiting."),
        "case is kept"
    );
    assert_ne!(
        mark("I am here."),
        mark("I here."),
        "an AM after no number is a word"
    );
    assert_ne!(
        mark_of(&said(Some("Built it."), Some("Done."))),
        mark_of(&said(Some("Broke it."), Some("Done."))),
        "the turn's earlier words are the reply too"
    );
    let walled = |wall: Wall, text: &str| TurnEnd {
        wall: Some(wall),
        ..said(Some("earlier"), Some(text))
    };
    let usage = |label: &str| Wall::Usage {
        reset_at: None,
        reset_label: Some(label.to_string()),
    };
    assert_eq!(
        mark_of(&walled(usage("5:11 PM"), "one reply")),
        mark_of(&walled(usage("6:40 AM"), "another")),
        "a wall's reply is its class and its own words, a reset's clock one number"
    );
    assert_eq!(
        mark_of(&TurnEnd {
            wall: Some(usage("5:11 PM")),
            ..said(Some("Built it."), Some("x"))
        }),
        mark_of(&TurnEnd {
            wall: Some(usage("5:11 PM")),
            ..said(Some("Broke it."), Some("y"))
        }),
        "a wall's reply ignores the turn's earlier words as its last"
    );
    let cut = |what: &str| Wall::Server {
        what: what.to_string(),
        cut_off: true,
        retry_at: None,
    };
    assert_ne!(
        mark_of(&walled(cut("stream disconnected"), "x")),
        mark_of(&walled(cut("response incomplete"), "x")),
        "the control: a wall's own other words"
    );
    assert_ne!(
        mark_of(&walled(Wall::Entitlement, "x")),
        mark_of(&walled(Wall::Login, "x")),
        "the control: another class"
    );
    assert_ne!(
        mark_of(&walled(Wall::Entitlement, "x")),
        mark("x"),
        "the control: the turn's words with no wall"
    );
}

/// THE STREAK KEEPS THE NEWEST THREE REPLIES (aterm's `Replies::count`, `of`, `held_by`): a
/// reply said again counts again and is newest; the second answer of one reply holds as
/// [`HeldBy::SameReply`], six no-work answers in a row of any wording as [`HeldBy::NoWork`];
/// a wall's spent tries hold at one answer. NEGATIVE CONTROLS: one answer holds nothing; a
/// fourth wording forgets the oldest; an answer with work ends the run; a reply no wall said
/// holds on no tries.
#[test]
fn the_streak_keeps_three_replies_and_holds_at_a_second_answer_or_a_run_of_six() {
    let mut r = Replies::default();
    r.count(1, false, false, true);
    assert_eq!(r.held_by(), None, "one answer holds nothing");
    r.count(2, false, false, true);
    r.count(1, false, false, true);
    assert_eq!(r.last().map(|k| (k.mark, k.answers)), Some((1, 2)));
    assert_eq!(r.held_by(), Some(HeldBy::SameReply));
    let mut r = Replies::default();
    for m in 0..NO_WORK_ACTS as u64 {
        assert_eq!(r.held_by(), None, "{m}");
        r.count(10 + m, false, false, true);
    }
    assert_eq!(r.kept().count(), 3, "the newest three: {r:?}");
    assert!(!r.has(10), "the oldest went");
    assert_eq!(r.held_by(), Some(HeldBy::NoWork));
    r.count(99, false, false, false);
    assert_eq!((r.run, r.held_by()), (0, None), "work ends the run");
    let mut r = Replies::default();
    for _ in 0..3 {
        r.count(7, true, true, true);
        r.kept = [r.last().map(|k| Kept { answers: 1, ..k }), None, None];
    }
    assert_eq!(r.last().map(|k| k.tries), Some(3));
    assert_eq!(r.held_by(), Some(HeldBy::SameReply), "a wall's tries spent");
    r.kept = [r.last().map(|k| Kept { walled: false, ..k }), None, None];
    assert_eq!(r.held_by(), None, "the control: no wall said it");
}

/// The arm's word reads back to the arm, and the stop's line leads with the stop and ends
/// with what starts the typing again, in aterm's words (`stop_words(.., StopForm::Line)`).
/// NEGATIVE CONTROL: another word is no arm.
#[test]
fn a_holds_arm_round_trips_its_word_and_its_line_says_the_stop_in_aterms_words() {
    for held in HeldBy::ALL {
        assert_eq!(HeldBy::of_word(held.word()), Some(held));
        let line = stop_line(held);
        assert!(line.starts_with(STOPPED_TYPING), "{line}");
        assert!(line.ends_with(TYPES_AGAIN), "{line}");
    }
    assert_eq!(
        stop_line(HeldBy::SameReply),
        "stopped typing: the same reply twice, no work \u{b7} types again at your turn or once the agent works"
    );
    assert_eq!(
        stop_line(HeldBy::NoWork),
        "stopped typing: 6 replies in a row, no work \u{b7} types again at your turn or once the agent works"
    );
    assert_eq!(HeldBy::of_word("-"), None);
}
