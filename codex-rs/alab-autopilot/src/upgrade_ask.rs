// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE LIVE UPGRADE ASKS FIRST under aterm's `[harness] upgrade_ask = true` (CONTRACT §5 row
//! "live upgrade", amended 2026-10-28 after the parity sweep, gap 27: codex-alab read the key
//! as the upgrade's off, so a codex-alab tab was never asked and never moved to a new build,
//! where a Claude Code tab is asked and moves once it confirms): pure. aterm's cooperative mode
//! (`aterm harness upgrade --help`, MODES; `harness::upgrade::next_step`'s announce, re-ask and
//! give-up arms, `clock_held`, `retry_due` and `unanswered_rest_extension`, and the drive's
//! `give_up` and `rearm`, aterm main `433715d26d`): the notice ([`crate::words::upgrade_notice`])
//! is typed at a due upgrade, again every [`REASK`] while it goes unanswered, up to
//! [`MAX_ASKS`] notices; [`REASK`] after the last the round gives up and rests (2 h, then 4 h,
//! 8 h, and 24 h from the fourth unanswered round in a row: [`unanswered_rest`]), then asks
//! again. THE AGENT'S LATEST REPLY SAYS WHETHER IT IS READY (aterm's `transcript_has_ready`
//! over its `answers`): each turn of the root thread that ends clean is heard
//! ([`Round::heard`]); a line that is the latest notice's reference alone says ready in any
//! reply, and ordinary language ([`crate::upgrade_readiness::says_ready_to`]) in a reply while
//! nothing but codex-alab's own acts came since the notice (aterm's `matching_notice`: a
//! native automatic turn is no direction), a late one after the round gave up included. A
//! ready reply lets the re-exec go once its own preconditions hold (`crate::upgrade::check`);
//! a message that is not codex-alab's own (a person's: aterm's `Turn::Direction`) supersedes
//! it, and the round asks again at once (aterm's `ready_superseded`, `reask_superseded`). A
//! notice whose turn met a login wall or a usage limit never reached the agent: its ask is
//! given back (aterm's `undelivered`, its `NoticeFate::Wall` and `Queued`: [`withheld_by`]);
//! one that met another wall stands, answered by the reply to codex-alab's try. The round
//! times its re-asks and rests in real time, the host's sleep counted ([`Round::slept_to`];
//! aterm's drive reads unix seconds). While the root thread's point stands at a wall its
//! re-ask clock is held (aterm's `clock_held` at a usage limit or a login), so no re-ask comes
//! due and no round gives up then; a given-up round's rest runs on, as aterm's does, and no
//! notice is typed at a wall (`decide::ThreadState::upgrade_notice`). An Off holds nothing, as
//! aterm's does not: codex-alab, passive, does not look at the round, and its first look after
//! the Off reads the time that passed.
//!
//! The runtime keeps one round for the process (the upgrade is the process's) and decides where
//! a notice goes (`decide::ThreadState::upgrade_notice`: a clean point of the root thread on
//! screen, never at a wall); a new target build begins a new round. Not ported: aterm's
//! escalation ladder (its rungs only narrow its screen's settle and a person's grace, which the
//! before-every-act checks keep whole here), the running clause (aterm names the processes it
//! sees under the agent; Codex's background terminals hold the re-exec itself), the release line
//! after an abandoned notice, a stale READY's drain (30 min), and the owner's `--now`, `--defer`
//! and `--skip`, which aterm's own drive reads.

use std::time::Duration;
use std::time::Instant;

/// How long a notice waits for its answer before the next (aterm's `REASK_S`).
pub const REASK: Duration = Duration::from_secs(30 * 60);
/// The notices of one round (aterm's `MAX_ASKS`).
pub const MAX_ASKS: u32 = 4;
/// A round's rest after it gave up (aterm's `RETRY_S`: `MAX_ASKS` re-asks).
pub const RETRY: Duration = Duration::from_secs(MAX_ASKS as u64 * 30 * 60);

/// THE REST AFTER THE `streak`-TH UNANSWERED ROUND IN A ROW (aterm's `RETRY_S` plus its
/// `unanswered_rest_extension`): 2 h, 4 h, 8 h, then 24 h.
#[must_use]
pub fn unanswered_rest(streak: u32) -> Duration {
    let multiples: u32 = match streak {
        0 | 1 => 0,
        2 => 1,
        3 => 3,
        _ => 11,
    };
    RETRY.saturating_mul(1 + multiples)
}

/// A WALL THE NOTICE NEVER GOT PAST (aterm's `notice_scan`: `NoticeFate::Wall`, a login wall,
/// and `Queued`, a usage limit, its spend cap and model bucket among them): a login, a usage
/// window, a quota or a plan without the model. At another wall (a server's, the network's, a
/// full context) the notice stands, and the reply to codex-alab's try answers it, as aterm
/// hears the reply to its harness retry.
#[must_use]
pub fn withheld_by(wall: &crate::decide::Wall) -> bool {
    use crate::decide::Wall;
    matches!(wall, Wall::Login | Wall::Usage { .. } | Wall::Entitlement)
}

