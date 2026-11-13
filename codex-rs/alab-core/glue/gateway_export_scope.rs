// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! The upstream cache/runner remains canonical; captured exports never rebind accounts.
use super::*;

impl GatewayExport {
    pub(crate) fn alab_check_policy(&self) -> Result<()> {
        let permit = self
            .policy
            .acquire_for_unsupported_sdk()
            .map_err(|error| CodexErr::Fatal(error.to_string()))?;
        permit
            .check()
            .map_err(|error| CodexErr::Fatal(error.to_string()))
    }

    pub(crate) async fn alab_owned_token(self: &Arc<Self>) -> Result<Arc<str>> {
        let export = Arc::clone(self);
        codex_api::alab_auth_source::credential_operation(self.policy.clone(), async move {
            export.token().await
        })
        .await
        .map_err(|error| CodexErr::Fatal(error.to_string()))?
    }

    pub(crate) async fn alab_invalidate_rejected(&self, rejected: &Arc<str>) -> bool {
        if self.alab_check_policy().is_err() {
            return false;
        }
        let mut cached = self.cached.lock().await;
        if self.alab_check_policy().is_err() {
            return false;
        }
        // A concurrent refresh can publish identical bytes as a distinct generation.
        if cached
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, rejected))
        {
            *cached = None;
        }
        true
    }
}
