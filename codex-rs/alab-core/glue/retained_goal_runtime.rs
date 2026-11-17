// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Install the actual retained gate in the actual prepared processor.
impl super::MessageProcessor {
    pub(crate) fn alab_bind_retained_goal_gate(
        &self,
        gate: codex_alab_core::retained_goal_gate::Gate,
    ) -> Result<(), String> {
        self.thread_processor
            .thread_manager
            .alab_bind_retained_goal_gate(gate)
    }
}