/// What the round does now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskStep {
    /// A notice is due: the root thread's next clean point types it.
    Notice,
    /// The agent said it is ready: the re-exec goes once its own preconditions hold.
    Ready,
    /// Nothing until `until`: `why` in a person's words.
    Wait { until: Instant, why: String },
}

/// One round of notices for one target build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Round {
    /// The build the launch path names, as the notices name it.
    target: Option<String>,
    /// Notices typed this round (aterm's `Phase::Announced { asks }`).
    asks: u32,
    /// When the latest notice was typed, moved on while the clock is held and back by the
    /// host's sleep.
    asked_at: Option<Instant>,
    /// The latest notice's reference: only an answer to it counts.
    marker: Option<String>,
    /// The reference of the notice offered to a decision and not typed yet.
    offered: Option<String>,
    /// NOTHING BUT CODEX-ALAB'S OWN ACTS CAME SINCE THE LATEST NOTICE (aterm's
    /// `matching_notice`): the agent's replies answer it in ordinary language.
    matching: bool,
    /// The agent's latest reply after the latest notice said ready.
    ready: bool,
    /// The next notice is due at once (a readiness superseded, a notice given back).
    reask_now: bool,
    /// When the round gave up (aterm's `Phase::Failed(GAVE_UP)`).
    gave_up_at: Option<Instant>,
    /// Unanswered rounds in a row (aterm's `stop_streak` under `GAVE_UP`).
    streak: u32,
    /// Rounds begun in this process: each notice's reference differs.
    rounds: u64,
    /// The due notice the runtime last woke a look for ([`Round::wake_due`]).
    woke: Option<(u64, u32, bool)>,
    /// The host's time asleep since boot at the round's last reading ([`Round::slept_to`]).
    asleep_mark: Option<Duration>,
}

impl Round {
    /// The round asks about `target`: another build than the round's begins a new round.
    pub fn aim(&mut self, target: &str) {
        if self.target.as_deref() == Some(target) {
            return;
        }
        *self = Round {
            target: Some(target.to_string()),
            rounds: self.rounds.wrapping_add(1),
            ..Round::default()
        };
    }

    /// The build the round asks about.
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// Notices typed this round.
    #[must_use]
    pub fn asks(&self) -> u32 {
        self.asks
    }

