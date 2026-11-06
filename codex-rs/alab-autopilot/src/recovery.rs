// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An unresolved provider wait crosses a process boundary as a failed turn and absolute
//! deadlines. It becomes a fresh decision, never an already-authorized prompt replay.

use crate::decide::Act;
use crate::decide::Decision;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use codex_alab_core::session::Recovery;
use codex_alab_core::session::RecoveryWall;
use codex_history::RolloutItem;
use std::time::Duration;
use std::time::Instant;

pub(crate) fn epoch_of(at: Instant, now: Instant, epoch: u64) -> u64 {
    if at > now {
        let wait = at.duration_since(now);
        let millis = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        // `epoch` is a floored millisecond sample. Round it up independently of
        // rounding the wait, so a future lower bound never moves before its target.
        epoch.saturating_add(
            millis
                .saturating_add(u64::from(!wait.subsec_nanos().is_multiple_of(1_000_000)))
                .saturating_add(1),
        )
    } else {
        epoch.saturating_sub(u64::try_from(now.duration_since(at).as_millis()).unwrap_or(u64::MAX))
    }
}

pub(crate) fn instant_of(ms: u64, now: Instant, epoch: u64) -> Option<Instant> {
    if ms >= epoch {
        now.checked_add(Duration::from_millis(ms - epoch))
    } else {
        Some(
            now.checked_sub(Duration::from_millis(epoch - ms))
                .unwrap_or(now),
        )
    }
}

/// The recovery intent a wall's `decision` at `state`'s failed point leaves, where it can be
/// decided again; `measured` where the wall's route was measured (a network wall's,
/// [`RecoveryWall::Network`]).
pub(crate) fn capture(
    thread: &str,
    turn_id: &str,
    state: &ThreadState,
    decision: &Decision,
    measured: bool,
    now: Instant,
    epoch: u64,
) -> Option<Recovery> {
    let point = state.point.as_ref().filter(|point| !point.aborted)?;
    let wall = match (&point.wall, &decision.act) {
        // The blind retry, or the measured return's act (a check that holds it back leaves
        // the blind retry in the record's `owed`, and the point is decided again here).
        (
            Some(Wall::Network { what }),
            Act::Type {
                rule: crate::words::RULE_API_RETRY | crate::words::RULE_API_BACK,
                ..
            },
        ) => RecoveryWall::Network {
            what: what.clone(),
            measured,
        },
        (
            Some(Wall::Overlong { what }),
            Act::Type {
                rule: crate::words::RULE_API_CUTOFF,
                ..
            },
        ) => RecoveryWall::Overlong { what: what.clone() },
        (
            Some(Wall::Entitlement),
            Act::Type {
                rule: crate::words::RULE_LIMIT_RESUME,
                ..
            },
        ) => RecoveryWall::Entitlement { quota: None },
        (
            Some(Wall::Server {
                what,
                cut_off,
                retry_at,
            }),
            Act::Type { rule, .. },
        ) if matches!(
            *rule,
            crate::words::RULE_API_RETRY | crate::words::RULE_API_CUTOFF
        ) =>
        {
            RecoveryWall::Server {
                what: what.clone(),
                cut_off: *cut_off,
                retry_at_ms: retry_at.map(|at| epoch_of(at, now, epoch)),
            }
        }
        (
            Some(Wall::Usage {
                reset_at,
                reset_label,
            }),
            Act::ResumeLimit { .. },
        ) => RecoveryWall::Usage {
            reset_at_ms: reset_at.map(|at| epoch_of(at, now, epoch)),
            reset_label: reset_label.clone(),
            quota: None,
        },
        _ => return None,
    };
    Some(Recovery {
        thread: thread.to_owned(),
        turn_id: turn_id.to_owned(),
        point_at_ms: epoch_of(point.at, now, epoch),
        not_before_ms: epoch_of(decision.not_before.unwrap_or(now).max(point.at), now, epoch),
        by_autopilot: point.by_autopilot,
        by_person: point.by_person,
        wall,
    })
}

