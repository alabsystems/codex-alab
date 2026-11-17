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
    pub(crate) fn alab_mutation_effect_authorized(&self) -> bool {
        self.alab_snapshot_data_change()
            .is_none_or(|change| change.mutation_effect_authorized())
    }

    pub(crate) fn alab_snapshot_data_change(
        &self,
    ) -> Option<codex_alab_core::goal_persistence::AccountingDataChange<'_>> {
        codex_alab_core::is_alab_binary()
            .then(|| self.alab_persistence.begin_accounting_data_change())
    }

    pub(crate) fn alab_needs_accounting_drain(&self) -> bool {
        codex_alab_core::is_alab_binary() && self.alab_persistence.accounting_drain_needed()
    }

    pub(crate) fn alab_close_empty_turn_boundary(&self, turn_id: &str) {
        self.alab_settle_empty_turn_boundary(turn_id, true);
    }

    pub(crate) fn alab_reopen_empty_turn_boundary(&self, turn_id: &str) {
        self.alab_settle_empty_turn_boundary(turn_id, false);
    }

    fn alab_settle_empty_turn_boundary(&self, turn_id: &str, clear: bool) {
        let Some(mut change) = self.alab_snapshot_data_change() else {
            return;
        };
        let mut inner = self.inner();
        let Some(goal) = inner
            .turns
            .get(turn_id)
            .and_then(|turn| turn.active_goal_id.clone())
        else {
            return;
        };
        let descendant_delta = self
            .descendant_token_usage
            .load(Ordering::Relaxed)
            .saturating_sub(inner.last_accounted_descendant_token_usage);
        let turn_delta = inner.turns.get(turn_id).map_or(
            0,
            super::GoalTurnAccounting::token_delta_since_last_accounting,
        );
        let owns_clock = inner.wall_clock.active_goal_id.as_deref() == Some(goal.as_str());
        if !change.additive_snapshot_is_current() {
            return;
        }
        let time_delta = if owns_clock {
            inner.wall_clock.time_delta_since_last_accounting()
        } else {
            0
        };
        if turn_delta.saturating_add(descendant_delta) > 0 || time_delta > 0 {
            return;
        }
        if clear {
            if let Some(turn) = inner.turns.get_mut(turn_id) {
                turn.active_goal_id = None;
            }
            if owns_clock {
                inner.wall_clock.clear_active_goal();
            }
            change.publish_additive_settlement(owns_clock);
        } else {
            if owns_clock {
                inner.wall_clock.mark_active_goal(goal);
                change.publish_additive_settlement_and_reopen();
            } else {
                change.publish_additive_settlement(false);
            }
        }
    }

    pub(crate) fn alab_close_empty_idle_boundary(&self) {
        self.alab_settle_empty_idle_boundary(true);
    }

    pub(crate) fn alab_reopen_empty_idle_boundary(&self) {
        self.alab_settle_empty_idle_boundary(false);
    }

    fn alab_settle_empty_idle_boundary(&self, clear: bool) {
        let Some(mut change) = self.alab_snapshot_data_change() else {
            return;
        };
        let mut inner = self.inner();
        if inner.wall_clock.active_goal_id.is_none() {
            return;
        }
        if !change.additive_snapshot_is_current() {
            return;
        }
        let descendant_delta = self
            .descendant_token_usage
            .load(Ordering::Relaxed)
            .saturating_sub(inner.last_accounted_descendant_token_usage);
        if descendant_delta > 0 || inner.wall_clock.time_delta_since_last_accounting() > 0 {
            return;
        }
        if clear {
            inner.wall_clock.clear_active_goal();
            change.publish_additive_settlement(true);
        } else {
            let Some(goal) = inner.wall_clock.active_goal_id.clone() else {
                return;
            };
            inner.wall_clock.mark_active_goal(goal);
            change.publish_additive_settlement_and_reopen();
        }
    }

    pub(crate) fn alab_close_all_empty_boundaries(&self) -> bool {
        self.alab_finish_all_empty_boundaries(true)
    }

    /// Stop the source clock at one exact post-accounting cutoff while retaining
    /// its goal and baseline for the destination. Retirement clears this frozen
    /// boundary only after final state has been captured.
    pub(crate) fn alab_freeze_all_empty_boundaries(&self) -> bool {
        self.alab_finish_all_empty_boundaries(false)
    }

    /// Freeze the final exact accounting cutoff and admit SQLite dispatch under
    /// one State guard. A callback can run before this operation or after the
    /// mutation is visible, never in a freeze/begin/dispatch gap.
    pub(crate) fn alab_begin_dispatched_mutation(
        &self,
    ) -> Result<
        codex_alab_core::goal_persistence::Write,
        codex_alab_core::goal_persistence::DispatchedMutationError,
    > {
        use codex_alab_core::goal_persistence::DispatchedMutationError;
        let Some(mut change) = self.alab_snapshot_data_change() else {
            return Err(DispatchedMutationError::Held);
        };
        let mut inner = self.inner();
        let descendant_delta = self
            .descendant_token_usage
            .load(Ordering::Relaxed)
            .saturating_sub(inner.last_accounted_descendant_token_usage);
        let has_turn_progress = inner.turns.values().any(|turn| {
            turn.active_goal_id.is_some() && turn.token_delta_since_last_accounting() > 0
        });
        if !change.can_prepare_dispatched_mutation() {
            return Err(if has_turn_progress || descendant_delta > 0 {
                DispatchedMutationError::Progress
            } else {
                DispatchedMutationError::Held
            });
        }
        let newly_frozen = inner.wall_clock.active_goal_id.is_some() && inner.wall_clock.freeze();
        let has_clock_progress = inner.wall_clock.active_goal_id.is_some()
            && (descendant_delta > 0 || inner.wall_clock.time_delta_since_last_accounting() > 0);
        if has_turn_progress || has_clock_progress {
            if newly_frozen {
                inner.wall_clock.frozen_at = None;
            }
            return Err(DispatchedMutationError::Progress);
        }
        if newly_frozen {
            change.publish_frozen_clock();
        }
        change
            .begin_dispatched_mutation(&self.alab_persistence)
            .ok_or(DispatchedMutationError::Held)
    }

    /// A typed storage rollback leaves the old association authoritative. Undo
    /// the freeze without moving its baseline, so elapsed time spent in SQLite
    /// remains attributable, and keep the reopened generation unreconciled.
    pub(crate) fn alab_reopen_rolled_back_mutation(&self) {
        let Some(mut change) = self.alab_snapshot_data_change() else {
            return;
        };
        let mut inner = self.inner();
        if inner.wall_clock.active_goal_id.is_none() {
            return;
        }
        if inner.wall_clock.frozen_at.is_none() {
            return;
        }
        inner.wall_clock.frozen_at = None;
        change.publish_clock_change(true);
    }

    fn alab_finish_all_empty_boundaries(&self, clear: bool) -> bool {
        let Some(mut change) = self.alab_snapshot_data_change() else {
            return true;
        };
        let mut inner = self.inner();
        let newly_frozen = inner.wall_clock.active_goal_id.is_some() && inner.wall_clock.freeze(); // ALAB(storage): decide against one fixed elapsed-time cutoff, never a moving clock.
        let descendant_delta = self
            .descendant_token_usage
            .load(Ordering::Relaxed)
            .saturating_sub(inner.last_accounted_descendant_token_usage);
        let has_turn_progress = inner.turns.values().any(|turn| {
            turn.active_goal_id.is_some() && turn.token_delta_since_last_accounting() > 0
        });
        let has_clock_progress = inner.wall_clock.active_goal_id.is_some()
            && (descendant_delta > 0 || inner.wall_clock.time_delta_since_last_accounting() > 0);
        if has_turn_progress || has_clock_progress {
            if newly_frozen {
                change.publish_clock_change(false);
            }
            return false;
        }
        if clear {
            for turn in inner.turns.values_mut() {
                turn.active_goal_id = None;
            }
            inner.wall_clock.clear_active_goal();
            change.publish_fresh_closed_clock();
        } else {
            change.publish_frozen_clock();
        }
        true
    }

    pub(crate) fn alab_finished_turn(self: &Arc<Self>, turn_id: &str) -> FinishedTurn {
        FinishedTurn {
            accounting: Arc::clone(self),
            turn_id: turn_id.to_owned(),
            active: codex_alab_core::is_alab_binary(),
        }
    }

    pub(crate) fn alab_retained_goal_id(&self, turn_id: &str) -> Option<String> {
        self.inner().turns.get(turn_id)?.active_goal_id()
    }

    pub(crate) fn alab_preserves_pending_progress(&self) -> bool {
        codex_alab_core::is_alab_binary()
            && !self
                .alab_persistence
                .permits_destructive_accounting_change()
    }

    pub(crate) fn alab_retain_rejected_progress(
        &self,
        original: &str,
        actual: Option<&codex_state::ThreadGoal>,
    ) -> Result<(), String> {
        if !codex_alab_core::is_alab_binary() {
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
        let usage = {
            let inner = self.inner();
            let same_clock = inner.wall_clock.active_goal_id.as_deref() == Some(original);
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
                turn_tokens: inner
                    .turns
                    .values()
                    .filter(|turn| turn.active_goal_id.as_deref() == Some(original))
                    .fold(0_i64, |total, turn| {
                        total.saturating_add(turn.token_delta_since_last_accounting().max(0))
                    }),
            }
        };
        self.alab_persistence.quarantine_original_goal(usage);
    }

    /// A supported mutation drains before changing the goal. A raw other writer
    /// can violate that ordering: quarantine the observed original boundary
    /// before native turn-start would reset its shared clock/descendant baseline.
    pub(crate) fn alab_acknowledge_pending_same_goal_effect(&self, next_goal: &str) {
        let Some(change) = self.alab_snapshot_data_change() else {
            return;
        };
        if !change.mutation_effect_pending_data_only_authorized() {
            return;
        }
        let inner = self.inner();
        let original = inner
            .wall_clock
            .active_goal_id
            .as_deref()
            .filter(|goal| *goal != next_goal)
            .or_else(|| {
                inner.turns.values().find_map(|turn| {
                    turn.active_goal_id
                        .as_deref()
                        .filter(|goal| *goal != next_goal)
                })
            })
            .map(str::to_owned);
        drop(inner);
        let Some(original) = original else {
            codex_alab_core::goal_persistence::mutation_effect_applied();
            return;
        };
        drop(change);
        self.alab_quarantine_original_boundary(&original);
    }

    pub(crate) fn alab_goal_association_allowed(&self, next_goal: Option<&str>) -> bool {
        if !codex_alab_core::is_alab_binary() || self.alab_persistence.permits_admission() {
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
        let original = original.map(str::to_owned);
        drop(inner);
        let Some(original) = original else {
            let quarantined = self.alab_persistence.quarantined_usage();
            for usage in &quarantined {
                self.alab_quarantine_original_boundary(&usage.original_goal_id);
            }
            return quarantined.is_empty();
        };
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
        accounting.record_token_usage(
            "old-turn",
            &TokenUsage {
                input_tokens: 5,
                total_tokens: 5,
                ..Default::default()
            },
        );
        accounting.alab_persistence.begin().finish(false);
        accounting.inner().current_turn_id = None;
        accounting.start_turn("new-turn", ModeKind::Default, &TokenUsage::default());
        assert!(!accounting.alab_held_goal_association_allowed(Some("new-goal")));
        let boundary = accounting.alab_persistence.quarantined_usage();
        assert_eq!(boundary.len(), 1);
        assert_eq!(boundary[0].original_goal_id, "old-goal");
        assert_eq!(boundary[0].descendant_tokens, 7);
        assert_eq!(boundary[0].turn_tokens, 5);
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
        accounting.record_token_usage(
            "old-turn",
            &TokenUsage {
                input_tokens: 9,
                total_tokens: 9,
                ..Default::default()
            },
        );
        assert!(!accounting.alab_held_goal_association_allowed(Some("old-goal")));
        let _ = accounting
            .alab_persistence
            .begin_mutation()
            .commit_mutation();
        let refreshed = accounting.alab_persistence.quarantined_usage();
        assert_eq!(refreshed[0].descendant_tokens, 18);
        assert_eq!(refreshed[0].turn_tokens, 9);
        assert!(!accounting.alab_persistence.permits_write());
        assert!(!accounting.alab_persistence.permits_admission());
    }

    #[test]
    fn frozen_same_goal_reactivation_retains_late_token_and_descendant_input() {
        codex_alab_core::activate();
        let accounting = GoalAccountingState::default();
        accounting.start_turn("turn", ModeKind::Default, &TokenUsage::default());
        accounting.mark_turn_goal_active("turn", "goal");
        assert!(accounting.alab_finish_all_empty_boundaries(false));
        accounting.record_token_usage(
            "turn",
            &TokenUsage {
                input_tokens: 5,
                total_tokens: 5,
                ..Default::default()
            },
        );
        accounting.record_descendant_token_usage(&TokenUsage {
            input_tokens: 7,
            total_tokens: 7,
            ..Default::default()
        });
        assert_eq!(
            accounting.mark_current_turn_goal_active("goal").as_deref(),
            Some("turn")
        );
        let snapshot = accounting.progress_snapshot("turn").unwrap();
        assert_eq!(snapshot.token_delta, 12);
        assert!(!accounting.alab_persistence.permits_admission());
    }

    #[test]
    fn same_goal_turn_start_during_mutation_keeps_rollback_elapsed_countable() {
        codex_alab_core::activate();
        let accounting = GoalAccountingState::default();
        accounting.start_turn("old-turn", ModeKind::Default, &TokenUsage::default());
        assert!(accounting.mark_turn_goal_active("old-turn", "goal"));
        let mut mutation = accounting
            .alab_begin_dispatched_mutation()
            .expect("clean mutation boundary");
        let five_seconds_ago = super::super::Instant::now()
            .checked_sub(Duration::from_secs(5))
            .unwrap();
        {
            let mut inner = accounting.inner();
            inner.wall_clock.last_accounted_at = five_seconds_ago;
            inner.wall_clock.frozen_at = Some(five_seconds_ago);
        }

        accounting.start_turn("new-turn", ModeKind::Default, &TokenUsage::default());
        assert!(accounting.mark_turn_goal_active("new-turn", "goal"));
        assert!(accounting.inner().wall_clock.frozen_at.is_some());

        accounting.alab_reopen_rolled_back_mutation();
        mutation.definitely_rolled_back();
        let inner = accounting.inner();
        assert!(inner.wall_clock.frozen_at.is_none());
        assert!(inner.wall_clock.time_delta_since_last_accounting() >= 5);
    }

    #[test]
    fn rollback_of_unknown_supersession_keeps_predecessor_clock_frozen() {
        codex_alab_core::activate();
        let accounting = GoalAccountingState::default();
        accounting.start_turn("turn", ModeKind::Default, &TokenUsage::default());
        assert!(accounting.mark_turn_goal_active("turn", "possibly-stale-goal"));
        let mut unknown = accounting
            .alab_begin_dispatched_mutation()
            .expect("first exact dispatch");
        unknown.settled_unknown();
        assert!(accounting.alab_persistence.settled_mutation_unknown());

        let mut superseding = accounting
            .alab_begin_dispatched_mutation()
            .expect("one exact superseding dispatch");
        assert!(!superseding.predecessor_is_authoritative());
        if superseding.predecessor_is_authoritative() {
            accounting.alab_reopen_rolled_back_mutation();
        }
        superseding.definitely_rolled_back();

        assert!(accounting.inner().wall_clock.frozen_at.is_some());
        assert!(accounting.alab_persistence.settled_mutation_unknown());
    }

    #[test]
    fn late_old_goal_callback_blocks_a_different_unscoped_turn_association() {
        codex_alab_core::activate();
        let accounting = GoalAccountingState::default();
        accounting.start_turn("old-turn", ModeKind::Default, &TokenUsage::default());
        assert!(accounting.mark_turn_goal_active("old-turn", "old-goal"));
        accounting.record_descendant_token_usage(&TokenUsage {
            input_tokens: 7,
            total_tokens: 7,
            ..Default::default()
        });
        accounting.start_turn("new-turn", ModeKind::Default, &TokenUsage::default());

        assert!(!accounting.mark_turn_goal_active("new-turn", "new-goal"));
        assert_eq!(
            accounting.alab_retained_goal_id("old-turn").as_deref(),
            Some("old-goal")
        );
        assert_eq!(accounting.alab_retained_goal_id("new-turn"), None);
        assert!(!accounting.alab_persistence.permits_admission());
    }

    #[test]
    fn dirty_attributed_turn_records_survive_start_interrupt_and_finish() {
        codex_alab_core::activate();
        for operation in ["start", "interrupt", "finish"] {
            let accounting = GoalAccountingState::default();
            accounting.start_turn("old", ModeKind::Default, &TokenUsage::default());
            assert!(accounting.mark_turn_goal_active("old", "goal"));
            if operation == "start" {
                accounting.interrupt_turn("old");
            } else {
                accounting.inner().current_turn_id = None;
            }
            accounting.record_token_usage(
                "old",
                &TokenUsage {
                    input_tokens: 5,
                    total_tokens: 5,
                    ..Default::default()
                },
            );

            match operation {
                "start" => accounting.start_turn("new", ModeKind::Default, &TokenUsage::default()),
                "interrupt" => accounting.interrupt_turn("old"),
                "finish" => accounting.finish_turn("old"),
                _ => unreachable!(),
            }

            assert_eq!(
                accounting.alab_retained_goal_id("old").as_deref(),
                Some("goal"),
                "{operation} cannot erase dirty attributed input"
            );
        }
    }

    #[tokio::test]
    async fn callback_after_additive_snapshot_prevents_clear_and_remains_visible() {
        codex_alab_core::activate();
        let accounting = Arc::new(GoalAccountingState::default());
        accounting.start_turn("turn", ModeKind::Default, &TokenUsage::default());
        accounting.mark_turn_goal_active("turn", "goal");
        accounting.record_token_usage(
            "turn",
            &TokenUsage {
                input_tokens: 5,
                total_tokens: 5,
                ..Default::default()
            },
        );
        let owner = Arc::clone(&accounting.alab_persistence);
        let work = Arc::clone(&accounting);
        codex_alab_core::goal_persistence::owned(owner, async move {
            let snapshot = work.progress_snapshot("turn").unwrap();
            assert!(work.inner().wall_clock.frozen_at.is_some());
            work.record_token_usage(
                "turn",
                &TokenUsage {
                    input_tokens: 8,
                    total_tokens: 8,
                    ..Default::default()
                },
            );
            work.mark_progress_accounted_for_status(
                "turn",
                &snapshot,
                codex_state::ThreadGoalStatus::Complete,
                super::super::BudgetLimitedGoalDisposition::ClearActive,
            );
            Ok::<_, ()>(())
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(
            accounting.alab_retained_goal_id("turn").as_deref(),
            Some("goal")
        );
        assert_eq!(accounting.progress_snapshot("turn").unwrap().token_delta, 3);
        assert!(!accounting.alab_persistence.permits_admission());
    }

    #[tokio::test]
    async fn empty_snapshots_settle_and_close_or_reopen_the_exact_clock() {
        codex_alab_core::activate();
        for (active_turn, clear) in [(false, false), (false, true), (true, false), (true, true)] {
            let accounting = Arc::new(GoalAccountingState::default());
            if active_turn {
                accounting.start_turn("turn", ModeKind::Default, &TokenUsage::default());
                assert!(accounting.mark_turn_goal_active("turn", "goal"));
            } else {
                accounting.mark_idle_goal_active("goal");
            }
            let owner = Arc::clone(&accounting.alab_persistence);
            let work = Arc::clone(&accounting);
            codex_alab_core::goal_persistence::owned(owner, async move {
                if active_turn {
                    assert!(work.progress_snapshot("turn").is_none());
                    if clear {
                        work.alab_close_empty_turn_boundary("turn");
                    } else {
                        work.alab_reopen_empty_turn_boundary("turn");
                    }
                } else {
                    assert!(work.idle_progress_snapshot().is_none());
                    if clear {
                        work.alab_close_empty_idle_boundary();
                    } else {
                        work.alab_reopen_empty_idle_boundary();
                    }
                }
                Ok::<_, ()>(())
            })
            .await
            .unwrap()
            .unwrap();

            assert!(accounting.alab_persistence.permits_admission());
            assert_eq!(
                accounting.inner().wall_clock.active_goal_id.as_deref(),
                (!clear).then_some("goal")
            );
            assert!(accounting.inner().wall_clock.frozen_at.is_none());
        }
    }

    #[tokio::test]
    async fn atomic_mutation_dispatch_retains_callbacks_on_the_frozen_old_goal() {
        codex_alab_core::activate();
        let accounting = GoalAccountingState::default();
        accounting.start_turn("turn", ModeKind::Default, &TokenUsage::default());
        assert!(accounting.mark_turn_goal_active("turn", "goal"));

        let write = accounting.alab_begin_dispatched_mutation().unwrap();
        accounting.record_token_usage(
            "turn",
            &TokenUsage {
                input_tokens: 4,
                total_tokens: 4,
                ..Default::default()
            },
        );
        let transition = write
            .commit_mutation()
            .expect("the durable mutation retains its pending-data effect");
        let ((), rejected, applied) =
            codex_alab_core::goal_persistence::mutation_effect(transition.effect, async {
                assert!(accounting.alab_preserves_pending_progress());
                accounting.alab_acknowledge_pending_same_goal_effect("goal");
            })
            .await;
        assert!(!rejected);
        assert!(applied);
        assert_eq!(accounting.progress_snapshot("turn").unwrap().token_delta, 4);
        assert!(!accounting.alab_persistence.permits_admission());
        assert!(accounting.alab_persistence.permits_write());
    }

    #[tokio::test]
    async fn pending_data_effect_quarantines_a_different_goal_boundary() {
        codex_alab_core::activate();
        let accounting = GoalAccountingState::default();
        accounting.start_turn("turn", ModeKind::Default, &TokenUsage::default());
        assert!(accounting.mark_turn_goal_active("turn", "old-goal"));

        let write = accounting.alab_begin_dispatched_mutation().unwrap();
        accounting.record_token_usage(
            "turn",
            &TokenUsage {
                input_tokens: 4,
                total_tokens: 4,
                ..Default::default()
            },
        );
        let effect = write
            .commit_mutation()
            .expect("the durable mutation retains its pending-data effect")
            .effect;
        let custody = effect.clone();
        let ((), rejected, applied) =
            codex_alab_core::goal_persistence::mutation_effect(effect, async {
                accounting.alab_acknowledge_pending_same_goal_effect("new-goal");
            })
            .await;
        assert!(!rejected);
        assert!(!applied);
        assert!(
            !accounting
                .alab_persistence
                .hold_if_mutation_effect_epoch_current(&custody),
            "quarantine already invalidates the committed effect epoch"
        );
        let quarantined = accounting.alab_persistence.quarantined_usage();
        assert_eq!(quarantined.len(), 1);
        assert_eq!(quarantined[0].original_goal_id, "old-goal");
        assert_eq!(quarantined[0].turn_tokens, 4);
        assert!(!accounting.alab_persistence.permits_write());
        assert!(!accounting.alab_persistence.permits_admission());
    }

    #[test]
    fn pre_dispatch_progress_rejection_leaves_the_old_clock_open_and_epoch_current() {
        codex_alab_core::activate();
        let accounting = GoalAccountingState::default();
        accounting.start_turn("turn", ModeKind::Default, &TokenUsage::default());
        assert!(accounting.mark_turn_goal_active("turn", "goal"));
        let epoch = accounting.alab_persistence.mutation_epoch().unwrap();
        accounting.record_token_usage(
            "turn",
            &TokenUsage {
                input_tokens: 3,
                total_tokens: 3,
                ..Default::default()
            },
        );

        assert!(matches!(
            accounting.alab_begin_dispatched_mutation(),
            Err(codex_alab_core::goal_persistence::DispatchedMutationError::Progress)
        ));
        assert!(accounting.inner().wall_clock.frozen_at.is_none());
        assert!(
            accounting
                .alab_persistence
                .mutation_epoch_is_current(&epoch)
        );
    }
}
