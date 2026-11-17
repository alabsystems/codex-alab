// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside the goal runtime; native policy and SQL remain in their owners.
use super::*;
use std::future::Future;
use std::pin::Pin;

#[derive(Clone, Copy)]
enum RetainedClockDisposition {
    Preserve,
    Freeze,
    Clear,
}

tokio::task_local! { static STOP_OWNER: (Arc<codex_alab_core::goal_persistence::State>, codex_alab_core::goal_persistence::MutationEpoch); }
tokio::task_local! { static HOST_GOAL_MUTATION_OWNER: codex_protocol::ThreadId; }

pub(crate) fn alab_host_goal_mutation_permit_held(thread_id: codex_protocol::ThreadId) -> bool {
    HOST_GOAL_MUTATION_OWNER
        .try_with(|owner| *owner == thread_id)
        .unwrap_or(false)
}

pub(crate) async fn alab_scope_host_goal_mutation<T>(
    thread_id: codex_protocol::ThreadId,
    future: impl Future<Output = T>,
) -> T {
    HOST_GOAL_MUTATION_OWNER.scope(thread_id, future).await
}

#[derive(Clone)]
pub(crate) struct GoalMutationBoundary {
    inner: Arc<GoalMutationBoundaryInner>,
}

struct GoalMutationBoundaryInner {
    runtime: Arc<GoalRuntimeHandle>,
    state: std::sync::Mutex<GoalMutationBoundaryState>,
}

#[derive(Default)]
struct GoalMutationBoundaryState {
    effect: Option<codex_alab_core::goal_persistence::MutationEffect>,
    bound: bool,
}

struct GoalMutationEffectApplication {
    runtime: GoalRuntimeHandle,
    effect: Option<codex_alab_core::goal_persistence::MutationEffect>,
    acknowledgement: codex_alab_core::goal_persistence::MutationEffectAcknowledgement,
}

impl GoalMutationEffectApplication {
    fn complete(&mut self) {
        self.effect = None;
    }
}

impl Drop for GoalMutationEffectApplication {
    fn drop(&mut self) {
        if !self.acknowledgement.is_applied()
            && let Some(effect) = &self.effect
        {
            self.runtime
                .accounting_state()
                .alab_persistence
                .hold_if_mutation_effect_epoch_current(effect);
        }
    }
}

impl std::fmt::Debug for GoalMutationBoundary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GoalMutationBoundary")
            .finish_non_exhaustive()
    }
}

impl GoalMutationBoundary {
    pub(crate) fn runtime(&self) -> Arc<GoalRuntimeHandle> {
        Arc::clone(&self.inner.runtime)
    }

    pub(crate) fn bound(
        runtime: Arc<GoalRuntimeHandle>,
        effect: codex_alab_core::goal_persistence::MutationEffect,
    ) -> Self {
        Self {
            inner: Arc::new(GoalMutationBoundaryInner {
                runtime,
                state: std::sync::Mutex::new(GoalMutationBoundaryState {
                    effect: Some(effect),
                    bound: true,
                }),
            }),
        }
    }

    pub(crate) fn bind(&self, expected: Option<codex_alab_core::goal_persistence::MutationEffect>) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.effect = expected;
        state.bound = true;
    }

    pub(crate) async fn scope<T>(&self, future: impl Future<Output = T>) -> Option<T> {
        let expected = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .effect
            .take()?;
        self.inner
            .runtime
            .alab_scope_exact_mutation_effect(expected, future)
            .await
    }
}

