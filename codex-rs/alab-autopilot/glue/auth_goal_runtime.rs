// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Runs under the native goal mutation permit; only the exact authentication or
//! quota failure witness can restore a goal.

impl super::GoalRuntimeHandle {
    /// Only the actual subscription request may retain quota recovery custody.
    /// API-key limits and unowned failures keep the ordinary usage-limit path.
    pub(crate) fn alab_quota_stop_reason(
        turn: &codex_extension_api::ExtensionData,
    ) -> super::ActiveGoalStopReason {
        let eligible = turn
            .get::<codex_alab_core::renewal::AuthRequestScope>()
            .is_some_and(|scope| {
                scope.eligible(turn.level_id())
                    && matches!(
                        scope.snapshot().auth,
                        Some(codex_alab_core::renewal::RequestAuth::ChatGpt { .. })
                    )
            });
        if !eligible {
            return super::ActiveGoalStopReason::UsageLimit;
        }
        let Some(origin) = turn.get::<codex_alab_core::renewal::GoalAtTurnStart>() else {
            return super::ActiveGoalStopReason::UsageLimit;
        };
        super::ActiveGoalStopReason::AccountQuotaError {
            standalone: origin.goal_id.is_none(),
            original_goal: origin.goal_id.clone(),
        }
    }

    pub(crate) fn alab_auth_stop_reason(
        turn: &codex_extension_api::ExtensionData,
    ) -> super::ActiveGoalStopReason {
        if !turn
            .get::<codex_alab_core::renewal::AuthRequestScope>()
            .is_some_and(|scope| {
                scope.eligible(turn.level_id())
                    || turn
                        .get::<codex_alab_core::renewal::AuthObservation>()
                        .is_some_and(|proof| proof.proves_cause(&scope, turn.level_id()))
            })
        {
            return super::ActiveGoalStopReason::BoundTurnError {
                original_goal: turn
                    .get::<codex_alab_core::renewal::GoalAtTurnStart>()
                    .and_then(|origin| origin.goal_id.clone()),
            };
        }
        let origin = turn.get::<codex_alab_core::renewal::GoalAtTurnStart>();
        super::ActiveGoalStopReason::AuthError {
            standalone: origin
                .as_ref()
                .is_some_and(|origin| origin.goal_id.is_none()),
            original_goal: origin.and_then(|origin| origin.goal_id.clone()),
        }
    }

    pub(super) fn alab_auth_goal_replaced(
        reason: &super::ActiveGoalStopReason,
        current: &str,
    ) -> bool {
        match reason {
            super::ActiveGoalStopReason::AuthError { original_goal, .. }
            | super::ActiveGoalStopReason::AccountQuotaError { original_goal, .. }
            | super::ActiveGoalStopReason::BoundTurnError { original_goal } => {
                original_goal.as_deref() != Some(current)
            }
            _ => false,
        }
    }

    /// Match the native typed login walls recognized by Autopilot. A provider's
    /// gateway-login text or an untyped Other error does not prove this wall.
    pub fn alab_is_auth_wall(error: &codex_protocol::protocol::CodexErrorInfo) -> bool {
        use codex_protocol::protocol::CodexErrorInfo;
        matches!(
            error,
            CodexErrorInfo::Unauthorized
                | CodexErrorInfo::HttpConnectionFailed {
                    http_status_code: Some(401)
                }
                | CodexErrorInfo::ResponseStreamConnectionFailed {
                    http_status_code: Some(401)
                }
                | CodexErrorInfo::ResponseTooManyFailedAttempts {
                    http_status_code: Some(401)
                }
                | CodexErrorInfo::ResponseStreamDisconnected {
                    http_status_code: Some(401)
                }
        )
    }

