// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Required process-local startup resource; claim strings are selection DATA.
//! Its optional checkout is the backend's actual allocation, never a path/ID.
//! No-worktree startup still has one explicit accepted startup operation.
pub enum InitialSessionKind {
    Start,
    Fork,
    Resume,
    Selection,
}
pub struct InitialSessionPlan {
    pub(crate) claim: String,
    pub(crate) kind: InitialSessionKind,
    pub(crate) binding: Option<codex_alab_task_worktree::StartupBinding>,
}
impl InitialSessionPlan {
    pub fn new(
        kind: InitialSessionKind,
        binding: Option<codex_alab_task_worktree::StartupBinding>,
    ) -> Self {
        Self {
            claim: Self::fresh_claim(),
            kind,
            binding,
        }
    }
    pub fn claim(&self) -> &str {
        &self.claim
    }
    pub(crate) fn fresh_claim() -> String {
        format!(
            "{:016x}{:016x}",
            codex_alab_core::native_source::connection_request_prefix(),
            codex_alab_core::native_source::connection_request_prefix()
        )
    }
}
