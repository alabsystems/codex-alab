// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use codex_alab_core::goal_clock;

impl GoalAccountingState {
    pub(crate) fn alab_goal_clock(&self, goal: &str) -> Result<Option<goal_clock::Stamp>, String> {
        let inner = self.inner();
        match inner.wall_clock.active_goal_id.as_deref() {
            None => Ok(None),
            Some(active) if active == goal => {
                goal_clock::checkpoint(&inner.wall_clock.last_accounted_at)
                    .map(Some)
                    .map_err(str::to_owned)
            }
            Some(_) => Err("native idle clock belongs to another goal".into()),
        }
    }
    pub(crate) fn alab_adopt_goal_clock(
        &self,
        goal: &str,
        stamp: goal_clock::Stamp,
    ) -> Result<(), String> {
        let mut inner = self.inner();
        if inner.current_turn_id.is_some()
            || inner.interrupted_turn_id.is_some()
            || inner.wall_clock.active_goal_id.as_deref() != Some(goal)
        {
            return Err("goal clock restoration requires the same idle native goal".into());
        }
        goal_clock::adopt(&mut inner.wall_clock.last_accounted_at, stamp).map_err(str::to_owned)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn native_accounting_adopts_exact_fraction_and_keeps_accounted_progress_on_retry() {
        let source = GoalAccountingState::default();
        source.mark_idle_goal_active("same-goal");
        source.inner().wall_clock.last_accounted_at = Instant::now()
            .checked_sub(Duration::new(2, 999_999_999))
            .unwrap();
        let stamp = source.alab_goal_clock("same-goal").unwrap().unwrap();
        let fresh = GoalAccountingState::default();
        assert!(fresh.alab_adopt_goal_clock("same-goal", stamp).is_err());
        fresh.mark_idle_goal_active("same-goal");
        fresh.alab_adopt_goal_clock("same-goal", stamp).unwrap();
        assert_eq!(fresh.alab_goal_clock("same-goal").unwrap(), Some(stamp));
        assert!(fresh.alab_goal_clock("another-goal").is_err());
        fresh.inner().wall_clock.mark_accounted(2);
        let advanced = fresh.alab_goal_clock("same-goal").unwrap();
        assert_ne!(advanced, Some(stamp));
        fresh.alab_adopt_goal_clock("same-goal", stamp).unwrap();
        assert_eq!(fresh.alab_goal_clock("same-goal").unwrap(), advanced);
    }
}