    /// The agent's latest reply after the latest notice said ready.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.ready
    }

    /// WHAT THE ROUND DOES AT `now`: it gives up after the last notice's [`REASK`], and asks
    /// again once its rest is over. `held` (the root thread's point stands at a wall) holds the
    /// latest notice's clock at `now`, so no re-ask comes due and no round gives up; a given-up
    /// round's rest runs on (aterm's `clock_held` moves only an announced phase's time).
    pub fn step(&mut self, now: Instant, held: bool) -> AskStep {
        if self.ready {
            return AskStep::Ready;
        }
        if held && let Some(at) = self.asked_at {
            self.asked_at = Some(at.max(now));
        }
        if let Some(at) = self.gave_up_at {
            let rest = unanswered_rest(self.streak);
            let until = at + rest;
            if now < until {
                return AskStep::Wait {
                    until,
                    why: format!(
                        "{MAX_ASKS} upgrade notices went unanswered; asking again after a {} h rest",
                        rest.as_secs() / 3600
                    ),
                };
            }
            self.gave_up_at = None;
            self.asks = 0;
            self.asked_at = None;
            self.marker = None;
            self.matching = false;
            self.reask_now = false;
            self.rounds = self.rounds.wrapping_add(1);
        }
        let Some(at) = self.asked_at.filter(|_| self.asks > 0) else {
            return AskStep::Notice;
        };
        let due = at + REASK;
        if !self.reask_now && now < due {
            return AskStep::Wait {
                until: due,
                why: format!(
                    "waiting for the agent to say it is ready to restart (notice {} of {MAX_ASKS})",
                    self.asks
                ),
            };
        }
        if self.asks < MAX_ASKS {
            return AskStep::Notice;
        }
        self.streak = self.streak.saturating_add(1);
        self.gave_up_at = Some(now);
        self.reask_now = false;
        let rest = unanswered_rest(self.streak);
        AskStep::Wait {
            until: now + rest,
            why: format!(
                "{MAX_ASKS} upgrade notices went unanswered; asking again after a {} h rest",
                rest.as_secs() / 3600
            ),
        }
    }

    /// THE RUNTIME LOOKS ONCE FOR EACH NOTICE THAT COMES DUE: `true` the first time
    /// [`Round::step`] says [`AskStep::Notice`] for this round's count, so a point that types
    /// none is not looked at again on every check.
    pub fn wake_due(&mut self, now: Instant, held: bool) -> bool {
        if self.step(now, held) != AskStep::Notice {
            return false;
        }
        let due = (self.rounds, self.asks, self.reask_now);
        if self.woke == Some(due) {
            return false;
        }
        self.woke = Some(due);
        true
    }

    /// THE REFERENCE OF THE NOTICE OFFERED NOW to the decision of thread `session` (aterm's
    /// `ready_marker`: FNV-1a/64 of the session, the target and a salt, its low 32 bits), kept
    /// until it is typed ([`Round::noticed`]).
    pub fn offer(&mut self, session: &str) -> String {
        let salt = (u64::from(std::process::id()) << 32)
            | ((self.rounds & 0x00ff_ffff) << 8)
            | u64::from(self.asks.wrapping_add(1) & 0xff);
        let mut bytes = Vec::with_capacity(session.len() + 64);
        bytes.extend_from_slice(session.as_bytes());
        bytes.extend_from_slice(self.target.as_deref().unwrap_or_default().as_bytes());
        bytes.extend_from_slice(&salt.to_le_bytes());
        let marker = format!(
            "{}{:08x}",
            crate::words::UPGRADE_READY_PREFIX,
            crate::needs::fnv1a_64(&bytes) & 0xffff_ffff
        );
        self.offered = Some(marker.clone());
        marker
    }

    /// A notice went in at `now` with `text`: counted where it carries the reference offered.
    pub fn noticed(&mut self, text: &str, now: Instant) {
        let Some(marker) = self.offered.take().filter(|m| text.contains(m.as_str())) else {
            return;
        };
        self.asks = self.asks.saturating_add(1);
        self.asked_at = Some(now);
        self.marker = Some(marker);
        self.matching = true;
        self.ready = false;
        self.reask_now = false;
    }

    /// The notice's turn did not start: the round stands as it did before [`Round::noticed`]
    /// (`before`), unless another notice or round came since; the looks and the sleep counted
    /// since stay counted.
    pub fn withdraw(&mut self, before: Round) {
        if self.target == before.target
            && self.rounds == before.rounds
            && self.asks == before.asks.saturating_add(1)
        {
            *self = Round {
                woke: self.woke,
                asleep_mark: self.asleep_mark,
                ..before
            };
        }
    }

    /// A TURN OF THE ROOT THREAD ENDED CLEAN with `words` (its last message), the notice's own
    /// or any after it (aterm's `transcript_has_ready`: the agent's last reply after the latest
    /// notice decides): ready where a line is the notice's reference alone, or, while nothing but
    /// codex-alab's own acts came since the notice, where they say so in ordinary language
    /// (aterm's `answers` with `matching_notice`); not ready otherwise, though no re-ask comes
    /// due before the notice's [`REASK`].
    pub fn heard(&mut self, words: Option<&str>) {
        let Some(marker) = self.marker.as_deref() else {
            return;
        };
        self.ready = words.is_some_and(|w| {
            crate::upgrade_readiness::names_reference(w, marker)
                || (self.matching && crate::upgrade_readiness::says_ready_to(w, marker))
        });
    }

    /// The latest notice's turn met a wall the notice never got past ([`withheld_by`]): its ask
    /// is given back, to be typed again at the next clean point (aterm's `undelivered`).
    pub fn undelivered(&mut self) {
        if self.marker.is_none() || self.ready {
            return;
        }
        self.asks = self.asks.saturating_sub(1);
        self.reask_now = true;
    }

    /// A MESSAGE THAT IS NOT CODEX-ALAB'S OWN came into the root thread (a person's: aterm's
    /// `Turn::Direction`; codex-alab's own automatic acts are
    /// none): the agent's later replies answer the notice by its reference alone, and a
    /// readiness it had said is superseded, the round asking again at once (aterm's
    /// `ready_superseded`, `reask_superseded`).
    pub fn directed(&mut self) {
        self.matching = false;
        if self.ready {
            self.ready = false;
            self.reask_now = true;
        }
    }

    /// THE HOST SLEPT (aterm's drive counts the round in unix seconds, `now_s`): `asleep` is the
    /// host's time asleep since boot as read now ([`codex_alab_core::wake::Reading::asleep`]).
    /// The round's clock is the `Instant` one, which stops while the host sleeps, so the latest
    /// notice's time and the give-up's move back by the time slept since the last reading: a
    /// re-ask or a rest the host slept through is due at the wake. The first reading only marks;
    /// a growth under [`codex_alab_core::wake::MIN_SLEEP`] is the two clocks read a moment apart,
    /// and moves nothing. A time the clock cannot move back that far stays where it is.
    pub fn slept_to(&mut self, asleep: Option<Duration>, now: Instant) {
        let Some(asleep) = asleep else {
            return;
        };
        let Some(mark) = self.asleep_mark.replace(asleep) else {
            return;
        };
        let Some(slept) = asleep
            .checked_sub(mark)
            .filter(|slept| *slept >= codex_alab_core::wake::MIN_SLEEP)
        else {
            return;
        };
        let back = |at: Instant| at.checked_sub(slept).unwrap_or(now.min(at));
        self.asked_at = self.asked_at.map(back);
        self.gave_up_at = self.gave_up_at.map(back);
    }
}
