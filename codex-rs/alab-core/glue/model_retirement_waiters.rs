// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

impl super::TurnState {
    pub(crate) fn alab_has_pending_tool_response(&self) -> bool {
        !self
            .pending_approvals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
            || !self.pending_request_permissions.is_empty()
            || !self.pending_user_input.is_empty()
            || !self.pending_elicitations.is_empty()
            || !self.pending_dynamic_tools.is_empty()
    }
}
