// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Every parked original ThreadInputState field. Borrowed by the enclosing
//! scene; this module never encodes a separate editor graph or submits input.
use super::user_messages::*;
use super::*;
use serde::Deserialize;
use std::io;
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
crate::alab_editor_upgrade::native_state!(
    ThreadComposerState,
    ThreadComposerWire,
    #[serde(remote = "ThreadComposerState", deny_unknown_fields)]
    struct ThreadComposerWire {
        text: String,
        local_images: Vec<LocalImageAttachment>,
        remote_image_urls: Vec<String>,
        text_elements: Vec<TextElement>,
        mention_bindings: Vec<MentionBinding>,
        pending_pastes: Vec<(String, String)>,
    }
);
crate::alab_editor_upgrade::native_state!(
    StatusSurfaceOverrides,
    StatusSurfaceWire,
    #[serde(remote = "StatusSurfaceOverrides", deny_unknown_fields)]
    struct StatusSurfaceWire {
        #[serde(deserialize_with = "required")]
        terminal_title: Option<Vec<String>>,
        #[serde(deserialize_with = "required")]
        status_line: Option<Vec<String>>,
        #[serde(deserialize_with = "required")]
        status_line_use_colors: Option<bool>,
    }
);
crate::alab_editor_upgrade::native_state!(
    ThreadInputState,
    ThreadInputWire,
    #[serde(remote = "ThreadInputState", deny_unknown_fields)]
    struct ThreadInputWire {
        #[serde(deserialize_with = "required")]
        provider_error: Option<codex_app_server_protocol::ModelProviderError>,
        #[serde(deserialize_with = "required")]
        questions: Option<crate::bottom_pane::QuestionState>,
        #[serde(deserialize_with = "required")]
        pending_thread_settings:
            Option<codex_app_server_protocol::ThreadSettingsUpdatedNotification>,
        #[serde(deserialize_with = "required")]
        initial_user_message: Option<UserMessage>,
        #[serde(deserialize_with = "required")]
        startup_submission: Option<crate::bottom_pane::ComposerDraftSnapshot>,
        #[serde(deserialize_with = "required")]
        composer: Option<ThreadComposerState>,
        #[serde(deserialize_with = "required")]
        safety_buffering_prompt: Option<UserMessage>,
        safety_buffering_source: UserMessageSource,
        pending_steers: std::collections::VecDeque<PendingSteer>,
        rejected_steers_queue: std::collections::VecDeque<UserMessage>,
        rejected_steer_sources: std::collections::VecDeque<UserMessageSource>,
        rejected_steer_history_records: std::collections::VecDeque<UserMessageHistoryRecord>,
        queued_user_messages: std::collections::VecDeque<QueuedUserMessage>,
        queued_user_message_history_records: std::collections::VecDeque<UserMessageHistoryRecord>,
        recovered_queue: bool,
        reconnect_pending: bool,
        user_turn_pending_start: bool,
        #[serde(deserialize_with = "required")]
        pending_user_message_client_id: Option<String>,
        submit_pending_steers_after_interrupt: bool,
        current_collaboration_mode: CollaborationMode,
        #[serde(deserialize_with = "required")]
        active_collaboration_mask: Option<CollaborationModeMask>,
        #[serde(deserialize_with = "required")]
        plan_mode_reasoning_effort: Option<ReasoningEffortConfig>,
        status_surface_overrides: StatusSurfaceOverrides,
        task_running: bool,
        agent_turn_running: bool,
    }
);
impl ThreadInputState {
    pub(crate) fn alab_validate_retained_backend_input(&self) -> io::Result<()> {
        if self.rejected_steer_sources.len() > self.rejected_steers_queue.len()
            || self.rejected_steer_history_records.len() > self.rejected_steers_queue.len()
            || self.queued_user_message_history_records.len() > self.queued_user_messages.len()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "parked input original queue lineage differs",
            ));
        }
        if let Some(questions) = &self.questions {
            questions.alab_validate_retained_backend_questions()?;
        }
        Ok(())
    }
}
