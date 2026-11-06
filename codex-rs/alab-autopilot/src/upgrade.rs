// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The live upgrade's preconditions (CONTRACT §5 row "live upgrade", aterm's `upgrade`): pure.
//! The process half (the trigger, the exec) is `codex_alab_tui::upgrade`; the extension
//! gathers the facts at each verified idle and on each change of the launch path, and acts on
//! [`UpgradeCheck::Go`] only after the before-every-act checks.
//!
//! Preconditions, in order: `upgrade` on; the launch path names another build; under
//! `upgrade_ask` the agent said it is ready to restart, or no thread holds work in hand
//! (amended 2026-10-28 after the parity sweep, gap 27: aterm's cooperative mode,
//! `crate::upgrade_ask`; a session with no task, its task landed, or its point left to a
//! person's own exchange is moved as aterm moves a quiet one, with no notice, as nothing is in
//! hand to ask about or no notice may be typed there; the left point crosses in the record's
//! pace, `codex_alab_core::session::LeftPoint`, edges-5); the session can be resumed exactly
//! (`resumeArgv`, the rollout on disk); every loaded thread idle, with no background terminal;
//! no goal Active; no autopilot wait pending that the record does not carry across the exec (a
//! record write-through tried again, a goal's resume tried again: a wait in memory would be
//! lost by the exec; a paced act's wait is carried, a not-now's too), nor a wall's try's
//! (amended 2026-10-21 after the parity-3 review, F3, F10, F11: its measured hold is this
//! image's, and an older image reads no wall's try); at most one thread with an act to carry;
//! no point codex-alab left to a person (its attention standing: the person is to see that
//! screen, not a restart).
//!
//! THE PACED ACT CROSSES THE RE-EXEC (amended 2026-10-19 for the aterm owner's FOREVER
//! CRUISE / FOREVER LAND ruling, "Cruise is always looking for more completeness and
//! perfection"; this module held the upgrade on every pending timer): under Cruise a paced act
//! (the short streak's back-off, `continue_per_hour`) is almost always pending, and holding
//! the upgrade on it would starve it for good. Such a wait no longer holds it: the image that
//! leaves writes the act it owed into its record (`codex_alab_core::session::OwedAct`: the
//! exact decision, its rule, its words, the Autopilot word it was decided under and its time,
//! so an answer stays the answer with its reversible guard and a Land nudge never becomes a
//! landing; Cruise's done-check words are kept without the repository suffix, which the new
//! image reads afresh when the act is due), or a Land landing, and the new image types that
//! act at its time (`decide::ThreadState::observe_owed`); a session whose word moved since
//! decides the resumed reply's point again under the new word. The new image still resumes
//! with no carry-on (`extension::relaunched`), and a point that was a person's stays theirs: a
//! point left to them is stood at again, left (`decide::ThreadState::seed_left_point`), so Land
//! keeps it theirs and a later switch to Cruise takes it back, as aterm's next loop applies its
//! LEFT record again (edges-5).
//!
//! THE EXEC IS FENCED (review of 2a9241bbf, findings 5 and 9): the extension takes every live
//! thread's evaluation permit before it gathers what to carry, so no evaluation is between its
//! decision and its act while the record is written (one that is makes the upgrade look again
//! shortly), a point no evaluation has decided yet (its evaluation queued behind those permits: a
//! turn end before its idle's look, a look a claim transition or a fired timer asked for, a resume
//! not read yet) holds it too and is looked at again shortly (review of the forever autopilot,
//! 2026-10-20, finding 4), a timer that fired meanwhile leaves its act where the upgrade carries
//! it, an act is never typed once the TUI was asked to leave, and a request the TUI refuses puts
//! the owed act back on its timer. The worker's standing NEEDS YOU are not carried: the exit takes
//! them down, and the new image raises them again from the resumed history
//! (`extension::history_needs`), all but the set an Off took down, which the record names
//! (`needsOff`, `extension::off_still_down`); the record also carries the folder the worker last
//! worked in (`workdir`), which the new image's repository probe reads.

