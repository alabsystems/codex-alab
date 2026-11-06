// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! RULING F, THE TYPING STOP (aterm's §14.3 of the Autopilot design, commit 4cefe18775:
//! "the same reply twice with no work, or six no-work answers in a row, stops typing until
//! real work or a person's turn"): the replies the short streak keeps ([`Replies`], aterm's
//! `turn_end::Replies`), each turn's reply as a mark ([`mark_of`], aterm's `reply_mark`), and
//! the arm that holds ([`HeldBy`], aterm's `harness::acts::HeldBy`) in aterm's own words.
//!
//! [`crate::decide::ThreadState`] counts every answer into them and asks
//! [`crate::decide::ThreadState::repeats`] before it types; this module is the arithmetic and
//! the hold's words ([`Told`]). STAGE 2 (2026-10-28): a wall's answers count with their walled
//! flag and own tries, and a wall that can hold ([`crate::decide::wall_holds`]: a usage window,
//! a quota, a reply cut off, a reply past its output maximum) is held once its own tries are
//! spent (aterm's `act_at_point` hold), its dated resets ([`same_reset`]) lifting the hold for
//! one more answer. The model's capacity outages and the cyber policy's refusal never hold
//! (the owner's ruling: codex-alab keeps retrying through them).

use std::time::Duration;
use std::time::Instant;

use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::needs;

/// The decider's check that holds an act at the same reply (aterm's `SAME_REPLY`).
pub const SAME_REPLY: &str = "same-reply";

/// HOW MANY REPLIES THE SHORT STREAK KEEPS (aterm's `REPLIES_KEPT`).
pub const REPLIES_KEPT: usize = 3;

/// HOW MANY NO-WORK ACTS IN A ROW HOLD WHATEVER THEIR WORDING (aterm's `NO_WORK_ACTS`): two
/// through each reply the streak keeps, so the same-reply hold always comes first.
pub const NO_WORK_ACTS: usize = 2 * REPLIES_KEPT;

/// A WALL'S OWN TRIES before its same answer holds (aterm's `FUTILE_AFTER_TRIES`).
pub const FUTILE_AFTER_TRIES: usize = 3;

/// HOW NEAR TWO READINGS OF ONE DATED RESET LAND (aterm's `SAME_RESET`): aterm places a clock
/// it reads on its own clock at each read; codex-alab places Codex's `resets_at` (epoch
/// seconds) on the `Instant` clock at each wall, so two readings of one reset land
/// milliseconds apart, and another reset names another minute.
pub const SAME_RESET: Duration = Duration::from_secs(30);

/// Whether `at` is the reset `kept` was, read again (aterm's `same_reset`, [`SAME_RESET`]).
#[must_use]
pub fn same_reset(kept: Option<Instant>, at: Instant) -> bool {
    kept.is_some_and(|k| k.max(at).duration_since(k.min(at)) < SAME_RESET)
}

/// ONE REPLY THE SHORT STREAK KEEPS (aterm's `Kept`): its mark ([`mark_of`]), the no-work
/// answers that settled to it, whether a wall said it, and those answering the wall's own
/// acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kept {
    pub mark: u64,
    pub answers: usize,
    pub walled: bool,
    pub tries: usize,
}

impl Kept {
    /// Two no-work answers settled to it (the reply came again), or a wall's tries are spent
    /// at it (aterm's `Kept::holds`).
    #[must_use]
    pub const fn holds(&self) -> bool {
        self.answers >= 2 || (self.walled && self.tries >= FUTILE_AFTER_TRIES)
    }
}

/// THE REPLIES THE SHORT STREAK KEEPS, the newest [`REPLIES_KEPT`] last, and how many no-work
/// answers came in a row (aterm's `Replies`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Replies {
    pub kept: [Option<Kept>; REPLIES_KEPT],
    pub run: usize,
}

impl Replies {
    /// The newest reply kept.
    #[must_use]
    pub fn last(&self) -> Option<Kept> {
        self.kept.iter().rev().find_map(|k| *k)
    }

