// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `@<sid> status`, parsed (`aterm ctl help status`): one `OK schema=1 …` line of
//! pct-encoded `key=value` fields, `-` for unset. Fields are additive and never bump the
//! schema, so an unknown schema MAJOR is refused and every unknown field is kept.
//!
//! Every reading here fails closed: a field that is missing or unreadable reads as the
//! state in which codex-alab must not act (held, a hand on the keys, input unread, a
//! person just now, no proven claim).

use std::collections::BTreeMap;
use std::time::Duration;

use codex_alab_core::session::HandPid;

use crate::Error;
use crate::wire;

/// The status schema major this client reads.
pub const STATUS_SCHEMA: u32 = 1;

/// How long ago a person last gave the session input (`human_ms=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanInput {
    /// `-`: no person has, since the session started (or since an update carried it).
    Never,
    /// That many milliseconds ago.
    Ago(u64),
    /// Missing or unreadable.
    Unknown,
}

/// The session's Autopilot word (`autopilot=`): FOREVER CRUISE / FOREVER LAND (aterm owner
/// direction 2026-10-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Autopilot {
    /// `off`: aterm acts on nothing; nor does codex-alab (its policy reads `effective=off`).
    Off,
    /// `cruise`, `-`, missing (an aterm that does not print the word) or a word this reader
    /// does not know: Cruise, which never ends a task on the worker's words.
    Cruise,
    /// `land`: land all of the work into the remote main branch, then end cleanly.
    Land,
}

/// The parsed `status` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// `schema=`'s major.
    pub schema: u32,
    /// `hold=1`, or a missing/unreadable `hold=`: a drive halt stands.
    pub hold: bool,
    /// `hand=`: `None` for `-` (nobody); else whose hand (`turn:<id>[:<holder>]`,
    /// `lease:<holder>`, `driving:<sid>`); a missing field reads `Some("unknown")`.
    pub hand: Option<String>,
    /// `hand_pid=`: the kernel-attested pid of the process that took the lease `hand=`
    /// names ([`HandPid::Absent`] on an aterm that does not print it).
    pub hand_pid: HandPid,
    /// `input=`: `-`/`clear`/`pending`/`typeahead`/`stalled`/`stopped`; missing is `unknown`.
    pub input: String,
    /// `human_ms=`.
    pub human: HumanInput,
    /// `human_typed_ms=`: the person's typing alone (a key, text, a paste), never a scroll or a
    /// click; [`HumanInput::Unknown`] from an aterm that does not print it ([`Status::typed`]).
    pub human_typed: HumanInput,
    /// `human_seq=`.
    pub human_seq: Option<u64>,
    /// `supervisor=`: the live claim's holder.
    pub supervisor: Option<String>,
    /// `supervisor_proven=`: `None` when the field is absent (an aterm without the proven
    /// self-claim, CONTRACT §1), which never counts as proven.
    pub supervisor_proven: Option<bool>,
    /// `program=`: the foreground process-group leader's argv[0] basename.
    pub program: Option<String>,
    /// `agent=`: aterm's own screen verdict.
    pub agent: Option<String>,
    /// `phase=`.
    pub phase: Option<String>,
    /// Every field as sent (still escaped), the ones above included.
    pub fields: BTreeMap<String, String>,
}

/// Why `status` says codex-alab must not act now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotReady {
    /// `hold=1`.
    Hold,
    /// Someone else's hand is on the session.
    Hand(String),
    /// The program has unread or unknown input.
    Input(String),
    /// A person gave input within the grace (or when is unknown).
    Human(HumanInput),
    /// The live claim is not `holder`'s.
    NotOurClaim(Option<String>),
    /// The claim is `holder`'s but aterm does not say it is proven.
    Unproven,
}

impl Status {
    /// THE PERSON'S LAST TYPING (aterm's `human_typed_ms=`, its `human_ms=` from a window that
    /// reports no typing stamp of its own, as aterm's own loop reads it): what holds the owner's
    /// standing go-ahead for the hour after it.
    #[must_use]
    pub fn typed(&self) -> HumanInput {
        match self.human_typed {
            HumanInput::Unknown => self.human,
            typed => typed,
        }
    }

    /// ANOTHER DRIVER'S HAND ON THE TAB (aterm's `outside_hand`): `hand=` names a lease or a
    /// turn whose holder is neither `holder` (this process's) nor a window's own host
    /// (`aterm-harness@…`): a person's drive or another program's, which holds the owner's
    /// standing go-ahead as the person's key does.
    #[must_use]
    pub fn outside_hand(&self, holder: &str) -> bool {
        self.hand.as_deref().is_some_and(|hand| {
            hand.strip_prefix("lease:")
                .or_else(|| {
                    hand.strip_prefix("turn:")
                        .and_then(|turn| turn.split_once(':'))
                        .map(|(_, held)| held)
                })
                .is_some_and(|held| held != holder && !held.starts_with("aterm-harness@"))
        })
    }

