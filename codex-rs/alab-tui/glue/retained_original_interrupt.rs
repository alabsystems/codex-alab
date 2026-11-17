// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original deferred interrupt dispatch under its retained queue item.
//! The wrapper owns no new sender and never derives a callback from params.
use super::*;
impl QueuedInterrupt {
    pub(super) fn alab_apply_now(self, chat: &mut ChatWidget) {
        match self {
            Self::ExecApproval(event) => chat.handle_exec_approval_now(event),
            Self::ApplyPatchApproval(event) => chat.handle_apply_patch_approval_now(event),
            Self::RequestPermissions(event) => chat.handle_request_permissions_now(event),
            Self::RequestUserInput(event) => chat.handle_request_user_input_now(event),
            Self::Elicitation { request_id, params } => {
                chat.handle_elicitation_request_now(request_id, params)
            }
            Self::ItemStarted(item) => chat.handle_queued_item_started_now(item),
            Self::ItemCompleted(item) => chat.handle_queued_item_completed_now(item),
            Self::AlabUserInput(event, item) => {
                item.with_scope(|| chat.handle_request_user_input_now(event))
            }
            Self::AlabApproval(original, item) => item.with_scope(|| original.alab_apply_now(chat)),
        }
    }
}
