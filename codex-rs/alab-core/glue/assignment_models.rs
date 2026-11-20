// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Catalog I/O belongs to the same real manager boundary as model and Apps traffic.

use super::RefreshedModelCatalog;
use codex_protocol::openai_models::ModelInfo;
use std::sync::Arc;

impl RefreshedModelCatalog {
    /// Require one credential-scoped catalog and the exact authenticated publication.
    pub fn alab_confirms_model_recovery(
        &self,
        original: &Self,
        catalog: &Arc<[ModelInfo]>,
    ) -> bool {
        self.identity == original.identity && Arc::ptr_eq(&self.models, catalog)
    }
}

pub(super) fn admit(
    manager: Option<&codex_login::AuthManager>,
) -> codex_protocol::error::Result<Option<codex_alab_core::assignment::Activity>> {
    manager
        .map(|manager| {
            manager
                .alab_assignment_domain()
                .admit_current_resource()
                .map_err(|_| {
                    codex_protocol::error::CodexErr::InvalidRequest(
                        "Account assignment is pending; catalog preparation has been deferred."
                            .into(),
                    )
                })
        })
        .transpose()
}
