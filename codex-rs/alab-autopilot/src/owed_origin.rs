// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bind an already-authorized handoff to the native point it observed.

use crate::decide::Owed;
use crate::decide::is_wall_rule;
use crate::resume_history::NativeHistory;
use crate::resume_history::NativePoint;
use crate::words::POLICY_BOUNDARY;
use crate::words::POLICY_CONTINUE;
use crate::words::RULE_HOST_TURN;
use crate::words::RULE_POLICY_CONTINUE;
use codex_alab_core::session::InFlightRetry;
use codex_alab_core::session::NativeOrigin;
use codex_alab_core::session::NativePhase;
use codex_alab_core::session::OwedAct;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::TurnAbortReason;

/// Only validators in this module create a handoff that can retain a landing.
#[derive(Clone)]
pub(crate) struct BoundHandoff {
    owed: Option<Owed>,
    landed: bool,
}

impl BoundHandoff {
    /// A validated live-carriable act or landing, not an empty or wall-try record.
    pub(crate) fn carries_automatic_intent(&self) -> bool {
        self.landed
            || self
                .owed
                .as_ref()
                .is_some_and(|owed| !is_wall_rule(owed.rule))
    }

    pub(crate) fn word(&self) -> &'static str {
        self.owed
            .as_ref()
            .map_or(if self.landed { "landed" } else { "none" }, |owed| {
                owed.rule
            })
    }
    pub(crate) fn empty() -> Self {
        Self {
            owed: None,
            landed: false,
        }
    }

    pub(crate) fn into_parts(self) -> (Option<Owed>, bool) {
        (self.owed, self.landed)
    }

    /// Whether it hands on a wall's try ([`is_wall_rule`]), which binds only at the failed
    /// turn its decision observed ([`bind`]): that image classified the failure with its
    /// details and decided the retry there, so the history's bare completion (a provisional
    /// fatal for core's `Other`) does not replace that point.
    pub(crate) fn wall_try(&self) -> bool {
        self.owed
            .as_ref()
            .is_some_and(|owed| is_wall_rule(owed.rule))
    }
}

fn policy_instruction(text: Option<&str>, history: &NativeHistory<'_>) -> bool {
    history.started
        && history.saw_regular_input
        && !history.compacting
        && matches!(history.point, NativePoint::Complete(complete)
            if complete.error.as_ref().and_then(|error| error.codex_error_info.as_ref())
                == Some(&CodexErrorInfo::CyberPolicy))
        && text
            .and_then(|text| text.strip_prefix(POLICY_CONTINUE))
            .is_some_and(|suffix| {
                suffix.is_empty() || suffix.strip_prefix(' ') == Some(POLICY_BOUNDARY)
            })
}

pub(crate) fn origin(history: &NativeHistory<'_>) -> Option<NativeOrigin> {
    let phase = match history.point {
        NativePoint::Running => NativePhase::Running,
        NativePoint::Complete(_) => NativePhase::Complete,
        NativePoint::Aborted(aborted)
            if history.started && aborted.turn_id.as_deref() == history.turn_id =>
        {
            if aborted.reason == TurnAbortReason::Interrupted {
                NativePhase::Interrupted
            } else {
                NativePhase::Aborted
            }
        }
        _ => return None,
    };
    let turn_id = history.turn_id?;
    if turn_id.trim().is_empty() || turn_id.len() > 512 {
        return None;
    }
    Some(NativeOrigin {
        turn_id: turn_id.to_owned(),
        phase,
    })
}

