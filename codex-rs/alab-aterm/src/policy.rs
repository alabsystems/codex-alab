// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `@<sid> harness policy` (CONTRACT §3): aterm's `[harness]` policy for this session,
//! the one reader, so codex-alab never parses aterm.toml.
//!
//! `OK gen=<cfg_gen> effective=<on|off> why=<word> <key>=<pct value>…`, every key in
//! `SupervisorConfig`'s own spelling, `continue_text=` already composed with the rules
//! file and `questions=` already resolved. An aterm that answers `ERR` (an older build:
//! `ERR unknown verb (try: help)`) or anything this reader cannot take is
//! [`PolicyAnswer::Unsupported`]: codex-alab stays passive and never claims.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::wire;

/// THE TEXTS aterm's own loop would type for this session under FOREVER CRUISE / FOREVER
/// LAND, each composed with the standing rules as aterm composes it (additive keys after
/// `questions=`): the Cruise defend prompt without its per-point suffixes (its first angle's
/// act), the land instruction, the land nudge, the land answer, the NEEDS YOU hint and its ack;
/// then, bare, Cruise's six angles one to a line and the words that follow each (aterm's
/// `ACT_TEXT_KEYS`; an older aterm sends neither). [`HarnessPolicy::text`] reads one.
pub const TEXT_KEYS: [&str; 8] = [
    "done_check_text",
    "land_text",
    "land_nudge_text",
    "land_answer_text",
    "needs_hint_text",
    "needs_ack_text",
    "done_check_angles",
    "done_check_tail",
];

/// What `harness policy` answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyAnswer {
    /// A new-enough aterm's policy.
    Supported(HarnessPolicy),
    /// `ERR …`, an unknown verb, or an `OK` this reader cannot take: the line as sent.
    Unsupported(String),
}

/// How the session's question boxes are answered (`questions=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Questions {
    /// Left to a person (with attention). Also any word this reader does not know.
    Ask,
    /// The recommended option.
    Recommended,
    /// The worker decides for long-term correctness.
    LongTerm,
    /// The worker decides for the current deliverable.
    ShortTerm,
}

/// How much codex-alab may approve (`approve=`). `safe` reads as [`Approve::None`]:
/// codex-alab has no proofs (CONTRACT §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approve {
    /// Every box's one-shot allow.
    All,
    /// Nothing; every box goes to a person.
    None,
}

/// `approve=` as aterm wrote it, `safe` kept: the level aterm's own rules read. The folder
/// trust answer (`codex_alab_tui::trust`) takes a trust root under `safe` or `all`, as aterm's
/// lane does; everything that approves a box reads [`HarnessPolicy::approve`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApproveLevel {
    /// `none`, missing, or a word this reader does not know.
    None,
    /// `safe`.
    Safe,
    /// `all`.
    All,
}

impl ApproveLevel {
    /// aterm's spelling.
    pub fn word(self) -> &'static str {
        match self {
            ApproveLevel::None => "none",
            ApproveLevel::Safe => "safe",
            ApproveLevel::All => "all",
        }
    }
}

/// A supported `harness policy` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessPolicy {
    /// The policy cell's generation; `harness-policy gen=<n>` events name it.
    pub generation: u64,
    /// `effective=on`, computed by aterm for this session. Only `on` is on.
    pub effective: bool,
    /// Why (`why=`), decoded.
    pub why: String,
    /// Every other key, decoded.
    pub keys: BTreeMap<String, String>,
}

impl PolicyAnswer {
    /// Read a `harness policy` reply's status line.
    pub fn parse(line: &str) -> PolicyAnswer {
        let Some(rest) = line.strip_prefix("OK ") else {
            return PolicyAnswer::Unsupported(line.to_string());
        };
        let mut fields = wire::parse_fields(rest);
        let generation = fields.remove("gen").and_then(|g| wire::canonical_u64(&g));
        let effective = fields.remove("effective");
        let (Some(generation), Some(effective)) = (generation, effective) else {
            return PolicyAnswer::Unsupported(line.to_string());
        };
        let why = fields
            .remove("why")
            .map(|w| wire::pct_decode(&w))
            .unwrap_or_default();
        let keys = fields
            .into_iter()
            .map(|(k, v)| {
                let v = if v == "-" {
                    String::new()
                } else {
                    wire::pct_decode(&v)
                };
                (k, v)
            })
            .collect();
        PolicyAnswer::Supported(HarnessPolicy {
            generation,
            effective: effective == "on",
            why,
            keys,
        })
    }
}

impl HarnessPolicy {
    /// A key's decoded value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.keys.get(key).map(String::as_str)
    }

    /// A switch: on only when aterm printed `true`; anything else is the limit (off).
    pub fn switch(&self, key: &str) -> bool {
        self.get(key) == Some("true")
    }

    /// `continue_text=`, composed by aterm with the rules file.
    pub fn continue_text(&self) -> Option<&str> {
        self.get("continue_text").filter(|t| !t.trim().is_empty())
    }

    /// `answer_text=`.
    pub fn answer_text(&self) -> Option<&str> {
        self.get("answer_text").filter(|t| !t.trim().is_empty())
    }

    /// One of [`TEXT_KEYS`], as aterm composed it; `None` where it is missing or blank (an
    /// aterm before the keys): the caller types its verbatim copy of aterm's words instead.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.get(key).filter(|t| !t.trim().is_empty())
    }

    /// `questions=`, resolved by aterm; unknown or missing reads [`Questions::Ask`].
    pub fn questions(&self) -> Questions {
        match self.get("questions").map(str::trim) {
            Some(w) if w.eq_ignore_ascii_case("recommended") => Questions::Recommended,
            Some(w) if w.eq_ignore_ascii_case("long-term") => Questions::LongTerm,
            Some(w) if w.eq_ignore_ascii_case("short-term") => Questions::ShortTerm,
            _ => Questions::Ask,
        }
    }

    /// `approve=`: only `all` approves.
    pub fn approve(&self) -> Approve {
        match self.get("approve").map(str::trim) {
            Some(w) if w.eq_ignore_ascii_case("all") => Approve::All,
            _ => Approve::None,
        }
    }

    /// `approve=` with `safe` kept ([`ApproveLevel`]); unknown or missing is `none`.
    pub fn approve_level(&self) -> ApproveLevel {
        match self.get("approve").map(str::trim) {
            Some(w) if w.eq_ignore_ascii_case("all") => ApproveLevel::All,
            Some(w) if w.eq_ignore_ascii_case("safe") => ApproveLevel::Safe,
            _ => ApproveLevel::None,
        }
    }

    /// `trust_roots=`, comma-joined.
    pub fn trust_roots(&self) -> Vec<String> {
        self.get("trust_roots")
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `continue_per_hour=`: `None` for no cap (`0`). Unreadable is a cap of 0 continues.
    pub fn continue_per_hour(&self) -> Option<u32> {
        match self
            .get("continue_per_hour")
            .and_then(|v| wire::canonical_u64(v.trim()))
            .and_then(|n| u32::try_from(n).ok())
        {
            Some(0) => None,
            Some(n) => Some(n),
            None => Some(0),
        }
    }

    /// `human_grace_s=`; missing or unreadable is aterm's own limit, an hour.
    pub fn human_grace(&self) -> Duration {
        let secs = self
            .get("human_grace_s")
            .and_then(|v| wire::canonical_u64(v.trim()))
            .unwrap_or(3600);
        Duration::from_secs(secs)
    }
}