/// The rule an upgrade is recorded as (`harness_act`).
pub const RULE_UPGRADE: &str = "upgrade@v1";

/// One loaded thread, as the upgrade needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedThread {
    pub id: String,
    pub idle: bool,
    pub background_terminals: usize,
    pub goal_active: bool,
}

/// What the upgrade looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeFacts {
    /// `upgrade` on.
    pub enabled: bool,
    /// The launch path names another build than this image runs.
    pub other_build: bool,
    /// UNDER `upgrade_ask`, WHY THE AGENT'S READINESS IS NOT IN HAND (its round's words:
    /// `crate::upgrade_ask`); `None` without the key, once the agent said it is ready, and where
    /// no thread holds work in hand (a task not landed, its point not left to a person's own
    /// exchange). Where the round cannot be read (no policy, the launch path read again naming
    /// no other build) it says so: it fails closed.
    pub unconfirmed: Option<String>,
    /// Why the session cannot safely be resumed exactly, if it cannot.
    pub unresumable: Option<String>,
    pub threads: Vec<LoadedThread>,
    /// An autopilot wait the record does not carry across the exec is pending (a look again
    /// after a record write-through or a goal's resume failed: the exec would lose it), or a
    /// wall's try waits on its rung (the record carries it for aterm's relaunch only: a
    /// measured outage's hold is this image's measure). A paced act's wait is carried (a
    /// not-now's too), and is not this.
    pub waits_pending: bool,
    /// How many threads have something to carry across the re-exec (a paced act owed, a Land
    /// landing): the record carries one.
    pub carried: usize,
    /// A thread whose point no evaluation has decided yet (one queued for its permit, a
    /// resume not read yet): the exec would leave it undecided for good.
    pub undecided: Option<String>,
    /// A point codex-alab left to a person (its standing attention), if one stands.
    pub left_to_person: Option<String>,
}

/// The verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeCheck {
    /// Nothing to do.
    NotDue(&'static str),
    /// Due, but not now: looked at again at the next idle or launch-path change.
    Wait(String),
    /// Every precondition holds (the before-every-act checks still come).
    Go,
}

/// The preconditions, in the contract's order.
pub fn check(facts: &UpgradeFacts) -> UpgradeCheck {
    if !facts.enabled {
        return UpgradeCheck::NotDue("upgrade is off");
    }
    if !facts.other_build {
        return UpgradeCheck::NotDue("the launch path names this build");
    }
    if let Some(why) = &facts.unconfirmed {
        return UpgradeCheck::Wait(format!("upgrade_ask: {why}"));
    }
    if let Some(why) = &facts.unresumable {
        return UpgradeCheck::Wait(format!("the session cannot be resumed yet: {why}"));
    }
    if let Some(t) = facts.threads.iter().find(|t| !t.idle) {
        return UpgradeCheck::Wait(format!("thread {} is not idle", t.id));
    }
    if let Some(t) = facts.threads.iter().find(|t| t.background_terminals > 0) {
        return UpgradeCheck::Wait(format!(
            "thread {} has {} background terminal(s)",
            t.id, t.background_terminals
        ));
    }
    if let Some(t) = facts.threads.iter().find(|t| t.goal_active) {
        return UpgradeCheck::Wait(format!("thread {} has an active goal", t.id));
    }
    if facts.waits_pending {
        return UpgradeCheck::Wait("an autopilot wait is pending".to_string());
    }
    if let Some(t) = &facts.undecided {
        return UpgradeCheck::Wait(format!("thread {t} stands at a point not decided yet"));
    }
    if facts.carried > 1 {
        return UpgradeCheck::Wait(format!(
            "{} threads owe an act; the record carries one",
            facts.carried
        ));
    }
    if let Some(why) = &facts.left_to_person {
        return UpgradeCheck::Wait(format!("a point is left to a person ({why})"));
    }
    UpgradeCheck::Go
}