    pub async fn alab_capture_auth_standalone(&self, wall: &str) -> Result<bool, String> {
        let _permit = self.goal_state_permit().await?;
        self.inner
            .state_dbs
            .thread_goals()
            .alab_capture_auth_standalone(self.thread_id(), wall)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn alab_defer_auth_recovery(&self, wall: &str) -> Result<(), String> {
        let _permit = self.goal_state_permit().await?;
        self.inner
            .state_dbs
            .thread_goals()
            .alab_defer_auth_recovery(self.thread_id(), wall)
            .await
            .map_err(|error| error.to_string())?;
        let deferred = self
            .inner
            .state_dbs
            .thread_goals()
            .has_thread_goal_continuation_deferral(self.thread_id())
            .await
            .map_err(|error| error.to_string())?;
        self.inner
            .alab_auth_deferred
            .store(deferred, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    pub fn alab_auth_recovery_deferred(&self) -> bool {
        self.inner
            .alab_auth_deferred
            .load(std::sync::atomic::Ordering::SeqCst)
            || self
                .inner
                .alab_auth_transient
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .1
    }

    pub fn alab_begin_transient_auth_recovery(&self) -> Result<u64, String> {
        let mut state = self
            .inner
            .alab_auth_transient
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.0 = state
            .0
            .checked_add(1)
            .ok_or("transient recovery generation exhausted")?;
        state.1 = true;
        Ok(state.0)
    }

    pub fn alab_finish_transient_auth_recovery(&self, token: u64) -> bool {
        let mut state = self
            .inner
            .alab_auth_transient
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.0 != token || !state.1 {
            return false;
        }
        state.1 = false;
        true
    }

    pub fn alab_auth_turn_started(&self) {
        self.inner
            .alab_auth_transient
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .1 = false;
        self.inner
            .alab_auth_deferred
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub async fn alab_cancel_auth_recovery(&self) -> Result<(), String> {
        self.inner
            .alab_auth_transient
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .1 = false;
        if self
            .inner
            .alab_auth_deferred
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            self.inner
                .state_dbs
                .thread_goals()
                .clear_thread_goal_continuation_deferral(self.thread_id())
                .await
                .map_err(|error| error.to_string())?;
            self.inner
                .alab_auth_deferred
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }

    pub async fn alab_retire_transient_auth_recovery(&self, token: u64) {
        if self.alab_finish_transient_auth_recovery(token) {
            let _ = self.continue_if_idle().await;
        }
    }

    pub async fn alab_retire_auth_recovery(
        &self,
        wall: &str,
        expected_goal: Option<&str>,
    ) -> Result<bool, String> {
        let permit = self.goal_state_permit().await?;
        let store = self.inner.state_dbs.thread_goals();
        let retired = store
            .alab_retire_auth_recovery(self.thread_id(), wall, expected_goal)
            .await
            .map_err(|error| error.to_string())?;
        let deferred = store
            .has_thread_goal_continuation_deferral(self.thread_id())
            .await
            .map_err(|error| error.to_string())?;
        self.inner
            .alab_auth_deferred
            .store(deferred, std::sync::atomic::Ordering::SeqCst);
        drop(permit);
        if retired && !deferred {
            self.continue_if_idle().await?;
        }
        Ok(retired)
    }
    pub fn alab_auth_recovery_enabled(&self) -> bool {
        self.is_enabled()
    }
    pub async fn alab_auth_goal_context(
        &self,
        wall: &str,
    ) -> Result<Option<Option<String>>, String> {
        self.inner
            .state_dbs
            .thread_goals()
            .alab_auth_goal_context(self.thread_id(), wall)
            .await
            .map_err(|error| error.to_string())
    }
    pub async fn alab_auth_goal_pending(&self, wall: &str) -> Result<bool, String> {
        self.inner
            .state_dbs
            .thread_goals()
            .alab_auth_goal_pending(self.thread_id(), wall)
            .await
            .map_err(|error| error.to_string())
    }

    /// Keep this permit only through pre-task admission. Native start clears the
    /// persisted continuation deferral, then owns ordinary accounting again.
    pub async fn alab_auth_recovery_permit(
        &self,
        wall: &str,
        expected_goal: Option<&str>,
        reserved_turn: &str,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        self.alab_auth_recovery_permit_with_deferral(wall, expected_goal, reserved_turn, true)
            .await
    }

    pub async fn alab_auth_recovery_permit_transient(
        &self,
        wall: &str,
        expected_goal: Option<&str>,
        reserved_turn: &str,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        self.alab_auth_recovery_permit_with_deferral(wall, expected_goal, reserved_turn, false)
            .await
    }

    async fn alab_auth_recovery_permit_with_deferral(
        &self,
        wall: &str,
        expected_goal: Option<&str>,
        reserved_turn: &str,
        durable_deferral: bool,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        let permit = self.goal_state_permit().await?;
        let store = self.inner.state_dbs.thread_goals();
        let current = store
            .get_thread_goal(self.thread_id())
            .await
            .map_err(|e| e.to_string())?;
        let context = store
            .alab_auth_goal_context(self.thread_id(), wall)
            .await
            .map_err(|e| e.to_string())?;
        if context.as_ref().map(|goal| goal.as_deref()) == Some(expected_goal)
            && expected_goal.is_none()
        {
            return Ok(permit);
        }
        let Some(current) = current else {
            return Err("the original account-recovery goal context is unavailable".into());
        };
        if expected_goal != Some(current.goal_id.as_str()) {
            return Err("the original goal was replaced".into());
        }
        if current
            .token_budget
            .is_some_and(|budget| current.tokens_used >= budget)
        {
            return Err("the original goal budget is exhausted".into());
        }
        if context.as_ref().map(|goal| goal.as_deref()) != Some(expected_goal) {
            return Err("the original account-recovery goal proof was invalidated".into());
        }
        let Some(restored) = store
            .alab_restore_auth_goal_with_deferral(self.thread_id(), wall, durable_deferral)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Err("the goal is not the unchanged account-recovery goal".into());
        };
        if !store
            .alab_accept_auth_goal(self.thread_id(), wall, reserved_turn)
            .await
            .map_err(|e| e.to_string())?
        {
            return Err("account recovery belongs to a different native admission".into());
        }
        self.inner
            .accounting_state
            .mark_idle_goal_active(restored.goal_id.clone());
        if current.status != restored.status {
            self.inner
                .metrics
                .record_resumed_if_status_changed(Some(current.status), restored.status);
            self.inner.analytics.status_changed(
                &restored,
                Some(current.status),
                super::GoalEventAttribution::Turn(wall),
            );
            self.inner.event_emitter.thread_goal_updated(
                format!("{wall}:auth-restored"),
                Some(wall.to_owned()),
                super::protocol_goal_from_state(restored),
            );
        }
        Ok(permit)
    }
}

#[cfg(test)]
mod tests {
    use super::super::GoalRuntimeHandle;
    use codex_protocol::protocol::CodexErrorInfo;

    #[test]
    fn quota_recovery_requires_the_current_subscription_request_and_goal_origin() {
        use codex_alab_core::renewal::AuthRequestScope;
        use codex_alab_core::renewal::GoalAtTurnStart;
        use codex_alab_core::renewal::RequestAuth;
        for case in 0..4 {
            let turn = codex_extension_api::ExtensionData::new("quota-turn");
            if case != 3 {
                turn.insert(GoalAtTurnStart {
                    goal_id: Some("original".into()),
                });
            }
            turn.insert(AuthRequestScope::new(
                "quota-turn".into(),
                Some(if case == 1 {
                    RequestAuth::OpenAiApiKey
                } else {
                    RequestAuth::ChatGpt {
                        account: "account".into(),
                        user: "user".into(),
                    }
                }),
                1,
                1,
                move || case != 2,
            ));
            let reason = GoalRuntimeHandle::alab_quota_stop_reason(&turn);
            assert_eq!(
                matches!(
                    reason,
                    super::super::ActiveGoalStopReason::AccountQuotaError { .. }
                ),
                case == 0
            );
        }
    }

    #[test]
    fn passive_auth_cause_records_only_original_custody_without_scope_authority() {
        use codex_alab_core::renewal::AuthObservation;
        use codex_alab_core::renewal::AuthRequestScope;
        use codex_alab_core::renewal::GoalAtTurnStart;
        use codex_alab_core::renewal::RequestAuth;
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::Ordering;
        let turn = codex_extension_api::ExtensionData::new("observed-wall");
        let scope = Arc::new(AuthRequestScope::new(
            "observed-wall".into(),
            Some(RequestAuth::OpenAiApiKey),
            1,
            2,
            || false,
        ));
        turn.insert(GoalAtTurnStart {
            goal_id: Some("original".into()),
        });
        turn.insert_arc(Arc::clone(&scope));
        let known_cause = Arc::new(AtomicBool::new(true));
        let current = Arc::clone(&known_cause);
        turn.insert(AuthObservation::new(&scope, &scope.snapshot(), move || {
            current.load(Ordering::SeqCst)
        }));
        assert!(
            !scope.eligible("observed-wall"),
            "observation grants no execution scope"
        );
        assert!(
            matches!(GoalRuntimeHandle::alab_auth_stop_reason(&turn), super::super::ActiveGoalStopReason::AuthError { original_goal: Some(goal), .. } if goal == "original")
        );
        known_cause.store(false, Ordering::SeqCst);
        assert!(
            matches!(GoalRuntimeHandle::alab_auth_stop_reason(&turn), super::super::ActiveGoalStopReason::BoundTurnError { original_goal: Some(goal) } if goal == "original")
        );
    }

    #[test]
    fn typed_auth_stop_ownership_cannot_cross_goals_or_a_standalone_origin() {
        use super::super::ActiveGoalStopReason;
        for reason in [
            ActiveGoalStopReason::AuthError {
                standalone: false,
                original_goal: Some("A".into()),
            },
            ActiveGoalStopReason::BoundTurnError {
                original_goal: Some("A".into()),
            },
            ActiveGoalStopReason::AccountQuotaError {
                standalone: false,
                original_goal: Some("A".into()),
            },
        ] {
            assert!(!GoalRuntimeHandle::alab_auth_goal_replaced(&reason, "A"));
            assert!(GoalRuntimeHandle::alab_auth_goal_replaced(&reason, "B"));
        }
        for reason in [
            ActiveGoalStopReason::AuthError {
                standalone: true,
                original_goal: None,
            },
            ActiveGoalStopReason::BoundTurnError {
                original_goal: None,
            },
            ActiveGoalStopReason::AccountQuotaError {
                standalone: true,
                original_goal: None,
            },
        ] {
            assert!(GoalRuntimeHandle::alab_auth_goal_replaced(&reason, "A"));
            assert!(GoalRuntimeHandle::alab_auth_goal_replaced(&reason, "B"));
        }
        assert!(!GoalRuntimeHandle::alab_auth_goal_replaced(
            &ActiveGoalStopReason::TurnError,
            "A"
        ));
    }

    #[test]
    fn only_native_typed_login_refusals_prove_auth_stop() {
        assert!(GoalRuntimeHandle::alab_is_auth_wall(
            &CodexErrorInfo::Unauthorized
        ));
        assert!(GoalRuntimeHandle::alab_is_auth_wall(
            &CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(401)
            }
        ));
        assert!(!GoalRuntimeHandle::alab_is_auth_wall(
            &CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(403)
            }
        ));
        assert!(!GoalRuntimeHandle::alab_is_auth_wall(
            &CodexErrorInfo::Other
        ));
    }
}
