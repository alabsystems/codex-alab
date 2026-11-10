// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Interrupting a turn to steer it does not cancel continuous work. Only an
//! explicit goal pause stops its objective in Cruise or Land. Ordinary Codex
//! and Autopilot Off retain the upstream interrupt-to-pause behavior.

use codex_alab_core::session::Autopilot;
use codex_alab_core::session::HarnessWord;

pub fn pause_goal_on_turn_interrupt() -> bool {
    crate::boxes::tick_shared().is_none_or(|shared| pauses_goal(shared.harness()))
}

fn pauses_goal(harness: (Autopilot, HarnessWord)) -> bool {
    !matches!(
        harness,
        (
            Autopilot::Active | Autopilot::Passive,
            HarnessWord::Cruise | HarnessWord::Land
        )
    )
}
