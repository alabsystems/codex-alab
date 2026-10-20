// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The live upgrade's preconditions (CONTRACT §5 row "live upgrade", aterm's `upgrade` with
//! `upgrade_ask=false`): pure. The process half (the trigger, the exec) is
//! `codex_alab_tui::upgrade`; the extension gathers the facts at each verified idle and on
//! each change of the launch path, and acts on [`UpgradeCheck::Go`] only after the
//! before-every-act checks.
//!
//! Preconditions, in order: `upgrade` on (and `upgrade_ask` off); the launch path names
//! another build; the session can be resumed exactly (`resumeArgv`, the rollout on disk);
//! every loaded thread idle, with no background terminal; no goal Active; no autopilot
//! timer pending (a wait in memory would be lost by the exec); no point codex-alab left to
//! a person (its attention standing: the person is to see that screen, not a restart).
//!
//! The new image resumes every thread with no carry-on (`extension::relaunched`): at a
//! quiescent point the old image owed no act (an owed one is typed, or pending as a timer,
//! which holds the upgrade), so the new image owes none either, and a point that was a
//! person's stays theirs.

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
    /// `upgrade` on and `upgrade_ask` off.
    pub enabled: bool,
    /// The launch path names another build than this image runs.
    pub other_build: bool,
    /// Why the session cannot be resumed exactly, if it cannot.
    pub unresumable: Option<String>,
    pub threads: Vec<LoadedThread>,
    pub timers_pending: bool,
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
        return UpgradeCheck::NotDue("upgrade is off (or upgrade_ask is on)");
    }
    if !facts.other_build {
        return UpgradeCheck::NotDue("the launch path names this build");
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
    if facts.timers_pending {
        return UpgradeCheck::Wait("an autopilot wait is pending".to_string());
    }
    if let Some(why) = &facts.left_to_person {
        return UpgradeCheck::Wait(format!("a point is left to a person ({why})"));
    }
    UpgradeCheck::Go
}
