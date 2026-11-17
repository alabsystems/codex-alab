// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retained input DATA at the actual ChatWidget boundary. No request is sent by
//! capture, decode, or install. In particular Unconfirmed stays Unconfirmed.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[path = "retained_message_codec.rs"]
mod codec;

const VERSION: u32 = 1;

#[derive(Serialize)]
struct SnapshotRef<'a> {
    version: u32,
    initial_user_message: &'a Option<UserMessage>,
    suppress_initial_user_message_submit: bool,
    input_queue: &'a InputQueueState,
    safety_buffering_prompt: &'a Option<UserMessage>,
    safety_buffering_source: UserMessageSource,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    #[serde(deserialize_with = "required_initial_message")]
    initial_user_message: Option<UserMessage>,
    suppress_initial_user_message_submit: bool,
    input_queue: InputQueueState,
    safety_buffering_prompt: Option<UserMessage>,
    safety_buffering_source: UserMessageSource,
}

fn required_initial_message<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<UserMessage>, D::Error> {
    Option::<UserMessage>::deserialize(decoder)
}

/// Prepared DATA, deliberately non-clone. This owns no Core or sender and does
/// not establish that a retained projection, UI frame, or Native ACK occurred.
pub(crate) struct PreparedMessages(Snapshot);

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn queue_empty(queue: &InputQueueState) -> bool {
    queue.startup_submission.is_none()
        && queue.queued_user_messages.is_empty()
        && queue.queued_user_message_history_records.is_empty()
        && !queue.user_turn_pending_start
        && queue.pending_user_message_client_id.is_none()
        && queue.pending_user_message_history_record.is_none()
        && queue.rejected_steers_queue.is_empty()
        && queue.rejected_steer_sources.is_empty()
        && queue.rejected_steer_history_records.is_empty()
        && queue.pending_steers.is_empty()
        && !queue.submit_pending_steers_after_interrupt
        && !queue.rate_limit_recovery_pending
        && !queue.recovered_queue
        && queue.transcript_copy.strong_count() == 0
}

impl ChatWidget {
    /// Borrow the actual state without capture_thread_input_state(): that
    /// navigation helper cancels image work and reduces the rich composer.
    pub(crate) fn alab_capture_retained_messages(
        &self,
    ) -> io::Result<Box<serde_json::value::RawValue>> {
        self.alab_capture_retained_messages_with_initial(&self.initial_user_message)
    }
    /// SAME complete original graph while configured startup owns the exact
    /// initial message. This only borrows that original value; no submit occurs.
    pub(crate) fn alab_capture_retained_messages_with_initial(
        &self,
        actual_initial: &Option<UserMessage>,
    ) -> io::Result<Box<serde_json::value::RawValue>> {
        if self.pending_image_submission.is_some()
            || self.input_queue.transcript_copy.strong_count() != 0
        {
            return Err(invalid(
                "retained input still has a live image/copy continuation",
            ));
        }
        serde_json::value::to_raw_value(&SnapshotRef {
            version: VERSION,
            initial_user_message: actual_initial,
            suppress_initial_user_message_submit: self.suppress_initial_user_message_submit,
            input_queue: &self.input_queue,
            safety_buffering_prompt: &self.safety_buffering_prompt,
            safety_buffering_source: self.safety_buffering_source,
        })
        .map_err(invalid)
    }
}

