// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ATERM'S MEASURED FACT FOR AN ACT CODEX-ALAB TYPES ANYWAY (CONTRACT §5 "aterm fact"; amended
//! after the parity sweep, gap 23: a hung background command in a codex tab went unmentioned).
//!
//! `@<sid> harness fact for=<rule> reader=<id> kinds=no-progress wall=<word|->`, on the claim's
//! connection, asks aterm for the session's untold fact for an act under `<rule>`. aterm keeps the
//! whole account (which episode, its freshness, once per episode, the lease of an offered episode
//! to `<id>`, the policy gate) and judges every read itself. It answers `OK fact=- why=<word>`
//! or `OK fact=<pct sentence> kind=no-progress episode=<016x> as_of_ms=<unix ms> rider=<pct
//! ' (aterm: <sentence>)'>`; any `ERR` is no fact. `@<sid> harness fact told episode=<016x>` says
//! the act that carried the rider started and was recorded: `OK`, `ERR stale fact` or `ERR
//! denied`.
//!
//! [`FactAnswer::parse`] takes a fact only where its frame and its sentence check out, so a
//! planted or malformed rider is never typed (gap 23's R6), and tells an aterm before the verb
//! (its `harness` usage, naming no `fact` form) from every other refusal, which is no fact this
//! time only.

use std::collections::BTreeMap;

use crate::wire;

/// How a fact opens after the act it rides (aterm's `policy::turn_end::FACT_OPEN`).
pub const FACT_OPEN: &str = " (aterm: ";

/// The longest fact sentence, in characters (aterm's `supervise::facts::FACT_CAP`).
pub const FACT_CAP: usize = 200;

/// How old the reading a fact comes from may be when it is typed, ms (aterm's
/// `supervise::facts::FACT_FRESH_MS`).
pub const FACT_FRESH_MS: i64 = 5_000;

/// The one kind this contract reads (`kinds=`; CONTRACT §5 "aterm fact").
pub const FACT_NO_PROGRESS: &str = "no-progress";

/// A fact aterm offered for one act.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// Its kind ([`FACT_NO_PROGRESS`]).
    pub kind: String,
    /// Its episode, the key the ack names.
    pub episode: u64,
    /// aterm's sentence, decoded.
    pub text: String,
    /// What the act carries after its words and standing rules: [`rider`] of [`Self::text`],
    /// exactly as aterm sent it.
    pub rider: String,
    /// When aterm's reading was taken, unix ms on this host's clock.
    pub as_of_ms: i64,
}

impl Fact {
    /// Whether the reading may still be told at `now_ms` (aterm's `FactsReading::tells_at`): at
    /// most [`FACT_FRESH_MS`] old. Read as the last step before the act's turn request.
    #[must_use]
    pub fn tells_at(&self, now_ms: i64) -> bool {
        now_ms.saturating_sub(self.as_of_ms) <= FACT_FRESH_MS
    }
}

/// The words a fact adds to the act it rides (aterm's `fact_suffix`): ` (aterm: <text>)`.
#[must_use]
pub fn rider(text: &str) -> String {
    format!("{FACT_OPEN}{text})")
}

/// What `harness fact for=…` answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactAnswer {
    /// A fact this reader can type.
    Fact(Fact),
    /// `OK fact=- why=<word>`: none for this act (its `why`, decoded).
    None(String),
    /// `ERR denied`: aterm proves no codex-alab self-claim of this connection's here.
    Denied(String),
    /// Any other `ERR` (a dialog, a stalled main thread, a refusal): no fact this time.
    Failed(String),
    /// An `OK` this reader cannot take (an unknown kind, a bad frame or sentence), or a reply
    /// that is neither `OK` nor `ERR`: never typed, never acknowledged, never latched.
    Unusable(String),
    /// An aterm before the verb: its `harness` usage, naming no `fact` form (`ERR usage: harness
    /// policy`, `ERR usage: harness policy | recovery`). Not asked again under this claim.
    Unsupported(String),
    /// Not sent: no claim is held on this connection.
    NotHeld,
}

/// What `harness fact told episode=…` answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToldOutcome {
    /// `OK`: aterm marked the episode told.
    Told,
    /// `ERR denied` or `ERR stale fact`: aterm holds no offer of it to this claim; it stays
    /// untold there.
    Refused(String),
    /// Any other answer: nothing is known to be told.
    Failed(String),
    /// Not sent: no claim is held on this connection.
    NotHeld,
}

/// A rule id as `for=` takes it: `[a-z0-9-]+@v[0-9]+`.
#[must_use]
pub fn valid_rule(rule: &str) -> bool {
    rule.split_once("@v").is_some_and(|(name, version)| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !version.is_empty()
            && version.bytes().all(|b| b.is_ascii_digit())
    })
}

