// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside the native goal runtime. ALab's automatic plain input uses the
//! same mutation boundary as the goal runtime's own idle continuation and upgrade checkpoints.

impl super::GoalRuntimeHandle {
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn alab_debug_progress_accounting_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        self.inner
            .accounting_state
            .progress_accounting_permit()
            .await
            .map_err(|error| error.to_string())
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn alab_debug_retain_upgrade_goal_usage(
        &self,
        turn: &str,
        goal: &str,
        tokens: i64,
    ) -> Result<(), String> {
        let accounting = &self.inner.accounting_state;
        accounting.start_turn(
            turn,
            codex_protocol::config_types::ModeKind::Default,
            &codex_protocol::protocol::TokenUsage::default(),
        );
        if !accounting.mark_turn_goal_active(turn, goal) {
            return Err("debug retained turn could not bind its goal".into());
        }
        let usage = codex_protocol::protocol::TokenUsage {
            input_tokens: tokens,
            total_tokens: tokens,
            ..Default::default()
        };
        if accounting.record_token_usage(turn, &usage).is_none() {
            return Err("debug retained turn produced no positive usage".into());
        }
        accounting.finish_turn(turn);
        if accounting.alab_retained_goal_id(turn).as_deref() != Some(goal) {
            return Err("debug retained turn did not preserve its goal boundary".into());
        }
        Ok(())
    }