impl PreparedMessages {
    pub(crate) fn decode(bytes: &serde_json::value::RawValue) -> io::Result<Self> {
        let snapshot: Snapshot = serde_json::from_str(bytes.get()).map_err(invalid)?;
        if snapshot.version != VERSION
            || snapshot
                .input_queue
                .queued_user_message_history_records
                .len()
                > snapshot.input_queue.queued_user_messages.len()
            || snapshot.input_queue.rejected_steer_sources.len()
                > snapshot.input_queue.rejected_steers_queue.len()
            || snapshot.input_queue.rejected_steer_history_records.len()
                > snapshot.input_queue.rejected_steers_queue.len()
        {
            return Err(invalid(
                "retained message graph differs from the original queue",
            ));
        }
        let canonical = SnapshotRef {
            version: snapshot.version,
            initial_user_message: &snapshot.initial_user_message,
            suppress_initial_user_message_submit: snapshot.suppress_initial_user_message_submit,
            input_queue: &snapshot.input_queue,
            safety_buffering_prompt: &snapshot.safety_buffering_prompt,
            safety_buffering_source: snapshot.safety_buffering_source,
        };
        if serde_json::to_value(canonical).map_err(invalid)?
            != serde_json::from_str::<serde_json::Value>(bytes.get()).map_err(invalid)?
        {
            return Err(invalid(
                "retained message graph omits or projects original native state",
            ));
        }
        Ok(Self(snapshot))
    }

    /// Feed THIS value into original ChatWidgetInit before constructing a C2
    /// widget. In particular None means the preserved original argv's CLI
    /// prompt was already consumed; do not call create_initial_user_message on
    /// those same arguments again. This is DATA, not submission authorization.
    pub(crate) fn initial_message_for_constructor(&self) -> Option<UserMessage> {
        self.0.initial_user_message.clone()
    }

    /// Preflight is separate so the caller can validate every UI section before
    /// installing any of them. A separately constructed CLI message may only
    /// coincide with the same original saved message; a different one is kept.
    pub(crate) fn validate_destination(&self, widget: &ChatWidget) -> io::Result<()> {
        if !queue_empty(&widget.input_queue)
            || widget.pending_image_submission.is_some()
            || widget.safety_buffering_prompt.is_some()
            || widget
                .initial_user_message
                .as_ref()
                .is_some_and(|original| self.0.initial_user_message.as_ref() != Some(original))
        {
            return Err(invalid(
                "retained message destination already owns different input",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_original_policy_destination(
        &self,
        widget: &ChatWidget,
    ) -> io::Result<()> {
        // Permit only the original post-filter initial value to differ. All
        // other actual destination ownership remains the original preflight.
        if !queue_empty(&widget.input_queue)
            || widget.pending_image_submission.is_some()
            || widget.safety_buffering_prompt.is_some()
        {
            return Err(invalid(
                "retained message destination already owns different input",
            ));
        }
        Ok(())
    }
    /// Actual constructor policy already selected/filtered this initial value.
    /// Preserve that exact result; only a real original cancellation event may
    /// clear the captured confirmation. Unconfirmed/queued values stay intact.
    pub(crate) fn install_after_original_policy(
        mut self,
        widget: &mut ChatWidget,
        cancel_confirmation: bool,
    ) -> io::Result<()> {
        self.validate_original_policy_destination(widget)?;
        self.0.initial_user_message = widget.initial_user_message.clone();
        if cancel_confirmation {
            self.0.input_queue.startup_submission = None;
        }
        self.install(widget)
    }
    /// No submission/queue drain or callback is invoked. The caller must keep
    /// startup submission and fresh automatic work held through real UI commit.
    pub(crate) fn install(self, widget: &mut ChatWidget) -> io::Result<()> {
        self.validate_destination(widget)?;
        let Snapshot {
            initial_user_message,
            suppress_initial_user_message_submit,
            input_queue,
            safety_buffering_prompt,
            safety_buffering_source,
            ..
        } = self.0;
        widget.initial_user_message = initial_user_message;
        widget.suppress_initial_user_message_submit = suppress_initial_user_message_submit;
        widget.input_queue = input_queue;
        widget.safety_buffering_prompt = safety_buffering_prompt;
        widget.safety_buffering_source = safety_buffering_source;
        Ok(())
    }
}

#[cfg(test)]
#[path = "retained_widget_transfer_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "retained_startup_constructor_tests.rs"]
mod constructor_tests;

#[cfg(test)]
async fn test_widget() -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<crate::app_command::AppCommand>,
) {
    let (widget, _sender, events, operations) =
        crate::boxed_future!(crate::chatwidget::tests::make_chatwidget_manual_with_sender()).await;
    (widget, events, operations)
}
