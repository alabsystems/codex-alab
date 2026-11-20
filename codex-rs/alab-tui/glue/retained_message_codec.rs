// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact original input types. The only skipped field is a process-local Weak
//! copy owner; capture refuses a live one before serialization.
use super::super::user_messages::*;
use super::*;
use crate::bottom_pane::QueuedInputAction;
use std::collections::VecDeque;

crate::alab_editor_upgrade::native_state!(
    UserMessage,
    UserMessageWire,
    #[serde(remote = "UserMessage", deny_unknown_fields)]
    struct UserMessageWire {
        text: String,
        local_images: Vec<crate::bottom_pane::LocalImageAttachment>,
        remote_image_urls: Vec<String>,
        text_elements: Vec<codex_protocol::user_input::TextElement>,
        mention_bindings: Vec<crate::bottom_pane::MentionBinding>,
    }
);
crate::alab_editor_upgrade::native_state!(
    UserMessageHistoryRecord,
    HistoryRecordWire,
    #[serde(remote = "UserMessageHistoryRecord", deny_unknown_fields)]
    enum HistoryRecordWire {
        UserMessageText,
        Override(UserMessageHistoryOverride),
    }
);
crate::alab_editor_upgrade::native_state!(
    UserMessageHistoryOverride,
    HistoryOverrideWire,
    #[serde(remote = "UserMessageHistoryOverride", deny_unknown_fields)]
    struct HistoryOverrideWire {
        text: String,
        text_elements: Vec<codex_protocol::user_input::TextElement>,
    }
);
crate::alab_editor_upgrade::native_state!(
    UserMessageSource,
    MessageSourceWire,
    #[serde(remote = "UserMessageSource", deny_unknown_fields)]
    enum MessageSourceWire {
        Prompt,
        DirectEnter,
        QuestionAnswer,
        SafetyRecourse,
    }
);
crate::alab_editor_upgrade::native_state!(
    MessageDelivery,
    DeliveryWire,
    #[serde(remote = "MessageDelivery", deny_unknown_fields)]
    enum DeliveryWire {
        Unsent,
        Unconfirmed(Option<String>),
    }
);
crate::alab_editor_upgrade::native_state!(
    QueuedInputAction,
    ActionWire,
    #[serde(remote = "QueuedInputAction", deny_unknown_fields)]
    enum ActionWire {
        Plain,
        Literal,
        ParseSlash,
        RunShell,
    }
);
crate::alab_editor_upgrade::native_state!(
    QueuedUserMessage,
    QueuedMessageWire,
    #[serde(remote = "QueuedUserMessage", deny_unknown_fields)]
    struct QueuedMessageWire {
        user_message: UserMessage,
        prepared_images: Option<Vec<codex_app_server_protocol::UserInput>>,
        action: QueuedInputAction,
        delivery: MessageDelivery,
        pending_pastes: Vec<(String, String)>,
        source: UserMessageSource,
    }
);
crate::alab_editor_upgrade::native_state!(
    PendingSteerCompareKey,
    CompareKeyWire,
    #[serde(remote = "PendingSteerCompareKey", deny_unknown_fields)]
    struct CompareKeyWire {
        message: String,
        image_count: usize,
    }
);
crate::alab_editor_upgrade::native_state!(
    PendingSteer,
    PendingSteerWire,
    #[serde(remote = "PendingSteer", deny_unknown_fields)]
    struct PendingSteerWire {
        client_id: String,
        admitted_turn_id: Option<String>,
        optimistically_rendered: bool,
        user_message: UserMessage,
        history_record: UserMessageHistoryRecord,
        source: UserMessageSource,
        compare_key: PendingSteerCompareKey,
    }
);
crate::alab_editor_upgrade::native_state!(
    InputQueueState,
    InputQueueWire,
    #[serde(remote = "InputQueueState", deny_unknown_fields)]
    struct InputQueueWire {
        #[serde(skip)]
        alab_snapshot_input_held: bool,
        startup_submission: Option<crate::bottom_pane::ComposerDraftSnapshot>,
        queued_user_messages: VecDeque<QueuedUserMessage>,
        queued_user_message_history_records: VecDeque<UserMessageHistoryRecord>,
        user_turn_pending_start: bool,
        pending_user_message_client_id: Option<String>,
        pending_user_message_history_record: Option<UserMessageHistoryRecord>,
        rejected_steers_queue: VecDeque<UserMessage>,
        rejected_steer_sources: VecDeque<UserMessageSource>,
        rejected_steer_history_records: VecDeque<UserMessageHistoryRecord>,
        pending_steers: VecDeque<PendingSteer>,
        submit_pending_steers_after_interrupt: bool,
        suppress_queue_autosend: bool,
        #[serde(skip)]
        transcript_copy: std::sync::Weak<crate::copy_input_guard::CopyInputGuard>,
        rate_limit_recovery_pending: bool,
        recovered_queue: bool,
    }
);