    /// The replies kept, the oldest first.
    pub fn kept(&self) -> impl Iterator<Item = Kept> + '_ {
        self.kept.iter().filter_map(|k| *k)
    }

    /// Nothing kept and no run: the streak's replies are at rest.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.last().is_none() && self.run == 0
    }

    /// WHICH ARM HOLDS (aterm's `Replies::held_by`): the newest kept reply's own
    /// ([`Kept::holds`]) is [`HeldBy::SameReply`], else [`NO_WORK_ACTS`] no-work answers in a
    /// row is [`HeldBy::NoWork`]; `None` where neither holds. It reads no turn: whether a
    /// point is held is [`crate::decide::ThreadState::repeats`]'s.
    #[must_use]
    pub fn held_by(&self) -> Option<HeldBy> {
        if self.last().is_some_and(|k| k.holds()) {
            Some(HeldBy::SameReply)
        } else if self.run >= NO_WORK_ACTS {
            Some(HeldBy::NoWork)
        } else {
            None
        }
    }

    /// One more answer that settled to `mark` (`own`: to a wall's own act; `idle`: no work):
    /// a reply kept already counts it again, newest now (aterm's `Replies::count`).
    pub fn count(&mut self, mark: u64, walled: bool, own: bool, idle: bool) {
        let before = self.kept().find(|k| k.mark == mark);
        let mut kept: Vec<Kept> = self.kept().filter(|k| k.mark != mark).collect();
        kept.push(Kept {
            mark,
            answers: before.map_or(1, |k| k.answers.saturating_add(1)),
            walled,
            tries: before
                .map_or(0, |k| k.tries)
                .saturating_add(usize::from(own)),
        });
        let run = if idle { self.run.saturating_add(1) } else { 0 };
        *self = Self {
            run,
            ..Self::of(&kept)
        };
    }

    /// Whether a reply kept has `mark`.
    #[must_use]
    pub fn has(&self, mark: u64) -> bool {
        self.kept().any(|k| k.mark == mark)
    }

    /// The newest [`REPLIES_KEPT`] of `kept` (aterm's `Replies::of`).
    #[must_use]
    pub fn of(kept: &[Kept]) -> Self {
        let mut out = Self::default();
        for (slot, k) in out
            .kept
            .iter_mut()
            .zip(&kept[kept.len().saturating_sub(REPLIES_KEPT)..])
        {
            *slot = Some(*k);
        }
        out
    }
}

/// WHY CODEX-ALAB STOPPED TYPING INTO A THREAD (aterm's `harness::acts::HeldBy`): the arm of
/// ruling F's hold that holds it ([`Replies::held_by`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeldBy {
    /// The newest reply came again with no work (or a wall's tries are spent at it).
    SameReply,
    /// [`NO_WORK_ACTS`] no-work answers in a row, whatever their wording.
    NoWork,
}

impl HeldBy {
    /// Both arms.
    pub const ALL: [HeldBy; 2] = [HeldBy::SameReply, HeldBy::NoWork];

    /// Its word, as aterm's `status held=` and journal spell it.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            HeldBy::SameReply => SAME_REPLY,
            HeldBy::NoWork => "no-work",
        }
    }

    /// The arm of `word` ([`Self::word`]'s inverse); `None` for any other word.
    #[must_use]
    pub fn of_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|arm| arm.word() == word)
    }

    /// What held it, in aterm's written form (`the same reply twice, no work`).
    #[must_use]
    pub fn why(self) -> String {
        match self {
            HeldBy::SameReply => "the same reply twice, no work".to_owned(),
            HeldBy::NoWork => format!("{NO_WORK_ACTS} replies in a row, no work"),
        }
    }
}

/// What every saying of a stop begins with (aterm's `STOPPED_TYPING`).
pub const STOPPED_TYPING: &str = "stopped typing";

/// What starts the typing again (aterm's `TYPES_AGAIN`): a mode switch does not, Off's park
/// does.
pub const TYPES_AGAIN: &str = "types again at your turn or once the agent works";

/// The diagnostic journal line, recorded once per held operation.
#[must_use]
pub fn stop_line(held: HeldBy) -> String {
    format!("{STOPPED_TYPING}: {} \u{b7} {TYPES_AGAIN}", held.why())
}

/// A LIMIT HELD PAST ITS RESET (aterm's `Exception::limit_repeats` with a reset read, the
/// actor's name codex-alab's).
pub const LIMIT_STANDS: &str =
    "The limit still stands after its reset and answers each of codex-alab's tries the same.";

/// A LIMIT HELD THAT NAMES NO RESET (aterm's `Exception::limit_repeats` with none read: Codex's
/// usage window without `resets_at`). A held quota says it too, codex-alab's own line beside
/// the quota's standing need: aterm tells a spend wall that names no reset as its `Spend`
/// exception, held or not, and that exception's counterpart here is the need
/// ([`crate::decide::QUOTA_NEED`]), which stays up while the hold stands.
pub const LIMIT_UNREAD: &str =
    "The limit names no reset codex-alab can read and answers each of codex-alab's tries the same.";

/// What lifts a limit's hold (aterm's `limit_repeats` `clears`, in the stop line's form),
/// without aterm's "a new reset": aterm's loop reads a reset off the screen at every look,
/// while codex-alab reads one only at a turn's end, which counts that answer and keeps its
/// reset as the one the hold compares with, so a new reset alone never lifts a spent hold.
pub const LIMIT_TYPES_AGAIN: &str = "types again at a new wall, your turn or once the agent works";

