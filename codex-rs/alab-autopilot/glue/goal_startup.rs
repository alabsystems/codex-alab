// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside native goal runtime; restored data never enables a runtime.
use codex_alab_core::upgrade_startup_hold::Fence;
use std::sync::Arc;
impl super::super::GoalRuntimeHandle {
    pub(crate) fn alab_bind_startup_hold(&self, hold: Option<Arc<Fence>>) {
        *self
            .inner
            .alab_startup
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = hold.map(|hold| (*hold).clone());
    }
    pub(crate) fn alab_startup_pending(&self) -> bool {
        self.inner
            .alab_startup
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(Fence::pending)
    }
    pub(crate) fn alab_startup_configuration_changing(&self, enabled: bool) {
        if self.is_enabled() != enabled
            && let Some(hold) = self
                .inner
                .alab_startup
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
        {
            hold.invalidate();
        }
    }
    /// Check the actual destination runtime, actual persisted row and actual
    /// bootstrap hold before adopting source clock data. No row is overwritten.
    pub async fn alab_restore_forest_goal(
        &self,
        hold: &Fence,
        expected_thread: codex_protocol::ThreadId,
        expected: Option<&codex_state::ThreadGoal>,
        clock: Option<codex_alab_core::goal_clock::Stamp>,
    ) -> Result<(), String> {
        let bound = self
            .inner
            .alab_startup
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or("goal runtime has no native startup hold")?;
        if self.thread_id() != expected_thread
            || !bound.same(hold)
            || !hold.pending()
            || !hold.valid()
        {
            return Err("goal restoration differs from its actual startup owner".into());
        }
        let _permit = self
            .inner
            .goal_state_lock
            .acquire()
            .await
            .map_err(|error| error.to_string())?;
        if !self.alab_upgrade_goal_state_ready() {
            return Err("goal runtime owns work during restoration".into());
        }
        let current = self
            .inner
            .state_dbs
            .thread_goals()
            .get_thread_goal(expected_thread)
            .await
            .map_err(|error| error.to_string())?;
        if current.as_ref() != expected {
            return Err("persisted goal differs from captured startup state".into());
        }
        match (current.as_ref(), clock) {
            (Some(goal), Some(clock))
                if self.is_enabled() && goal.status == codex_state::ThreadGoalStatus::Active =>
            {
                self.inner
                    .accounting_state
                    .mark_idle_goal_active(goal.goal_id.clone());
                self.inner
                    .accounting_state
                    .alab_adopt_goal_clock(&goal.goal_id, clock)?;
            }
            (Some(goal), None)
                if self.is_enabled() && goal.status == codex_state::ThreadGoalStatus::Active =>
            {
                return Err("active destination goal requires its captured clock".into());
            }
            (_, Some(_)) => {
                return Err("goal clock cannot enable a stopped destination runtime".into());
            }
            (_, None) => self.inner.accounting_state.clear_active_goal(),
        }
        if !hold.valid() {
            return Err("goal configuration changed during restoration".into());
        }
        Ok(())
    }
    /// Forests without a goal capsule can restore only absent/stopped native
    /// goals; an active row must not silently acquire a new accounting epoch.
    pub async fn alab_restore_stopped_forest_goal(&self, hold: &Fence) -> Result<(), String> {
        let row = self
            .inner
            .state_dbs
            .thread_goals()
            .get_thread_goal(self.thread_id())
            .await
            .map_err(|error| error.to_string())?;
        self.alab_restore_forest_goal(hold, self.thread_id(), row.as_ref(), None)
            .await
    }
}