impl GoalRuntimeHandle {
    pub async fn alab_goal_chronology_generation(
        &self,
    ) -> Result<Option<codex_alab_core::goal_chronology_fence::FenceGeneration>, String> {
        let manager = self
            .inner
            .thread_manager
            .upgrade()
            .ok_or_else(|| "goal chronology thread manager is unavailable".to_owned())?;
        let thread = manager
            .get_thread(self.thread_id())
            .await
            .map_err(|error| error.to_string())?;
        codex_alab_core::goal_chronology_fence::read_current(
            &thread.config().await.codex_home,
            &self.thread_id().to_string(),
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn alab_invalidate_turn_start_options(&self) {
        let invalidations = &self.inner.alab_turn_options_invalidations;
        let mut current = invalidations.load(std::sync::atomic::Ordering::SeqCst);
        loop {
            if current == u64::MAX {
                return;
            }
            match invalidations.compare_exchange(
                current,
                current + 1,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    pub(crate) fn alab_release_turn_start_options_invalidation(&self) {
        let invalidations = &self.inner.alab_turn_options_invalidations;
        let mut current = invalidations.load(std::sync::atomic::Ordering::SeqCst);
        loop {
            if matches!(current, 0 | u64::MAX) {
                return;
            }
            match invalidations.compare_exchange(
                current,
                current - 1,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    pub(crate) fn alab_validate_future_turn_start_options(&self) {
        self.inner
            .alab_turn_options_invalidations
            .store(0, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn alab_turn_start_options_invalidated(&self) -> bool {
        self.inner
            .alab_turn_options_invalidations
            .load(std::sync::atomic::Ordering::SeqCst)
            != 0
    }

    pub(crate) async fn alab_scope_exact_mutation_effect<T>(
        &self,
        expected: codex_alab_core::goal_persistence::MutationEffect,
        future: impl Future<Output = T>,
    ) -> Option<T> {
        let mut application = GoalMutationEffectApplication {
            runtime: self.clone(),
            effect: Some(expected.clone()),
            acknowledgement:
                codex_alab_core::goal_persistence::MutationEffectAcknowledgement::default(),
        };
        if !self.alab_goal_mutation_effect_is_current(&expected)
            && !self.alab_goal_mutation_effect_has_only_pending_data(&expected)
        {
            return None;
        } // A late callback may invalidate destructive effects while the same-goal no-op still owes acknowledgement.
        let (output, _rejected) =
            codex_alab_core::goal_persistence::mutation_effect_with_acknowledgement(
                expected,
                application.acknowledgement.clone(),
                future,
            )
            .await;
        if application.acknowledgement.is_applied() {
            application.complete();
        }
        Some(output)
    }
}

impl Drop for GoalMutationBoundaryInner {
    fn drop(&mut self) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pending = state.bound.then(|| state.effect.clone()).flatten();
        drop(state);
        if let Some(effect) = pending {
            self.runtime
                .accounting_state()
                .alab_persistence
                .hold_if_mutation_effect_epoch_current(&effect);
        }
    }
}

impl GoalRuntimeHandle {
    pub(crate) fn alab_mutation_effect_epoch_authorized(&self) -> bool {
        !codex_alab_core::is_alab_binary()
            || self
                .inner
                .accounting_state
                .alab_persistence
                .mutation_effect_epoch_authorized()
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn alab_debug_goal_persistence_write(&self) -> codex_alab_core::goal_persistence::Write {
        self.inner.accounting_state.alab_persistence.begin()
    }

    /// Build the exact terminal-callback boundary without racing a provider
    /// stream. Debug integration tests use this to exercise the ending-turn
    /// finalizer after a durable mutation was dispatched and callback data
    /// arrived before that mutation published its transition.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn alab_debug_begin_dispatched_goal_mutation_with_usage(
        &self,
        turn_id: &str,
        goal_id: &str,
        usage: &codex_protocol::protocol::TokenUsage,
    ) -> Result<codex_alab_core::goal_persistence::Write, String> {
        let accounting = self.accounting_state();
        accounting.reset_idle_progress_baseline_and_clear_active_goal();
        accounting.start_turn(
            turn_id,
            codex_protocol::config_types::ModeKind::Default,
            &codex_protocol::protocol::TokenUsage::default(),
        );
        if !accounting.mark_turn_goal_active(turn_id, goal_id) {
            return Err("debug turn could not retain the expected goal".to_owned());
        }
        let write = accounting
            .alab_begin_dispatched_mutation()
            .map_err(|error| format!("debug mutation dispatch was refused: {error:?}"))?;
        if accounting.record_token_usage(turn_id, usage).is_none() {
            return Err("debug callback did not publish attributable usage".to_owned());
        }
        Ok(write)
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn alab_debug_account_ending_turn_progress(
        &self,
        turn_id: &str,
        event_id: &str,
    ) -> Result<(), String> {
        self.alab_account_ending_turn_progress(turn_id, event_id)
            .await
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn alab_debug_finish_goal_turn(&self, turn_id: &str) {
        self.accounting_state().finish_turn(turn_id);
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn alab_debug_quarantine_goal_persistence(&self, goal_id: &str) {
        self.inner
            .accounting_state
            .alab_persistence
            .quarantine_original_goal(codex_alab_core::goal_persistence::QuarantinedUsage {
                original_goal_id: goal_id.to_owned(),
                elapsed_seconds: 1,
                descendant_tokens: 1,
                turn_tokens: 1,
            });
    }

    pub(crate) fn alab_seed_goal_persistence_carry(
        &self,
        carry: &codex_alab_core::goal_persistence::PersistenceCarry,
    ) {
        self.inner
            .accounting_state
            .alab_persistence
            .seed_carry(carry);
    }

    pub(crate) fn alab_permanently_hold_goal_persistence(&self) {
        self.inner
            .accounting_state
            .alab_persistence
            .hold_permanently();
    }

    pub(crate) fn alab_retired_goal_persistence_carry(
        &self,
    ) -> codex_alab_core::goal_persistence::PersistenceCarry {
        self.inner
            .accounting_state
            .alab_persistence
            .retirement_carry()
    }

    pub fn alab_goal_mutation_epoch(
        &self,
    ) -> Option<codex_alab_core::goal_persistence::MutationEpoch> {
        self.inner
            .accounting_state
            .alab_persistence
            .mutation_epoch()
    }

    pub fn alab_goal_mutation_epoch_is_current(
        &self,
        epoch: &codex_alab_core::goal_persistence::MutationEpoch,
    ) -> bool {
        self.inner
            .accounting_state
            .alab_persistence
            .mutation_epoch_is_current(epoch)
    }

    pub(crate) fn alab_goal_mutation_effect_is_current(
        &self,
        effect: &codex_alab_core::goal_persistence::MutationEffect,
    ) -> bool {
        self.inner
            .accounting_state
            .alab_persistence
            .mutation_effect_is_current(effect)
    }

    pub(crate) fn alab_goal_mutation_effect_has_only_pending_data(
        &self,
        effect: &codex_alab_core::goal_persistence::MutationEffect,
    ) -> bool {
        self.inner
            .accounting_state
            .alab_persistence
            .mutation_effect_has_only_pending_data(effect)
    }

    pub fn alab_goal_mutation_epoch_is_pristine(
        &self,
        epoch: &codex_alab_core::goal_persistence::MutationEpoch,
    ) -> bool {
        self.inner
            .accounting_state
            .alab_persistence
            .mutation_epoch_is_pristine(epoch)
    }

    pub fn alab_model_goal_epoch(
        &self,
    ) -> Option<codex_alab_core::goal_persistence::MutationEpoch> {
        self.alab_goal_mutation_epoch()
    }

    pub fn alab_model_goal_epoch_current(
        &self,
        epoch: &codex_alab_core::goal_persistence::MutationEpoch,
    ) -> bool {
        self.alab_goal_mutation_epoch_is_current(epoch)
    }

    pub(crate) fn alab_begin_goal_mutation(
        &self,
    ) -> Option<codex_alab_core::goal_persistence::Write> {
        if !codex_alab_core::is_alab_binary() {
            return None;
        }
        let write = self
            .inner
            .accounting_state
            .alab_persistence
            .begin_mutation();
        if write.mutation_epoch().is_some() {
            Some(write)
        } else {
            write.finish(false);
            None
        }
    }

    /// Observable backpressure; this grants neither write nor continuation authority.
    pub fn alab_goal_persistence_held(&self) -> bool {
        codex_alab_core::is_alab_binary()
            && !self
                .inner
                .accounting_state
                .alab_persistence
                .permits_admission()
    }

    /// Read-only owner phase, separate from an unfinished validation operation.
    pub fn alab_accounting_write_pending(&self) -> bool {
        codex_alab_core::is_alab_binary()
            && self
                .inner
                .accounting_state
                .alab_persistence
                .accounting_write_pending()
    }

    /// Do not consume an exact native usage snapshot until its SQLite update is
    /// power-loss durable. A failed checkpoint is outcome-unknown: retain the
    /// snapshot and close admission instead of allowing additive replay.
    pub(super) async fn alab_make_accounting_durable(&self) -> Result<(), String> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(());
        }
        if let Err(error) = self
            .inner
            .state_dbs
            .thread_goals()
            .alab_durable_goal_barrier()
            .await
        {
            self.inner
                .accounting_state
                .alab_persistence
                .block_ambiguous();
            return Err(format!(
                "goal usage committed but its durability could not be proven: {error}"
            ));
        }
        Ok(())
    }

    /// A native terminal status must reach the same durable boundary as its
    /// already-settled usage before runtime effects can clear the active goal.
    pub(super) async fn alab_make_stop_durable(
        &self,
        mutation: &mut Option<codex_alab_core::goal_persistence::Write>,
    ) -> Result<(), String> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(());
        }
        if let Err(error) = self
            .inner
            .state_dbs
            .thread_goals()
            .alab_durable_goal_barrier()
            .await
        {
            self.alab_settle_mutation_unknown(mutation);
            return Err(format!(
                "goal stop committed but its durability could not be proven: {error}"
            ));
        }
        Ok(())
    }

    pub(super) fn alab_stop_still_current() -> bool {
        STOP_OWNER
            .try_with(|(state, epoch)| state.mutation_epoch_is_current(epoch))
            .unwrap_or(true)
    }

    pub(super) fn alab_owns_stop() -> bool {
        STOP_OWNER.try_with(|_| ()).is_ok()
    }

    pub(super) fn alab_owned_active_progress(
        &self,
        turn_id: &str,
        event_id: &str,
        mode: codex_state::GoalAccountingMode,
        disposition: BudgetLimitedGoalDisposition,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AccountedGoalProgress>, String>> + Send + '_>>
    {
        let state = Arc::clone(&self.inner.accounting_state.alab_persistence);
        let accounting = Arc::clone(&self.inner.accounting_state);
        let runtime = self.clone();
        let turn_id = turn_id.to_string();
        let boundary_turn_id = turn_id.clone();
        let event_id = event_id.to_string();
        Box::pin(async move {
            codex_alab_core::goal_persistence::owned(state, async move {
                let progress = runtime
                    .account_active_goal_progress(&turn_id, &event_id, mode, disposition)
                    .await?;
                if progress.is_none() {
                    match disposition {
                        BudgetLimitedGoalDisposition::ClearActive => {
                            accounting.alab_close_empty_turn_boundary(&boundary_turn_id);
                        }
                        BudgetLimitedGoalDisposition::KeepActive => {
                            accounting.alab_reopen_empty_turn_boundary(&boundary_turn_id);
                        }
                    }
                }
                Ok(progress)
            })
            .await
            .map_err(|error| {
                format!("goal accounting owner ended before acknowledgement: {error}")
            })?
        })
    }

    pub(super) fn alab_owned_idle_progress(
        &self,
        event_id: &str,
        mode: codex_state::GoalAccountingMode,
        disposition: BudgetLimitedGoalDisposition,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AccountedGoalProgress>, String>> + Send + '_>>
    {
        let state = Arc::clone(&self.inner.accounting_state.alab_persistence);
        let accounting = Arc::clone(&self.inner.accounting_state);
        let runtime = self.clone();
        let event_id = event_id.to_string();
        Box::pin(async move {
            codex_alab_core::goal_persistence::owned(state, async move {
                let progress = runtime
                    .account_idle_goal_progress(&event_id, mode, disposition)
                    .await?;
                if progress.is_none() {
                    match disposition {
                        BudgetLimitedGoalDisposition::ClearActive => {
                            accounting.alab_close_empty_idle_boundary();
                        }
                        BudgetLimitedGoalDisposition::KeepActive => {
                            accounting.alab_reopen_empty_idle_boundary();
                        }
                    }
                }
                Ok(progress)
            })
            .await
            .map_err(|error| {
                format!("idle goal accounting owner ended before acknowledgement: {error}")
            })?
        })
    }

    pub(super) fn alab_owned_stop(
        &self,
        turn_id: &str,
        reason: ActiveGoalStopReason,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let state = Arc::clone(&self.inner.accounting_state.alab_persistence);
        let Some(epoch) = state.mutation_epoch_without_active_mutation() else {
            return Box::pin(async {
                Err("the native goal mutation epoch is exhausted".to_owned())
            });
        };
        let runtime = self.clone();
        let turn_id = turn_id.to_string();
        Box::pin(async move {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            tokio::spawn(STOP_OWNER.scope((state, epoch), async move {
                let result = runtime.stop_active_goal_for_turn(&turn_id, reason).await;
                let _ = sender.send(result);
            }));
            receiver
                .await
                .map_err(|error| format!("goal stop owner ended before acknowledgement: {error}"))?
        })
    }

    pub(super) fn alab_begin_stop_mutation(
        &self,
    ) -> Result<
        Option<codex_alab_core::goal_persistence::Write>,
        codex_alab_core::goal_persistence::DispatchedMutationError,
    > {
        if !codex_alab_core::is_alab_binary() {
            return Ok(None);
        }
        self.inner
            .accounting_state
            .alab_begin_dispatched_mutation()
            .map(Some)
    }

    pub(super) fn alab_commit_stop_mutation(
        &self,
        mutation: Option<codex_alab_core::goal_persistence::Write>,
    ) -> Option<codex_alab_core::goal_persistence::MutationEffect> {
        let transition =
            mutation.and_then(codex_alab_core::goal_persistence::Write::commit_mutation);
        if transition.is_some() {
            self.alab_reconcile_unowned_goal_mutation();
        }
        transition.map(|transition| transition.effect)
    }

    pub(super) fn alab_definitely_rollback_mutation(
        &self,
        mutation: &mut Option<codex_alab_core::goal_persistence::Write>,
    ) {
        if let Some(mutation) = mutation {
            if mutation.predecessor_is_authoritative() {
                self.accounting_state().alab_reopen_rolled_back_mutation();
            }
            mutation.definitely_rolled_back();
        }
    }

    pub(super) fn alab_settle_mutation_unknown(
        &self,
        mutation: &mut Option<codex_alab_core::goal_persistence::Write>,
    ) {
        if let Some(mutation) = mutation {
            mutation.settled_unknown();
        }
    }

    pub(super) fn alab_settle_mutation_error(
        &self,
        mutation: &mut Option<codex_alab_core::goal_persistence::Write>,
        error: &anyhow::Error,
    ) {
        if codex_state::alab_goal_read_recovery::mutation_definitely_uncommitted(error) {
            self.alab_definitely_rollback_mutation(mutation);
        } else {
            self.alab_settle_mutation_unknown(mutation);
        }
    }

    pub(super) async fn alab_apply_stop_mutation_effect(
        &self,
        effect: codex_alab_core::goal_persistence::MutationEffect,
    ) -> bool {
        let custody = effect.clone();
        let (_, rejected, applied) =
            codex_alab_core::goal_persistence::mutation_effect(effect, async {
                self.inner.accounting_state.clear_active_goal();
                if !codex_alab_core::goal_persistence::mutation_effect_rejected() {
                    codex_alab_core::goal_persistence::mutation_effect_applied();
                }
            })
            .await;
        if rejected || !applied {
            self.inner
                .accounting_state
                .alab_persistence
                .hold_if_mutation_effect_epoch_current(&custody);
            false
        } else {
            true
        }
    }

    pub(crate) async fn alab_drain_retained_progress(&self) -> Result<(), String> {
        self.alab_drain_retained_progress_with(
            BudgetLimitedGoalDisposition::ClearActive,
            RetainedClockDisposition::Clear,
        )
        .await
    }

    pub(crate) async fn alab_drain_retained_progress_for_mutation(&self) -> Result<(), String> {
        self.alab_drain_retained_progress_with(
            BudgetLimitedGoalDisposition::KeepActive,
            RetainedClockDisposition::Preserve,
        )
        .await
    }

    /// Final upgrade capture owns every remaining source-row write. It drains
    /// retained turns and idle usage, then freezes the exact active-goal clock
    /// without clearing the baseline which the destination must adopt.
    pub(crate) async fn alab_drain_retained_progress_for_upgrade(&self) -> Result<(), String> {
        self.alab_drain_retained_progress_with(
            BudgetLimitedGoalDisposition::KeepActive,
            RetainedClockDisposition::Freeze,
        )
        .await
    }

    /// Deferred committed-upgrade retirement runs after final state capture and
    /// therefore may perform no SQLite write. Native permits exclude every
    /// producer while this closes the frozen clock and proves an empty boundary.
    pub(crate) fn alab_finish_deferred_upgrade_retirement(&self) -> Result<(), String> {
        self.alab_reconcile_retained_progress(RetainedClockDisposition::Clear)
    }

    pub(crate) fn alab_freeze_goal_mutation_boundary(
        self: &Arc<Self>,
    ) -> Result<
        (
            GoalMutationBoundary,
            codex_alab_core::goal_persistence::Write,
        ),
        codex_alab_core::goal_persistence::DispatchedMutationError,
    > {
        self.accounting_state()
            .alab_begin_dispatched_mutation()
            .map(|write| {
                self.alab_invalidate_turn_start_options();
                (
                    GoalMutationBoundary {
                        inner: Arc::new(GoalMutationBoundaryInner {
                            runtime: Arc::clone(self),
                            state: std::sync::Mutex::new(GoalMutationBoundaryState {
                                effect: None,
                                bound: true,
                            }),
                        }),
                    },
                    write,
                )
            })
    }

    async fn alab_drain_retained_progress_with(
        &self,
        disposition: BudgetLimitedGoalDisposition,
        clock: RetainedClockDisposition,
    ) -> Result<(), String> {
        self.alab_drain_other_progress_with(None, disposition)
            .await?;
        if !codex_alab_core::is_alab_binary() {
            return Ok(());
        }
        let accounting = self.accounting_state();
        if let Some(snapshot) = accounting.idle_progress_snapshot() {
            let validation = accounting.alab_persistence.begin();
            let goal = self
                .inner
                .state_dbs
                .thread_goals()
                .get_thread_goal(self.thread_id())
                .await
                .map_err(|error| error.to_string())?;
            let Some(goal) = goal.filter(|goal| goal.goal_id == snapshot.expected_goal_id) else {
                accounting.alab_quarantine_pending_boundaries();
                return Err("retained idle usage belongs to a different or missing goal; the original boundary remains quarantined".to_owned());
            };
            // The caller retains its native permit. From dispatch onward the
            // additive task owns cancellation and baseline finalization; an
            // observer's preparation guard must not manufacture a failed write.
            validation.finish(true);
            self.account_idle_goal_progress(
                &format!("{}:retained-idle-goal-mutation", self.thread_id()),
                Self::alab_retained_accounting_mode(goal.status),
                disposition,
            )
            .await?;
        }
        self.alab_reconcile_retained_progress(clock)
    }

    fn alab_reconcile_retained_progress(
        &self,
        clock: RetainedClockDisposition,
    ) -> Result<(), String> {
        let accounting = self.accounting_state();
        let boundary_ready = match clock {
            RetainedClockDisposition::Preserve => true,
            RetainedClockDisposition::Freeze => accounting.alab_freeze_all_empty_boundaries(),
            RetainedClockDisposition::Clear => accounting.alab_close_all_empty_boundaries(),
        };
        if !boundary_ready {
            return Err(
                "goal accounting changed before its empty clock boundary could be frozen".into(),
            );
        }
        let Some(drain_generation) = accounting.alab_persistence.reconciliation_candidate() else {
            return Err("goal accounting has an unsettled or unknown write outcome".into());
        };
        if accounting
            .alab_retained_turn_ids()
            .into_iter()
            .any(|turn| accounting.progress_snapshot(&turn).is_some())
            || accounting.idle_progress_snapshot().is_some()
        {
            return Err("goal accounting changed during its final retained-progress scan".into());
        }
        if !accounting
            .alab_persistence
            .mark_accounting_reconciled(drain_generation)
        {
            return Err(
                "goal accounting changed while its retained snapshot was reconciled".into(),
            );
        }
        Ok(())
    }

    /// Plan's native clear must exclude Plan time. Drain first under the native
    /// permit, before turn-start takes its differently ordered accounting lock.
    /// If storage still refuses, quarantine the original boundary before reset.
    pub(crate) async fn alab_prepare_plan_accounting(&self) {
        let accounting = self.accounting_state();
        if !accounting.alab_needs_accounting_drain() {
            return;
        }
        let prepared = async {
            let _permit = self.goal_state_permit().await?;
            self.alab_drain_retained_progress().await
        }
        .await;
        if let Err(error) = prepared {
            accounting.alab_quarantine_pending_boundaries();
            tracing::warn!(%error, "Plan input retained an unacknowledged original goal boundary without replay");
        }
    }

    fn alab_retained_accounting_mode(
        status: codex_state::ThreadGoalStatus,
    ) -> codex_state::GoalAccountingMode {
        match status {
            codex_state::ThreadGoalStatus::Active
            | codex_state::ThreadGoalStatus::BudgetLimited => {
                codex_state::GoalAccountingMode::ActiveOnly
            }
            codex_state::ThreadGoalStatus::Complete => {
                codex_state::GoalAccountingMode::ActiveOrComplete
            }
            codex_state::ThreadGoalStatus::Paused
            | codex_state::ThreadGoalStatus::Blocked
            | codex_state::ThreadGoalStatus::UsageLimited => {
                codex_state::GoalAccountingMode::ActiveOrStopped
            }
        }
    }

    pub(crate) async fn alab_account_ending_turn_progress(
        &self,
        turn_id: &str,
        event_id: &str,
    ) -> Result<(), String> {
        if !codex_alab_core::is_alab_binary() {
            self.account_active_goal_progress(
                turn_id,
                event_id,
                codex_state::GoalAccountingMode::ActiveOnly,
                BudgetLimitedGoalDisposition::ClearActive,
            )
            .await?;
            return Ok(());
        }
        let accounting = self.accounting_state();
        if !accounting
            .alab_persistence
            .has_only_pending_accounting_data()
        {
            self.account_active_goal_progress(
                turn_id,
                event_id,
                codex_state::GoalAccountingMode::ActiveOnly,
                BudgetLimitedGoalDisposition::ClearActive,
            )
            .await?;
            return Ok(());
        }
        let _goal_permit = self.goal_state_permit().await?;
        if !accounting
            .alab_persistence
            .has_only_pending_accounting_data()
        {
            self.account_active_goal_progress(
                turn_id,
                event_id,
                codex_state::GoalAccountingMode::ActiveOnly,
                BudgetLimitedGoalDisposition::ClearActive,
            )
            .await?;
            return Ok(());
        }
        let Some(expected_goal_id) = accounting.alab_retained_goal_id(turn_id) else {
            self.account_active_goal_progress(
                turn_id,
                event_id,
                codex_state::GoalAccountingMode::ActiveOnly,
                BudgetLimitedGoalDisposition::ClearActive,
            )
            .await?;
            return Ok(());
        };
        let validation = accounting.alab_persistence.begin();
        let goal = self
            .inner
            .state_dbs
            .thread_goals()
            .get_thread_goal(self.thread_id())
            .await
            .map_err(|error| error.to_string())?;
        let Some(goal) = goal.filter(|goal| goal.goal_id == expected_goal_id) else {
            accounting.alab_quarantine_pending_boundaries();
            return Err(
                "ending-turn usage belongs to a different or missing goal; the original boundary remains quarantined"
                    .to_owned(),
            );
        };
        validation.finish(true);
        self.account_active_goal_progress(
            turn_id,
            event_id,
            Self::alab_retained_accounting_mode(goal.status),
            BudgetLimitedGoalDisposition::ClearActive,
        )
        .await?;
        Ok(())
    }

    /// Caller owns the native goal permit. Keep the ending turn for its own
    /// native status/policy path; account all older retained records first.
    pub(crate) async fn alab_drain_other_progress(
        &self,
        ending_turn: Option<&str>,
    ) -> Result<(), String> {
        self.alab_drain_other_progress_with(ending_turn, BudgetLimitedGoalDisposition::ClearActive)
            .await
    }

    async fn alab_drain_other_progress_with(
        &self,
        ending_turn: Option<&str>,
        disposition: BudgetLimitedGoalDisposition,
    ) -> Result<(), String> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(());
        }
        let accounting = self.accounting_state();
        if !accounting.alab_persistence.quarantined_usage().is_empty() {
            return Err("original goal usage is quarantined after an uncoordinated replacement; it remains held without replay".to_owned());
        }
        let turns: Vec<_> = accounting
            .alab_retained_turn_ids()
            .into_iter()
            .filter(|turn| Some(turn.as_str()) != ending_turn)
            .collect();
        for turn_id in turns {
            let Some(expected_goal_id) = accounting.alab_retained_goal_id(&turn_id) else {
                if accounting.current_turn_id().as_deref() != Some(&turn_id) {
                    accounting.finish_turn(&turn_id);
                }
                continue;
            };
            let validation = accounting.alab_persistence.begin();
            let goal = self
                .inner
                .state_dbs
                .thread_goals()
                .get_thread_goal(self.thread_id())
                .await
                .map_err(|error| error.to_string())?;
            let Some(goal) = goal.filter(|goal| goal.goal_id == expected_goal_id) else {
                accounting.alab_quarantine_pending_boundaries();
                return Err("retained usage belongs to a different or missing goal; it was preserved without attribution".to_owned());
            };
            validation.finish(true);
            if accounting.progress_snapshot(&turn_id).is_none() {
                if disposition == BudgetLimitedGoalDisposition::ClearActive
                    && accounting.current_turn_id().as_deref() != Some(&turn_id)
                {
                    accounting.finish_turn(&turn_id);
                }
                continue;
            }
            let mode = Self::alab_retained_accounting_mode(goal.status);
            let progress = self
                .account_active_goal_progress(
                    &turn_id,
                    &format!("{turn_id}:retained-goal-mutation"),
                    mode,
                    disposition,
                )
                .await?;
            if progress.is_none() {
                // A concurrently completed owner may have consumed this snapshot.
                // An Unchanged result itself never authorizes dropping a record.
                if accounting.progress_snapshot(&turn_id).is_some() {
                    accounting.alab_persistence.begin().finish(false);
                    return Err("the original goal did not accept its retained usage; the snapshot remains held".to_owned());
                }
                continue;
            }
            if disposition == BudgetLimitedGoalDisposition::ClearActive
                && accounting.current_turn_id().as_deref() != Some(&turn_id)
            {
                accounting.finish_turn(&turn_id);
            }
        }
        let Some(drain_generation) = accounting.alab_persistence.reconciliation_candidate() else {
            return Err("goal accounting has an unsettled or unknown write outcome".into());
        };
        if accounting
            .alab_retained_turn_ids()
            .into_iter()
            .filter(|turn| Some(turn.as_str()) != ending_turn)
            .any(|turn| accounting.progress_snapshot(&turn).is_some())
        {
            return Err("goal accounting changed during its final retained-turn scan".into());
        }
        if !accounting
            .alab_persistence
            .mark_accounting_reconciled(drain_generation)
        {
            return Err("goal accounting changed while retained progress was reconciled".into());
        }
        Ok(())
    }
}
