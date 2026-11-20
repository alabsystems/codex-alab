// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl ChatWidget {
    pub(crate) fn alab_recover_undispatched_steer(&mut self, client_id: &str) {
        if let Some((message, history, source)) = self.take_submitted_message(client_id) {
            self.input_queue.rejected_steers_queue.push_back(message);
            self.input_queue
                .rejected_steer_history_records
                .push_back(history);
            self.input_queue.rejected_steer_sources.push_back(source);
            self.input_queue.recovered_queue = true;
            self.refresh_pending_input_preview();
            self.update_task_running_state();
        }
    }

    /// Keep input that was waiting for the refused turn available to the queue editor.
    pub(super) fn alab_recover_finding_input(&mut self, fresh_launch: bool) {
        self.input_queue.submit_pending_steers_after_interrupt = false;
        self.requeue_image_submission();
        #[cfg(unix)]
        let retained_startup = self.alab_holds_retained_startup();
        #[cfg(not(unix))]
        let retained_startup = false;
        if !retained_startup {
            self.cancel_startup_submission();
            if !fresh_launch && let Some(message) = self.initial_user_message.take() {
                self.input_queue
                    .queued_user_messages
                    .push_front(QueuedUserMessage::new(message, QueuedInputAction::Literal));
                self.input_queue
                    .queued_user_message_history_records
                    .push_front(UserMessageHistoryRecord::UserMessageText);
            }
        }
        self.input_queue.recovered_queue |= self.input_queue.has_queued_follow_up_messages()
            || !self.input_queue.pending_steers.is_empty();
        self.refresh_pending_input_preview();
    }

    pub(super) fn alab_recover_restored_finding_input(&mut self) {
        if self.misalignment_policy_violation.is_some() && !self.has_misalignment_policy_violation()
        {
            self.alab_recover_finding_input(/*fresh_launch*/ false);
        }
    }

    pub(super) fn alab_reconcile_recovered_queue(&mut self) {
        self.input_queue.recovered_queue &= self.input_queue.has_queued_follow_up_messages()
            || !self.input_queue.pending_steers.is_empty();
    }

    /// A terminal notification may reach the widget again while the same finding is visible.
    pub(super) fn alab_retain_recovered_input_on_interrupt(&mut self) -> bool {
        if !self.input_queue.recovered_queue || self.misalignment_policy_violation.is_none() {
            return false;
        }
        let queue = &mut self.input_queue;
        for pending in queue.pending_steers.drain(..) {
            queue.rejected_steers_queue.push_back(pending.user_message);
            queue.rejected_steer_sources.push_back(pending.source);
            queue
                .rejected_steer_history_records
                .push_back(pending.history_record);
        }
        self.refresh_pending_input_preview();
        self.request_redraw();
        true
    }
}

impl ThreadInputState {
    /// Navigation can park the original composer between Enter and the operation's dispatch.
    pub(crate) fn alab_recover_undispatched_steer(&mut self, client_id: &str) {
        let (message, history, source) = if let Some(index) =
            self.pending_steers.iter().position(|pending| {
                pending.client_id == client_id && pending.admitted_turn_id.is_none()
            }) {
            let pending = self
                .pending_steers
                .remove(index)
                .expect("matched pending steer");
            (pending.user_message, pending.history_record, pending.source)
        } else if self.user_turn_pending_start
            && self.pending_user_message_client_id.as_deref() == Some(client_id)
            && let Some(message) = self.safety_buffering_prompt.take()
        {
            self.user_turn_pending_start = false;
            self.pending_user_message_client_id = None;
            (
                message,
                UserMessageHistoryRecord::UserMessageText,
                self.safety_buffering_source,
            )
        } else {
            return;
        };
        self.rejected_steers_queue.push_back(message);
        self.rejected_steer_history_records.push_back(history);
        self.rejected_steer_sources.push_back(source);
        self.recovered_queue = true;
    }
}
