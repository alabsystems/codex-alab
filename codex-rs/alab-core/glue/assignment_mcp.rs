// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A fresh destination router never inherits an old account's pending responses.

use super::ElicitationRequestRouter;
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct RouterOwner(Mutex<ElicitationRequestRouter>);
impl RouterOwner {
    pub(super) fn snapshot(&self) -> ElicitationRequestRouter {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    fn replace_closed(&self, old: &ElicitationRequestRouter) {
        let next = ElicitationRequestRouter::default();
        next.set_auto_deny(old.auto_deny());
        // Full-access form input is a permission; a destination must establish it afresh.
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnterpriseState {
    Absent,
    Granted,
    Unavailable,
}

impl super::McpRuntime {
    pub fn alab_enterprise_state(&self) -> EnterpriseState {
        match self.ema_auth.get() {
            None => EnterpriseState::Absent,
            Some(Ok(_)) => EnterpriseState::Granted,
            Some(Err(_)) => EnterpriseState::Unavailable,
        }
    }

    /// Used only after all foreground work drained, before native source commit.
    pub async fn alab_retire_account(
        &self,
        domain: &std::sync::Arc<codex_alab_core::assignment::Domain>,
        retirement: &codex_alab_core::assignment::RetirementPermit,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            retirement.protects(domain),
            "account retirement is unavailable"
        );
        anyhow::ensure!(
            matches!(self.alab_enterprise_state(), EnterpriseState::Absent),
            "enterprise account policy requires separate qualification"
        );
        let router = self.elicitation_router.snapshot();
        // Runtime shutdown retains its own cleanup tasks across caller cancellation.
        self.shutdown().await;
        anyhow::ensure!(retirement.protects(domain), "account retirement changed");
        self.elicitation_router.replace_closed(&router);
        self.reconnect_on_next_refresh();
        self.invalidate_resource_caches();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn assignment_retirement_replaces_closed_router_without_inheriting_form_permission() {
        let runtime = super::super::McpRuntime::empty(false);
        let old = runtime.elicitation_router.snapshot();
        old.set_auto_deny(true);
        old.enable_full_access_form_input();
        let domain = std::sync::Arc::new(codex_alab_core::assignment::Domain::default());
        let intent = codex_alab_core::assignment::Intent {
            operation: "a".repeat(32),
            thread: "00000000-0000-0000-0000-000000000000".into(),
            selection: codex_alab_core::assignment::Selection::Auto {
                revision: "b".repeat(64),
            },
        };
        domain.request(intent.clone()).unwrap();
        let retirement = domain.try_retire(&intent.operation).unwrap();
        let other = std::sync::Arc::new(codex_alab_core::assignment::Domain::default());
        assert!(
            runtime
                .alab_retire_account(&other, &retirement)
                .await
                .is_err()
        );
        assert!(
            runtime
                .elicitation_router
                .snapshot()
                .full_access_form_input_enabled()
        );
        runtime
            .alab_retire_account(&domain, &retirement)
            .await
            .unwrap();
        let destination = runtime.elicitation_router.snapshot();
        assert!(destination.auto_deny());
        assert!(!destination.full_access_form_input_enabled());
        assert!(
            old.full_access_form_input_enabled(),
            "old owners keep their exact closed snapshot"
        );
    }
}
