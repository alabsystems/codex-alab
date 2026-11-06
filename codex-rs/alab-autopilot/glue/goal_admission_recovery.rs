// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside the goal runtime: retry admission, never the failed child action.

use super::GoalRuntimeHandle;
use codex_protocol::error::CodexErr;
use std::sync::Arc;

#[path = "goal_start_recovery.rs"]
mod start;

impl GoalRuntimeHandle {
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