    /// Serialize a committed model-tool observer with native goal mutation.
    /// Persistence admission is checked later when automatic work tries to use
    /// the binding; unrelated accounting must not erase exact source proof.
    pub async fn alab_model_goal_epoch_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        self.goal_state_permit().await
    }

    /// Keep the last goal read and automatic native admission ordered with external
    /// set/clear. Native turn-start accounting uses its separate accounting permit.
    pub async fn alab_automatic_input_permit(
        &self,
    ) -> Result<codex_alab_core::retained_goal_gate::AutomaticPermit<'_>, String> {
        let retained = self
            .alab_retained_goal_admission()
            .await
            .map_err(|()| "retained frontend continuation remains held".to_owned())?;
        let permit = self.goal_state_permit().await?;
        if self.alab_defer_for_goal_persistence() {
            return Err(
                "automatic input is held after failed or unfinished goal persistence".to_owned(),
            );
        }
        Ok(codex_alab_core::retained_goal_gate::AutomaticPermit::new(
            permit, retained,
        ))
    }

    /// Flush idle progress and read the active goal under the native mutation
    /// permit, then retain a revocable transfer. New work revokes this attempt
    /// before its mutation; final commit serializes with admission without
    /// blocking UI teardown.
    /// The caller additionally proves the primary thread and resumable native frame.
    pub async fn alab_upgrade_goal_checkpoint(
        &self,
    ) -> Result<
        (
            codex_state::ThreadGoal,
            codex_alab_core::upgrade_barrier::Transfer,
        ),
        String,
    > {
        if !self.tools_available() {
            return Err("the native goal runtime cannot execute goal work".into());
        }
        let (goal, transfer) = self
            .alab_upgrade_goal_state_checkpoint(self.thread_id())
            .await?;
        if !self.alab_upgrade_goal_ready() {
            return Err("the native goal runtime changed during its checkpoint".into());
        }
        let goal = goal
            .filter(|goal| goal.status == codex_state::ThreadGoalStatus::Active)
            .ok_or("no active persisted goal to carry")?;
        Ok((goal, transfer))
    }

    /// Checkpoint the actual runtime's complete goal state, including absence or
    /// a stopped goal. This data is not permission to resume automatic work.
    /// New goal mutations revoke the transfer before changing the row.
    pub async fn alab_upgrade_goal_state_checkpoint(
        &self,
        expected_thread: codex_protocol::ThreadId,
    ) -> Result<
        (
            Option<codex_state::ThreadGoal>,
            codex_alab_core::upgrade_barrier::Transfer,
        ),
        String,
    > {
        if self.thread_id() != expected_thread {
            return Err("goal checkpoint belongs to another native thread".into());
        }
        // A database worker can commit before its awaiting future is polled
        // again. Keep the native permits through the accounting baseline update
        // even when the source preparation or its caller stops waiting.
        let owner = self.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        drop(tokio::spawn(async move {
            let result = owner
                .alab_owned_upgrade_goal_state_checkpoint(expected_thread)
                .await;
            // An abandoned result drops its transfer and reopens admission.
            let _ = send.send(result);
        }));
        receive
            .await
            .map_err(|_| "native goal checkpoint ended without a result".to_owned())?
    }

    async fn alab_owned_upgrade_goal_state_checkpoint(
        &self,
        expected_thread: codex_protocol::ThreadId,
    ) -> Result<
        (
            Option<codex_state::ThreadGoal>,
            codex_alab_core::upgrade_barrier::Transfer,
        ),
        String,
    > {
        if self.thread_id() != expected_thread {
            return Err("goal checkpoint belongs to another native thread".into());
        }
        let permit = self.goal_state_permit().await?;
        if !self.alab_upgrade_goal_state_ready() {
            return Err("the native goal runtime still owns work or recovery".into());
        }
        let goals = self.inner.state_dbs.thread_goals();
        if goals
            .has_thread_goal_continuation_deferral(self.thread_id())
            .await
            .map_err(|error| error.to_string())?
        {
            return Err("the goal has a persisted continuation deferral".into());
        }
        // Use native accounting before the next process resets its idle clock.
        // Its empty-progress path preserves the current baseline; a refused
        // handoff must not lose elapsed time or account the same delta twice.
        self.account_idle_goal_progress(
            &format!("{}:alab-upgrade-checkpoint", self.thread_id()),
            codex_state::GoalAccountingMode::ActiveOnly,
            crate::accounting::BudgetLimitedGoalDisposition::KeepActive,
        )
        .await?;
        let goal = goals
            .get_thread_goal(self.thread_id())
            .await
            .map_err(|error| error.to_string())?;
        if let Some(goal) = &goal
            && goal.status == codex_state::ThreadGoalStatus::Active
        {
            // Refuse an unsupported clock while source mutation is still revocable.
            self.inner.accounting_state.alab_goal_clock(&goal.goal_id)?;
        }
        if !self.alab_upgrade_goal_state_ready() {
            return Err("the native goal runtime changed during its checkpoint".into());
        }
        let transfer = self
            .inner
            .alab_upgrade
            .prepare()
            .map_err(|error| error.to_string())?;
        drop(permit);
        Ok((goal, transfer))
    }
    fn alab_upgrade_goal_ready(&self) -> bool {
        self.tools_available() && self.alab_upgrade_goal_state_ready()
    }
    fn alab_upgrade_goal_state_ready(&self) -> bool {
        !self.alab_goal_persistence_held()
            && self.inner.accounting_state.current_turn_id().is_none()
            && self.inner.accounting_state.interrupted_turn_id().is_none()
            && !self.alab_auth_recovery_deferred()
            && !self.inner.alab_recovery.pending()
    }
    fn alab_final_upgrade_goal_state_ready(&self) -> bool {
        self.inner
            .accounting_state
            .alab_persistence
            .permits_final_reconciliation()
            && self.inner.accounting_state.current_turn_id().is_none()
            && self.inner.accounting_state.interrupted_turn_id().is_none()
            && !self.alab_auth_recovery_deferred()
            && !self.inner.alab_recovery.pending()
    }

    /// Combine the actual runtime's final readiness with the native handoff decision.
    /// A token from another runtime cannot commit this runtime's goal admission.
    pub fn alab_commit_goal_upgrade(
        &self,
        transfer: &mut codex_alab_core::upgrade_barrier::Transfer,
        decide: impl FnOnce() -> bool,
    ) -> bool {
        transfer.belongs_to(&self.inner.alab_upgrade)
            && transfer.commit_if(|| self.alab_upgrade_goal_ready() && decide())
    }

    /// Commit captured goal data even for a runtime whose goal tools are disabled.
    /// This preserves its state guard and grants no permission to continue work.
    pub fn alab_commit_goal_state_upgrade(
        &self,
        transfer: &mut codex_alab_core::upgrade_barrier::Transfer,
        decide: impl FnOnce() -> bool,
    ) -> bool {
        transfer.belongs_to(&self.inner.alab_upgrade)
            && transfer.commit_if(|| self.alab_upgrade_goal_state_ready() && decide())
    }

    /// Final source accounting after the caller joins all original Core producers.
    /// Only this runtime's committed token permits a read without reopening its
    /// mutation semaphore. This returns data, never continuation permission.
    pub async fn alab_final_upgrade_goal_state(
        &self,
        expected_thread: codex_protocol::ThreadId,
        transfer: &codex_alab_core::upgrade_barrier::Transfer,
    ) -> Result<
        (
            Option<codex_state::ThreadGoal>,
            Option<codex_alab_core::goal_clock::Stamp>,
        ),
        String,
    > {
        if self.thread_id() != expected_thread
            || !transfer.belongs_to(&self.inner.alab_upgrade)
            || !transfer.committed()
            || !self.alab_final_upgrade_goal_state_ready()
        {
            return Err("final goal accounting requires its exact committed native runtime".into());
        }
        self.alab_drain_retained_progress_for_upgrade().await?;
        let goal = self
            .inner
            .state_dbs
            .thread_goals()
            .get_thread_goal(self.thread_id())
            .await
            .map_err(|error| error.to_string())?;
        let clock = match &goal {
            Some(goal) if goal.status == codex_state::ThreadGoalStatus::Active => {
                self.inner.accounting_state.alab_goal_clock(&goal.goal_id)?
            }
            _ => None,
        };
        Ok((goal, clock))
    }

    /// Adopt source time data only after a fresh runtime validates its actual
    /// persisted goal. The caller must separately hold startup/SDK launch gates.
    /// Repeating an adoption cannot rewind subsequent native accounting.
    pub async fn alab_restore_upgrade_goal_clock(
        &self,
        expected_thread: codex_protocol::ThreadId,
        expected_goal: &codex_state::ThreadGoal,
        clock: codex_alab_core::goal_clock::Stamp,
    ) -> Result<(), String> {
        if self.thread_id() != expected_thread
            || expected_goal.thread_id != expected_thread
            || expected_goal.status != codex_state::ThreadGoalStatus::Active
        {
            return Err("goal clock data belongs to another native goal".into());
        }
        let _permit = self.goal_state_permit().await?;
        if !self.alab_upgrade_goal_state_ready() || !self.is_enabled() {
            return Err("goal clock restoration requires an enabled idle native runtime".into());
        }
        let current = self
            .inner
            .state_dbs
            .thread_goals()
            .get_thread_goal(expected_thread)
            .await
            .map_err(|error| error.to_string())?;
        if current.as_ref() != Some(expected_goal) {
            return Err("persisted goal changed before native clock restoration".into());
        }
        self.inner
            .accounting_state
            .alab_adopt_goal_clock(&expected_goal.goal_id, clock)
    }

    pub(crate) async fn alab_native_goal_state_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        if self.alab_startup_pending() {
            return Err("goal restoration has not activated".into());
        }
        let admission = self
            .inner
            .alab_upgrade
            .admit()
            .map_err(|error| error.to_string())?;
        if admission.interrupted_transfer() {
            codex_alab_core::session::shared().invalidate_upgrade();
        }
        let permit = self
            .inner
            .goal_state_lock
            .acquire()
            .await
            .map_err(|error| error.to_string())?;
        drop(admission);
        if !self.alab_registered_current() {
            return Err("the native goal runtime is no longer registered".into());
        }
        Ok(permit)
    }

    /// Retirement first excludes mutations, then performs its final accounting
    /// drain. It takes the accounting permit only after that drain so the drain's
    /// own additive owner can settle without self-deadlocking.
    pub(crate) async fn alab_goal_runtime_retirement_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        self.inner
            .goal_state_lock
            .acquire()
            .await
            .map_err(|error| error.to_string())
    }

    /// A committed process transfer owns final goal accounting after Core and
    /// its source readers quiesce. Runtime stop registers that deferred owner
    /// instead of retiring persistence ahead of the exact transfer.
    pub(crate) fn alab_defer_committed_goal_runtime_retirement(
        &self,
    ) -> Option<codex_alab_core::upgrade_barrier::RetirementRegistration> {
        self.inner.alab_upgrade.defer_retirement()
    }

    pub(crate) async fn alab_goal_runtime_retirement_accounting_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        self.inner
            .accounting_state
            .progress_accounting_permit()
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn alab_goal_runtime_retirement_callback_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        self.inner
            .accounting_state
            .callback_permit()
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) fn alab_goal_configuration_changed(&self) {
        if self.inner.alab_upgrade.interrupt() {
            codex_alab_core::session::shared().invalidate_upgrade();
        }
    }

    pub(crate) fn alab_idle_continuation_deferred(&self) -> bool {
        self.alab_startup_pending()
            || self.inner.alab_upgrade.committed()
            || self.alab_auth_recovery_deferred()
    }
}

#[path = "goal_startup.rs"]
mod startup;

#[path = "retained_goal_admission.rs"]
mod retained;
