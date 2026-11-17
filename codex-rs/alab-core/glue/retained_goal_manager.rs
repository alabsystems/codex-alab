// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use codex_alab_core::retained_goal_gate::Gate;
impl ThreadManager {
    pub fn alab_bind_retained_goal_gate(&self, gate: Gate) -> Result<(), String> {
        self.state.alab_retained_goal_gate.bind(gate)
    }
}
fn attach(init: &mut ExtensionDataInit, gate: Gate) -> Result<(), String> {
    if init
        .get::<Gate>()
        .is_some_and(|existing| !existing.same(&gate))
    {
        return Err("thread supplied another retained goal gate".into());
    }
    init.insert(gate);
    Ok(())
}

pub(super) fn before_start(
    registration: &codex_alab_core::retained_goal_gate::Registration,
    init: &mut ExtensionDataInit,
) -> Result<(), String> {
    if let Some(gate) = registration.enter()? {
        attach(init, gate)?;
    }
    Ok(())
}
