// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! What the banner claims against upstream codex, as measured: one row per measurement, with
//! codex-alab's value, upstream's value and the day it was measured.
//!
//! A row has no words of its own. Its [`Measure`] works its words out of its two numbers and
//! says nothing when they do not back a claim, so the banner can only say what a row measured.
//! There is no measure of start-up time, so it never says "faster": cold starts measured the
//! same, and vendor codex with its daemon already running started faster.
//!
//! The rows are the 2026-10-23 head-to-head on one Mac (a 200x50 pty, the owner's own
//! `~/.codex`, the builds alternated run by run, four rounds) of codex-alab
//! 0.165.0-dev.13290+g3087a0371 against vendor codex 0.164.0 as shipped (`codex-old`), which
//! starts its app-server daemon and leaves it running: the independent harness's medians, 70 s
//! after the session was ready (footprint 39.4 MB against 263 MB, ranges 38.3-40.6 and
//! 218-280). Vendor codex run with codex-alab's settings measured 39.2 MB, one process and one
//! connection: the gap is codex-alab's packaged defaults (`alab-core/defaults.toml`), with the
//! daemon, apps, plugins and analytics off. On 2026-10-20, against 0.163.0, the first harness
//! found the same daemon and at least the same footprint ratio, and against a daemon already
//! running vendor codex measured 218 MB. Footprint is `phys_footprint` over the whole process
//! tree, daemon included, rather than RSS, which counts the shared pages of the binary.
//! Connections are the ones still open once the session is idle: both builds open more while
//! they start (on 2026-10-20 codex-alab about 9 in its first minute, all to chatgpt.com; vendor
//! codex about 25), so that row never counts what is opened.
//!
//! These are dated comparisons of default configurations, not the current session's
//! resource usage or guarantees under supported overrides. The renderer keeps that scope
//! beside every visible claim, and omits the figures when its qualification cannot fit.

/// Every claim the banner may make, in the order it draws them.
pub const CLAIMS: &[Claim] = &[
    Claim {
        measure: Measure::FootprintMb,
        alab: 39,
        upstream: 263,
        against: "0.164",
        measured: "2026-10-23",
    },
    Claim {
        measure: Measure::Daemons,
        alab: 0,
        upstream: 1,
        against: "0.164",
        measured: "2026-10-23",
    },
    Claim {
        measure: Measure::IdleConnections,
        alab: 1,
        upstream: 5,
        against: "0.164",
        measured: "2026-10-23",
    },
];

/// What a row measured.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Measure {
    /// Memory footprint of the whole process tree a session runs, in MB.
    FootprintMb,
    /// Background daemons a session starts and leaves running.
    Daemons,
    /// Outbound HTTPS connections still open once the session is idle (`lsof`, 70 s after it
    /// is ready).
    IdleConnections,
}

/// One measurement against upstream codex.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Claim {
    pub measure: Measure,
    /// codex-alab's value.
    pub alab: u32,
    /// Upstream codex's value, the comparison.
    pub upstream: u32,
    /// The upstream codex release measured.
    pub against: &'static str,
    /// The day it was measured.
    pub measured: &'static str,
}

/// One run of a claim's words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Run {
    /// A measured number, which the banner draws bold.
    Figure(String),
    /// The words around it.
    Words(String),
}

impl Claim {
    /// The ways to say what this row backs, longest first; none when its numbers back no claim.
    pub fn forms(&self) -> Vec<Vec<Run>> {
        let figure = |n: u32| Run::Figure(n.to_string());
        let words = |s: &str| Run::Words(s.to_owned());
        let (alab, upstream) = (self.alab, self.upstream);
        match self.measure {
            Measure::FootprintMb if alab < upstream => {
                // A ratio, rounded down, once it is at least 2; a percentage below that.
                let by = if alab > 0 && upstream / alab >= 2 {
                    format!("{}×", upstream / alab)
                } else {
                    format!(
                        "{}%",
                        u64::from(upstream - alab) * 100 / u64::from(upstream)
                    )
                };
                let less = [Run::Figure(by), words(" less memory")];
                let numbers = [figure(alab), words(" vs "), figure(upstream), words(" MB)")];
                let than = Run::Words(format!(" than codex {} (", self.against));
                vec![
                    [&less[..], &[than], &numbers].concat(),
                    [&less[..], &[words(" (")], &numbers].concat(),
                    less.to_vec(),
                ]
            }
            Measure::Daemons if alab == 0 && upstream > 0 => {
                vec![
                    vec![words("no background daemon")],
                    vec![words("no daemon")],
                ]
            }
            Measure::IdleConnections if alab < upstream => {
                let (noun, short) = if alab == 1 {
                    ("connection", "socket")
                } else {
                    ("connections", "sockets")
                };
                vec![
                    vec![
                        figure(alab),
                        Run::Words(format!(" idle {noun} vs ")),
                        figure(upstream),
                    ],
                    vec![
                        figure(alab),
                        Run::Words(format!(" idle {short} vs ")),
                        figure(upstream),
                    ],
                ]
            }
            Measure::FootprintMb | Measure::Daemons | Measure::IdleConnections => Vec::new(),
        }
    }
}
