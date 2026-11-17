// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve the original confirmed-but-not-dispatched startup draft unchanged.
//! A chat_composer descendant through the prompt stash glue, beside its editor DATA codecs.
use super::super::super::*;

crate::alab_editor_upgrade::native_state!(
    ComposerDraftSnapshot,
    SubmissionWire,
    #[serde(remote = "ComposerDraftSnapshot", deny_unknown_fields)]
    struct SubmissionWire {
        vim_normal_mode: bool,
        text: String,
        shell_mode: Option<bool>,
        cursor: usize,
        text_elements: Vec<TextElement>,
        local_images: Vec<LocalImageAttachment>,
        remote_image_urls: Vec<String>,
        mention_bindings: Vec<MentionBinding>,
        pending_pastes: Vec<(String, String)>,
        startup_local_history: Vec<crate::bottom_pane::chat_composer_history::HistoryEntry>,
        #[serde(with = "codex_alab_tui::upgrade_editor_clock::optional")]
        last_composer_activity_at: Option<Instant>,
        sparkle_draft: sparkle::SparkleDraft,
    }
);