fn bind(
    previous: Option<&NativeOrigin>,
    host_turn: bool,
    landed: bool,
    failed_point_intent: bool,
    history: &NativeHistory<'_>,
    retry: Option<&InFlightRetry>,
) -> Option<NativeOrigin> {
    let current = origin(history)?;
    if !landed
        && matches!(history.point, NativePoint::Complete(complete)
        if complete.error.as_ref().is_some_and(ErrorEvent::affects_turn_status))
    {
        // Only a wall try or a separately validated fixed policy instruction may retain
        // intent at its own failed point. An ordinary act, another point or a legacy
        // origin authorizes no request at a failure.
        return (failed_point_intent && previous == Some(&current)).then_some(current);
    }
    if let Some(previous) = previous {
        if previous != &current {
            return None;
        }
        return match (host_turn, landed, current.phase) {
            (_, true, NativePhase::Complete | NativePhase::Interrupted | NativePhase::Aborted)
            | (
                true,
                false,
                NativePhase::Running | NativePhase::Complete | NativePhase::Interrupted,
            )
            | (false, false, NativePhase::Complete) => Some(current),
            _ => None,
        };
    }
    // An unbound ordinary act or landing cannot distinguish its old completion from
    // a newer completed correction. Legacy records persist no identity for that point.
    // Older in-flight pacing can identify the exact open attempt of an existing HostTurn.
    // It only binds the owed action supplied by the authenticated record, never creates one.
    (host_turn
        && current.phase == NativePhase::Running
        && retry.is_some_and(|retry| retry.turn_id == current.turn_id))
    .then_some(current)
}

/// Validate the schema snapshot before losing a landing's origin in the runtime tuple.
pub(crate) fn record(
    carried: &OwedAct,
    history: &NativeHistory<'_>,
    retry: Option<&InFlightRetry>,
) -> Option<BoundHandoff> {
    // Renewal intent has its own native receipt and identity admission. Never
    // turn an unverified receipt into the ordinary carried text path.
    if carried.renewal.is_some() {
        return None;
    }
    let policy_continue = carried.rule.as_deref() == Some(RULE_POLICY_CONTINUE);
    if policy_continue && (carried.landed || !policy_instruction(carried.text.as_deref(), history))
    {
        return None;
    }
    let retry = retry.filter(|retry| retry.thread == carried.thread);
    let mut carried = carried.clone();
    carried.native_origin = Some(bind(
        carried.native_origin.as_ref(),
        !carried.landed && carried.rule.as_deref() == Some(RULE_HOST_TURN),
        carried.landed,
        !carried.landed && (policy_continue || carried.rule.as_deref().is_some_and(is_wall_rule)),
        history,
        retry,
    )?);
    let owed = if carried.landed {
        None
    } else {
        Some(crate::Autopilot::owed_of(&carried)?)
    };
    Some(BoundHandoff {
        owed,
        landed: carried.landed,
    })
}

/// Defense for direct/runtime handoffs. Invalid old intent becomes an empty handoff, so
/// the separately proved image identity may still restore needs without restoring an act.
#[cfg(test)]
pub(crate) fn runtime(
    handoff: Option<(Option<Owed>, bool)>,
    history: &NativeHistory<'_>,
) -> Option<BoundHandoff> {
    handoff.map(|(owed, landed)| match owed {
        Some(mut owed) => {
            let policy_continue = owed.rule == RULE_POLICY_CONTINUE;
            if policy_continue && (landed || !policy_instruction(Some(owed.text.as_str()), history))
            {
                return BoundHandoff::empty();
            }
            let bound = bind(
                owed.native_origin.as_ref(),
                !landed && owed.rule == RULE_HOST_TURN,
                landed,
                !landed && (policy_continue || is_wall_rule(owed.rule)),
                history,
                None,
            );
            if let Some(bound) = bound {
                owed.native_origin = Some(bound);
                BoundHandoff {
                    owed: Some(owed),
                    landed,
                }
            } else {
                BoundHandoff::empty()
            }
        }
        None => BoundHandoff::empty(),
    })
}

/// An interrupted point already observed by an authenticated owed HostTurn does not
/// supersede that intent. A later interrupt changes a Running origin and revokes it.
pub(crate) fn interrupted_host_turn(owed: Option<&Owed>, history: &NativeHistory<'_>) -> bool {
    owed.is_some_and(|owed| {
        owed.rule == RULE_HOST_TURN
            && owed.native_origin.as_ref().is_some_and(|previous| {
                previous.phase == NativePhase::Interrupted
                    && bind(Some(previous), true, false, false, history, None).is_some()
            })
    })
}