/// A reader as `reader=` takes it: 1 to 64 of `[A-Za-z0-9_-]`.
#[must_use]
pub fn valid_reader(reader: &str) -> bool {
    (1..=64).contains(&reader.len())
        && reader
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

/// A wall as `wall=` takes it: `-` (no wall's track stands), or one lowercase word of at most 32
/// letters (a wall class's word: `usage`, `entitlement`, `server`, …).
#[must_use]
pub fn valid_wall(wall: &str) -> bool {
    wall == "-" || ((1..=32).contains(&wall.len()) && wall.bytes().all(|b| b.is_ascii_lowercase()))
}

/// An episode as the wire spells it: exactly 16 lowercase hex digits.
#[must_use]
pub fn episode_of(raw: &str) -> Option<u64> {
    if raw.len() != 16 || !raw.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    u64::from_str_radix(raw, 16).ok()
}

/// Whether `text` is a sentence this reader types: 1 to [`FACT_CAP`] characters, none of them a
/// control character or a bidi or invisible formatting one ([`wire::is_format_char`]).
#[must_use]
pub fn sentence_ok(text: &str) -> bool {
    (1..=FACT_CAP).contains(&text.chars().count())
        && !text
            .chars()
            .any(|c| c.is_control() || wire::is_format_char(c))
}

/// Whether `status` is a `harness` usage line that names no `fact` form: an aterm before the
/// verb.
fn older_usage(status: &str) -> bool {
    let Some(forms) = status.strip_prefix("ERR usage: harness") else {
        return false;
    };
    (forms.is_empty() || forms.starts_with(' '))
        && !forms
            .split('|')
            .any(|form| form.split_ascii_whitespace().next() == Some("fact"))
}

/// The `key=value` tokens of a reply, raw, every token a `key=value` and every key once; `None`
/// otherwise.
fn strict_fields(rest: &str) -> Option<BTreeMap<&str, &str>> {
    let mut fields = BTreeMap::new();
    for token in rest.split_ascii_whitespace() {
        let (key, value) = token.split_once('=')?;
        if fields.insert(key, value).is_some() {
            return None;
        }
    }
    Some(fields)
}

impl FactAnswer {
    /// Read a `harness fact for=…` reply's status line.
    #[must_use]
    pub fn parse(status: &str) -> FactAnswer {
        if let Some(rest) = status.strip_prefix("OK ") {
            return Self::parse_ok(status, rest);
        }
        if status == "ERR" || status.starts_with("ERR ") {
            return if older_usage(status) {
                FactAnswer::Unsupported(status.to_string())
            } else if crate::client::denied(status) {
                FactAnswer::Denied(status.to_string())
            } else {
                FactAnswer::Failed(status.to_string())
            };
        }
        FactAnswer::Unusable(status.to_string())
    }

    fn parse_ok(status: &str, rest: &str) -> FactAnswer {
        let unusable = || FactAnswer::Unusable(status.to_string());
        let Some(fields) = strict_fields(rest) else {
            return unusable();
        };
        let Some(raw_text) = fields.get("fact").copied() else {
            return unusable();
        };
        if raw_text == "-" {
            return match fields.get("why").copied() {
                None => FactAnswer::None("-".to_string()),
                Some(why) => match wire::pct_decode_strict(why) {
                    Some(why) => FactAnswer::None(why),
                    None => unusable(),
                },
            };
        }
        let kind = fields.get("kind").copied();
        let episode = fields.get("episode").copied().and_then(episode_of);
        let as_of_ms = fields
            .get("as_of_ms")
            .copied()
            .and_then(wire::canonical_u64)
            .and_then(|ms| i64::try_from(ms).ok());
        let text = wire::pct_decode_strict(raw_text).filter(|text| sentence_ok(text));
        let said = fields
            .get("rider")
            .copied()
            .and_then(wire::pct_decode_strict);
        match (kind, episode, as_of_ms, text, said) {
            (Some(FACT_NO_PROGRESS), Some(episode), Some(as_of_ms), Some(text), Some(said))
                if said == rider(&text) =>
            {
                FactAnswer::Fact(Fact {
                    kind: FACT_NO_PROGRESS.to_string(),
                    episode,
                    text,
                    rider: said,
                    as_of_ms,
                })
            }
            _ => unusable(),
        }
    }
}

impl ToldOutcome {
    /// Read a `harness fact told episode=…` reply's status line.
    #[must_use]
    pub fn parse(status: &str) -> ToldOutcome {
        if status == "OK" || status.starts_with("OK ") {
            return ToldOutcome::Told;
        }
        let stale = status
            .strip_prefix("ERR stale fact")
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', ':']));
        if stale || crate::client::denied(status) {
            ToldOutcome::Refused(status.to_string())
        } else {
            ToldOutcome::Failed(status.to_string())
        }
    }
}
