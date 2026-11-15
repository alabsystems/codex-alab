// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A finished native turn may retain usage without retaining live-turn authority.
use super::GoalAccountingState;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub(crate) struct FinishedTurn {
    accounting: Arc<GoalAccountingState>,
    turn_id: String,
    active: bool,
}

impl GoalAccountingState {
    pub(crate) fn alab_finished_turn(self: &Arc<Self>, turn_id: &str) -> FinishedTurn {
        FinishedTurn {
            accounting: Arc::clone(self),
            turn_id: turn_id.to_owned(),
            active: codex_alab_core::active(),
        }
    }

    pub(crate) fn alab_retained_goal_id(&self, turn_id: &str) -> Option<String> {
        self.inner().turns.get(turn_id)?.active_goal_id()
    }

    pub(crate) fn alab_preserves_pending_progress(&self) -> bool {
        codex_alab_core::active() && !self.alab_persistence.permits_admission()
    }

    pub(crate) fn alab_retain_rejected_progress(
        &self,
        original: &str,
        actual: Option<&codex_state::ThreadGoal>,
    ) -> Result<(), String> {
        if !codex_alab_core::active() {
            return Ok(());
        }
        codex_alab_core::goal_persistence::definitely_rolled_back();
        if actual.is_none_or(|goal| goal.goal_id != original) {
            self.alab_quarantine_original_boundary(original);
        }
        Err(
            "the original goal did not accept positive pending usage; it remains retained"
                .to_owned(),
        )
    }

    pub(crate) fn alab_quarantine_pending_boundaries(&self) {
        let originals: std::collections::BTreeSet<_> = {
            let inner = self.inner();
            inner
                .wall_clock
                .active_goal_id
                .iter()
                .chain(
                    inner
                        .turns
                        .values()
                        .filter_map(|turn| turn.active_goal_id.as_ref()),
                )
                .cloned()
                .collect()
        };
        for original in originals {
            self.alab_quarantine_original_boundary(&original);
        }
    }

    fn alab_quarantine_original_boundary(&self, original: &str) {
        let inner = self.inner();
        let same_clock = inner.wall_clock.active_goal_id.as_deref() == Some(original);
        self.alab_persistence.quarantine_original_goal(
            codex_alab_core::goal_persistence::QuarantinedUsage {
                original_goal_id: original.to_owned(),
                elapsed_seconds: if same_clock {
                    inner.wall_clock.time_delta_since_last_accounting()
                } else {
                    0
                },
                descendant_tokens: if same_clock {
                    self.descendant_token_usage
                        .load(Ordering::Relaxed)
                        .saturating_sub(inner.last_accounted_descendant_token_usage)
                } else {
                    0
                },
            },
        );
    }

    /// A supported mutation drains before changing the goal. A raw other writer
    /// can violate that ordering: quarantine the observed original boundary
    /// before native turn-start would reset its shared clock/descendant baseline.
    pub(crate) fn alab_goal_association_allowed(&self, next_goal: Option<&str>) -> bool {
        if !codex_alab_core::active() || self.alab_persistence.permits_admission() {
            return true;
        }
        self.alab_held_goal_association_allowed(next_goal)
    }

    fn alab_held_goal_association_allowed(&self, next_goal: Option<&str>) -> bool {
        let inner = self.inner();
        let original = inner
            .wall_clock
            .active_goal_id
            .as_deref()
            .filter(|goal| Some(*goal) != next_goal)
            .or_else(|| {
                inner.turns.values().find_map(|turn| {
                    turn.active_goal_id
                        .as_deref()
                        .filter(|goal| Some(*goal) != next_goal)
                })
            });
        let Some(original) = original else {
            return self.alab_persistence.quarantined_usage().is_empty();
        };
        let original = original.to_owned();
        drop(inner);
        self.alab_quarantine_original_boundary(&original);
        false
    }
}

impl Drop for FinishedTurn {
    fn drop(&mut self) {
        if self.active {
            let mut inner = self.accounting.inner();
            if inner.current_turn_id.as_deref() == Some(&self.turn_id) {
                inner.current_turn_id = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::protocol::TokenUsage;
    use std::time::Duration;

    #[test]
    fn a_raw_replacement_quarantines_the_original_shared_boundary() {
        let accounting = Arc::new(GoalAccountingState::default());
        accounting.start_turn("old-turn", ModeKind::Default, &TokenUsage::default());
        accounting.mark_turn_goal_active("old-turn", "old-goal");
        accounting.inner().wall_clock.last_accounted_at = super::super::Instant::now()
            .checked_sub(Duration::from_secs(60))
            .unwrap();
        accounting.record_descendant_token_usage(&TokenUsage {
            input_tokens: 7,
            total_tokens: 7,
            ..Default::default()
        });
        accounting.alab_persistence.begin().finish(false);
        accounting.inner().current_turn_id = None;
        accounting.start_turn("new-turn", ModeKind::Default, &TokenUsage::default());
        assert!(!accounting.alab_held_goal_association_allowed(Some("new-goal")));
        let boundary = accounting.alab_persistence.quarantined_usage();
        assert_eq!(boundary.len(), 1);
        assert_eq!(boundary[0].original_goal_id, "old-goal");
        assert_eq!(boundary[0].descendant_tokens, 7);
        assert!(boundary[0].elapsed_seconds >= 60);
        assert_eq!(
            accounting.alab_retained_goal_id("old-turn").as_deref(),
            Some("old-goal")
        );
        // Later manual work and even a raw restoration cannot reconcile the
        // old observations or turn a successful unrelated mutation into release.
        accounting.record_descendant_token_usage(&TokenUsage {
            input_tokens: 11,
            total_tokens: 11,
            ..Default::default()
        });
        assert!(!accounting.alab_held_goal_association_allowed(Some("old-goal")));
        accounting
            .alab_persistence
            .begin_mutation()
            .commit_mutation();
        assert_eq!(accounting.alab_persistence.quarantined_usage(), boundary);
        assert!(!accounting.alab_persistence.permits_write());
        assert!(!accounting.alab_persistence.permits_admission());
    }
}
