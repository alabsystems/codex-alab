// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside the goal runtime: retry admission, never the failed child action.

use super::GoalRuntimeHandle;
use codex_protocol::error::CodexErr;
use std::sync::Arc;

#[path = "goal_start_recovery.rs"]
mod start;

impl GoalRuntimeHandle {
    /// A healthy owned write can outlive its observer and the last idle event.
    /// Pace that deferral through the existing single owner; failed persistence
    /// authorizes no new request or automatic additive replay.
    pub(super) fn alab_defer_for_goal_persistence(&self) -> bool {
        if !codex_alab_core::is_alab_binary() {
            return false;
        }
        let enhanced = codex_alab_core::active();
        match self.inner.accounting_state.alab_persistence.admission() {
            codex_alab_core::goal_persistence::Admission::Ready => false,
            codex_alab_core::goal_persistence::Admission::Writing => {
                if enhanced {
                    self.alab_recheck_goal_admission();
                }
                true
            }
            codex_alab_core::goal_persistence::Admission::Held => {
                if enhanced {
                    self.alab_cancel_goal_admission_recovery();
                }
                true
            }
        }
    }

    pub(crate) fn alab_hold_policy_continuation(&self, policy: Option<String>) {
        if let Some(policy) = policy {
            *self
                .inner
                .alab_policy_continuation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(policy);
        }
    }

    pub(crate) fn alab_policy_continuation(&self) -> Option<String> {
        self.inner
            .alab_policy_continuation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn alab_clear_policy_continuation(&self) {
        self.inner
            .alab_policy_continuation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    pub(crate) fn alab_recheck_failed_read(&self, transient: bool) {
        if transient {
            self.alab_recheck_goal_admission();
        } else {
            self.inner.alab_recovery.permanent_failure();
        }
    }

    /// Observe a failed automatic start. Permanent refusals authorize no new request.
    pub fn alab_record_goal_admission_failure(&self, error: &CodexErr) {
        if start::retryable_start(error) {
            self.alab_recheck_goal_admission();
        } else {
            self.inner.alab_recovery.permanent_failure();
        }
    }

    fn alab_recheck_goal_admission(&self) {
        if !codex_alab_core::active() || !self.tools_available() {
            return;
        }
        let runtime = Arc::downgrade(&self.inner);
        self.inner.alab_recovery.failed(Arc::new(move || {
            let runtime = runtime.clone();
            Box::pin(async move {
                let Some(inner) = runtime.upgrade() else {
                    return;
                };
                let runtime = GoalRuntimeHandle { inner };
                if let Err(error) = runtime.continue_if_idle().await {
                    tracing::warn!(%error, "automatic goal admission recheck failed");
                }
            })
        }));
    }

    pub(crate) fn alab_cancel_goal_admission_recovery(&self) {
        self.inner.alab_recovery.cancel();
    }

    /// Observable recovery state; this is not authority to continue a stopped goal.
    pub fn alab_goal_admission_recovery_pending(&self) -> bool {
        self.inner.alab_recovery.pending()
    }
}
