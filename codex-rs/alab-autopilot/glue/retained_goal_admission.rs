// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use codex_alab_core::retained_goal_gate::Admission;
use codex_alab_core::retained_goal_gate::Gate;
use std::sync::Arc;
impl super::super::GoalRuntimeHandle {
    pub(crate) fn alab_bind_retained_goal_gate(&self, gate: Option<Arc<Gate>>) {
        *self
            .inner
            .alab_retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = gate.map(|gate| (*gate).clone());
    }
    pub(crate) async fn alab_retained_goal_admission(&self) -> Result<Option<Admission>, ()> {
        let gate = self.inner.alab_retained.lock().map_err(|_| ())?.clone();
        match gate {
            Some(gate) => gate.admit().await.map(Some).ok_or(()),
            None => Ok(None),
        }
    }
    /// Call from the actual backend owner after its exact Native frontier opened.
    /// The original policy/runtime/goal checks decide whether anything can run.
    pub async fn alab_recheck_after_retained_frontier(&self) -> Result<(), String> {
        self.continue_if_idle_with_policy(self.alab_policy_continuation())
            .await
    }
}