/// A stale writer record cannot resurrect a failure a newer rollout point replaced.
fn current_failed_turn(items: &[RolloutItem], recovery: &Recovery) -> bool {
    let crate::resume_history::NativePoint::Complete(complete) =
        crate::resume_history::latest(items).point
    else {
        return false;
    };
    complete.turn_id == recovery.turn_id
        && complete
            .error
            .as_ref()
            .and_then(|error| error.codex_error_info.as_ref())
            .and_then(|info| {
                crate::extension::classify(
                    info,
                    &codex_protocol::error::CodexErrorDetails::Stream("recorded failure".into()),
                    false,
                )
            })
            .is_some_and(|wall| {
                matches!(
                    (&recovery.wall, wall),
                    (RecoveryWall::Server { .. }, Wall::Server { .. })
                | (RecoveryWall::Overlong { .. }, Wall::Server { .. })
                | (RecoveryWall::Overlong { .. }, Wall::Overlong { .. })
                | (RecoveryWall::Network { .. }, Wall::Network { .. })
                | (RecoveryWall::Entitlement { .. }, Wall::Entitlement)
                // Native rollout collapses QuotaExceeded and UsageNotIncluded into
                // UsageLimitExceeded. The recorded classification retains the distinction.
                | (RecoveryWall::Entitlement { .. }, Wall::Usage { .. })
                | (RecoveryWall::Usage { .. }, Wall::Usage { .. })
                )
            })
}

/// Whether a restored `recovery` has its wall's route measured again: a network wall whose
/// route the process before measured ([`RecoveryWall::Network`]'s `measured`).
pub(crate) fn rearms(recovery: &Recovery) -> bool {
    matches!(recovery.wall, RecoveryWall::Network { measured: true, .. })
}

/// The caller first proves the exact crash/re-exec handoff and thread identity. Restore
/// only the matching latest failed rollout turn, then let normal policy/goal checks decide.
pub(crate) fn restore(
    state: &mut ThreadState,
    items: &[RolloutItem],
    recovery: &Recovery,
    now: Instant,
    epoch: u64,
    timing: &Timing,
) -> bool {
    if !state.task || !current_failed_turn(items, recovery) {
        return false;
    }
    let Some(not_before) = instant_of(recovery.not_before_ms, now, epoch) else {
        return false;
    };
    let wall = match &recovery.wall {
        RecoveryWall::Network { what, .. } => Wall::Network { what: what.clone() },
        RecoveryWall::Overlong { what } => Wall::Overlong { what: what.clone() },
        RecoveryWall::Entitlement { .. } => Wall::Entitlement,
        RecoveryWall::Server {
            what,
            cut_off,
            retry_at_ms,
        } => Wall::Server {
            what: what.clone(),
            cut_off: *cut_off,
            retry_at: match retry_at_ms {
                Some(ms) => {
                    let Some(at) = instant_of(*ms, now, epoch) else {
                        return false;
                    };
                    Some(at)
                }
                None => None,
            },
        },
        RecoveryWall::Usage {
            reset_at_ms,
            reset_label,
            ..
        } => Wall::Usage {
            reset_at: match reset_at_ms {
                Some(ms) => {
                    let Some(at) = instant_of(*ms, now, epoch) else {
                        return false;
                    };
                    Some(at)
                }
                None => None,
            },
            reset_label: reset_label.clone(),
        },
    };
    state.observe_turn_end(
        TurnEnd {
            at: instant_of(recovery.point_at_ms, now, epoch).unwrap_or(now),
            by_autopilot: recovery.by_autopilot,
            by_person: recovery.by_person,
            text: None,
            earlier: None,
            questions: false,
            worked: Duration::ZERO,
            wall: Some(wall),
            aborted: false,
            ran_tools: false,
            restored: true,
        },
        timing,
    );
    state.recovery_not_before = Some(not_before);
    state.point_turn_id = Some(recovery.turn_id.clone());
    true
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