/// The keyed attention reason for a go-ahead refusal that only the person can lift.
pub const GO_AHEAD_STOP_KEY: &str = "stopped";

/// A held operation is recorded once per change. Only a spent standing go-ahead asks the
/// person: ordinary holds remain quiet under current main's autonomous-recovery policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Told {
    /// A repeated operation that cannot proceed yet.
    Stop(HeldBy),
    /// A spent usage window or quota.
    Limit { reset_read: bool },
    /// THE GO-AHEAD'S REFUSAL (aterm's `Exception::refuses_go_ahead`): the agent still parks
    /// its own reversible work after the standing answer's bound, or repeats the same no-work
    /// reply after one. The person's own word in the tab is the one action that lifts it.
    GoAhead { arm: Option<HeldBy> },
}

impl Told {
    /// The arm recorded in aterm's machine-readable status.
    #[must_use]
    pub const fn arm(&self) -> Option<HeldBy> {
        match self {
            Told::Stop(arm) => Some(*arm),
            Told::GoAhead { arm } => *arm,
            Told::Limit { .. } => None,
        }
    }

    /// The diagnostic journal entry.
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            Told::Stop(arm) => stop_line(*arm),
            Told::Limit { reset_read } => format!(
                "{STOPPED_TYPING}: {} \u{b7} {LIMIT_TYPES_AGAIN}",
                if *reset_read {
                    LIMIT_STANDS
                } else {
                    LIMIT_UNREAD
                }
            ),
            Told::GoAhead { .. } => format!(
                "{STOPPED_TYPING}: {} {}",
                crate::go_ahead::REFUSES_GO_AHEAD,
                crate::go_ahead::REFUSES_GO_AHEAD_ACTION
            ),
        }
    }

    /// The one hold that needs a person's action now. All other holds are journal-only.
    #[must_use]
    pub fn ask(&self) -> Option<String> {
        matches!(self, Told::GoAhead { .. }).then(|| {
            format!(
                "Stopped typing: {} {}",
                crate::go_ahead::REFUSES_GO_AHEAD,
                crate::go_ahead::REFUSES_GO_AHEAD_ACTION
            )
        })
    }
}

/// A WALL'S CLASS WORD IN ITS MARK (aterm's `WallKind::name`, pushed raw before the wall's own
/// words): an API's failure of any cause is aterm's `api-error`, a usage window its `usage`
/// (Codex says no session or weekly), a quota its `spend`, a full context `context`, a
/// refused login `auth`; the cyber policy's refusal has no aterm kind and keeps its own.
fn wall_word(wall: &Wall) -> (&'static str, Option<&str>) {
    match wall {
        Wall::Server { what, .. } | Wall::Network { what } | Wall::Overlong { what } => {
            ("api-error", Some(what))
        }
        Wall::Fatal { why } => ("api-error", Some(why)),
        Wall::Usage { reset_label, .. } => ("usage", reset_label.as_deref()),
        Wall::Entitlement => ("spend", None),
        Wall::Context { .. } => ("context", None),
        Wall::Login => ("auth", None),
        Wall::Policy => ("policy", None),
    }
}

/// THE REPLY A TURN SAID (aterm's `reply_mark` over `mark_of`): at a wall, its class word and
/// the wall's own words, the turn's words ignored; else the turn's earlier words, then its
/// last ([`TurnEnd::earlier`], [`TurnEnd::text`], aterm's `turn_said` then `said_tail`). Each
/// word's letters and digits only, case kept, no separator; a word with a digit is one `#`,
/// and a clock's `AM` or `PM` after it is dropped, so a count, a countdown or a clock is the
/// same reply. FNV-1a/64 over the result ([`needs::fnv1a_64`]); an empty reply is one mark.
#[must_use]
pub fn mark_of(end: &TurnEnd) -> u64 {
    let mut letters = String::new();
    let words: [Option<&str>; 2] = match &end.wall {
        Some(wall) => {
            let (word, said) = wall_word(wall);
            letters.push_str(word);
            [said, None]
        }
        None => [end.earlier.as_deref(), end.text.as_deref()],
    };
    let mut number = false;
    for word in words.into_iter().flatten().flat_map(str::split_whitespace) {
        let bare: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
        if word.contains(char::is_numeric) {
            letters.push('#');
            number = true;
        } else if number && (bare.eq_ignore_ascii_case("am") || bare.eq_ignore_ascii_case("pm")) {
            number = false;
        } else {
            letters.push_str(&bare);
            number = false;
        }
    }
    needs::fnv1a_64(letters.as_bytes())
}
