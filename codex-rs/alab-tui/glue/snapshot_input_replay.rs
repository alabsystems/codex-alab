// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ChatWidget;
use super::RateLimitErrorKind;
use super::ThreadInputState;
use crate::history_cell;

impl ChatWidget {
    pub(super) fn alab_render_historical_quota(
        &mut self,
        kind: &RateLimitErrorKind,
        message: &str,
    ) -> bool {
        if !codex_alab_core::active()
            || !self.thread_usage.replaying_turn_completion
            || !matches!(
                kind,
                RateLimitErrorKind::UsageLimit | RateLimitErrorKind::Generic
            )
        {
            return false;
        }
        // The saved turn may belong to another subscription. Close its displayed
        // activity without putting the current account's input queue into recovery.
        self.finalize_turn();
        self.add_to_history(history_cell::new_error_event(message.to_owned()));
        self.request_redraw();
        true
    }

    pub(crate) fn alab_snapshot_input_held(&self) -> bool {
        self.input_queue.alab_snapshot_input_held
    }

    pub(crate) fn alab_hold_snapshot_input(&mut self) {
        self.input_queue.alab_snapshot_input_held = true;
    }

    pub(crate) fn alab_prepare_resumed_input(
        &mut self,
        input: &mut ThreadInputState,
        read_only: bool,
        previously_held: bool,
    ) {
        // Writer conflicts and provider errors keep recovered follow-ups for review.
        input.recovered_queue |= read_only;
        if read_only || self.blocks_direct_input || previously_held {
            self.alab_hold_snapshot_input();
        }
    }

    pub(crate) fn alab_resume_snapshot_input(&mut self) {
        if self.blocks_direct_input
            || !std::mem::take(&mut self.input_queue.alab_snapshot_input_held)
        {
            return;
        }
        // Receipt reconciliation can remove the last recovered message before this wake.
        self.input_queue.recovered_queue &= self.input_queue.has_queued_follow_up_messages()
            || !self.input_queue.pending_steers.is_empty();
        self.submit_initial_user_message_if_pending();
        self.maybe_send_next_queued_input();
    }
}

#[cfg(test)]
#[path = "snapshot_quota_tests.rs"]
mod quota_tests;