    /// Parse a `status` reply's text after `OK `.
    pub fn parse(rest: &str) -> Result<Status, Error> {
        let fields = wire::parse_fields(rest);
        let schema = fields
            .get("schema")
            .and_then(|s| s.split('.').next())
            .and_then(wire::canonical_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| Error::Malformed(format!("status without schema: {rest}")))?;
        if schema != STATUS_SCHEMA {
            return Err(Error::Malformed(format!("status schema {schema}")));
        }
        let hold = fields.get("hold").map(String::as_str) != Some("0");
        let hand = match fields.get("hand").map(String::as_str) {
            Some("-") => None,
            Some(raw) => Some(wire::pct_decode(raw)),
            None => Some("unknown".to_string()),
        };
        let input = fields
            .get("input")
            .map_or_else(|| "unknown".to_string(), |raw| wire::pct_decode(raw));
        let human_input = |field: &str| match fields.get(field).map(String::as_str) {
            Some("-") => HumanInput::Never,
            Some(ms) => wire::canonical_u64(ms).map_or(HumanInput::Unknown, HumanInput::Ago),
            None => HumanInput::Unknown,
        };
        let human = human_input("human_ms");
        let human_typed = human_input("human_typed_ms");
        let supervisor_proven = match fields.get("supervisor_proven").map(String::as_str) {
            Some("1") => Some(true),
            Some(_) => Some(false),
            None => None,
        };
        Ok(Status {
            schema,
            hold,
            hand,
            hand_pid: HandPid::parse(fields.get("hand_pid").map(String::as_str)),
            input,
            human,
            human_typed,
            human_seq: fields.get("human_seq").and_then(|v| wire::canonical_u64(v)),
            supervisor: wire::field(&fields, "supervisor"),
            supervisor_proven,
            program: wire::field(&fields, "program"),
            agent: wire::field(&fields, "agent"),
            phase: wire::field(&fields, "phase"),
            fields,
        })
    }

    /// A field by name, decoded; `None` when absent or `-`.
    pub fn get(&self, key: &str) -> Option<String> {
        wire::field(&self.fields, key)
    }

    /// `autopilot=`, the session's own Autopilot word ([`Autopilot`]).
    pub fn autopilot(&self) -> Autopilot {
        match self.get("autopilot").as_deref().map(str::trim) {
            Some(w) if w.eq_ignore_ascii_case("off") => Autopilot::Off,
            Some(w) if w.eq_ignore_ascii_case("land") => Autopilot::Land,
            _ => Autopilot::Cruise,
        }
    }

    /// `autopilot=` as aterm said it, `None` where it said no word: missing (an older aterm),
    /// `-` (aterm names no agent's word until its status sweep admits the one in the session,
    /// so a codex-alab that asks at launch can read it) or a word this reader does not know.
    pub fn autopilot_said(&self) -> Option<Autopilot> {
        match self.get("autopilot").as_deref().map(str::trim) {
            Some(w) if w.eq_ignore_ascii_case("off") => Some(Autopilot::Off),
            Some(w) if w.eq_ignore_ascii_case("land") => Some(Autopilot::Land),
            Some(w) if w.eq_ignore_ascii_case("cruise") => Some(Autopilot::Cruise),
            _ => None,
        }
    }

    /// THE SESSION'S LAND TIMER (`land_at=<unix s>`, aterm's timed Cruise: `cruise until=`,
    /// its Autopilot menu's land-in and land-at rows), while it is armed; `-`, a missing field
    /// (an aterm before its timers), `0` or a value this reader cannot read names none.
    pub fn land_at(&self) -> Option<u64> {
        wire::canonical_u64(self.get("land_at")?.trim()).filter(|&at| at > 0)
    }

    /// Whether `holder` holds a claim aterm says is proven (CONTRACT §1).
    pub fn proven_holder(&self, holder: &str) -> bool {
        self.supervisor.as_deref() == Some(holder) && self.supervisor_proven == Some(true)
    }

    /// aterm's half of CONTRACT §5 "before every act": `hold=0`, `hand=-` or `holder`'s,
    /// `input=` `-`/`clear`, no person within `grace`, and `holder`'s proven claim. The
    /// TUI's own half (its input stamp, composer, queue, modals) is the caller's.
    pub fn ready_for_act(&self, holder: &str, grace: Duration) -> Result<(), NotReady> {
        if self.hold {
            return Err(NotReady::Hold);
        }
        if let Some(hand) = &self.hand
            && !hand_is(hand, holder)
        {
            return Err(NotReady::Hand(hand.clone()));
        }
        if !matches!(self.input.as_str(), "-" | "clear") {
            return Err(NotReady::Input(self.input.clone()));
        }
        let grace_ms = u64::try_from(grace.as_millis()).unwrap_or(u64::MAX);
        match self.human {
            HumanInput::Never => {}
            HumanInput::Ago(ms) if ms >= grace_ms => {}
            other => return Err(NotReady::Human(other)),
        }
        if self.supervisor.as_deref() != Some(holder) {
            return Err(NotReady::NotOurClaim(self.supervisor.clone()));
        }
        if self.supervisor_proven != Some(true) {
            return Err(NotReady::Unproven);
        }
        Ok(())
    }
}

/// Whether a `hand=` value names `holder` itself (`lease:<holder>`, `turn:<id>:<holder>`).
fn hand_is(hand: &str, holder: &str) -> bool {
    if let Some(h) = hand.strip_prefix("lease:") {
        return h == holder;
    }
    if let Some(rest) = hand.strip_prefix("turn:") {
        return rest.split_once(':').is_some_and(|(_, h)| h == holder);
    }
    false
}

/// `whoami`: `OK <session> <nonce> <scope>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Whoami {
    /// The session the connection is in (`-` when none).
    pub session: String,
    /// `owner`, `edge`, …
    pub scope: String,
}

impl Whoami {
    /// Parse the text after `OK `. The nonce is not kept.
    pub fn parse(rest: &str) -> Result<Whoami, Error> {
        let mut toks = rest.split_ascii_whitespace();
        match (toks.next(), toks.next(), toks.next()) {
            (Some(session), Some(_nonce), Some(scope)) => Ok(Whoami {
                session: session.to_string(),
                scope: scope.to_string(),
            }),
            _ => Err(Error::Malformed(format!("whoami: {rest}"))),
        }
    }

    /// Whether the connection holds the instance token (Owner scope).
    pub fn is_owner(&self) -> bool {
        self.scope == "owner"
    }
}
