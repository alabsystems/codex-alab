// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Composer-owned data projections for the complete draft/stash checkpoint.
use super::super::super::*;

crate::alab_editor_upgrade::native_state!(
    crate::bottom_pane::LocalImageAttachment,
    LocalImageAttachmentWire,
    #[serde(
        remote = "crate::bottom_pane::LocalImageAttachment",
        deny_unknown_fields
    )]
    struct LocalImageAttachmentWire {
        placeholder: String,
        path: PathBuf,
    }
);
crate::alab_editor_upgrade::native_state!(
    crate::bottom_pane::MentionBinding,
    MentionBindingWire,
    #[serde(remote = "crate::bottom_pane::MentionBinding", deny_unknown_fields)]
    struct MentionBindingWire {
        sigil: char,
        mention: String,
        path: String,
    }
);
crate::alab_editor_upgrade::native_state!(
    crate::bottom_pane::chat_composer::ComposerDraft,
    ComposerDraftWire,
    #[serde(
        remote = "crate::bottom_pane::chat_composer::ComposerDraft",
        deny_unknown_fields
    )]
    struct ComposerDraftWire {
        is_bash_mode: bool,
        text: String,
        text_elements: Vec<TextElement>,
        local_image_paths: Vec<PathBuf>,
        remote_image_urls: Vec<String>,
        mention_bindings: Vec<MentionBinding>,
        mention_ranges: Vec<Range<usize>>,
        pending_pastes: Vec<(String, String)>,
        cursor: usize,
    }
);
crate::alab_editor_upgrade::native_state!(
    crate::bottom_pane::chat_composer::sparkle::SparkleDraft,
    SparkleDraftWire,
    #[serde(
        remote = "crate::bottom_pane::chat_composer::sparkle::SparkleDraft",
        deny_unknown_fields
    )]
    enum SparkleDraftWire {
        Untouched,
        Command,
        Dismissed,
    }
);

crate::alab_editor_upgrade::native_state!(
    crate::bottom_pane::chat_composer_history::HistoryEntry,
    HistoryEntryWire,
    #[serde(
        remote = "crate::bottom_pane::chat_composer_history::HistoryEntry",
        deny_unknown_fields
    )]
    struct HistoryEntryWire {
        text: String,
        text_elements: Vec<TextElement>,
        local_image_paths: Vec<PathBuf>,
        remote_image_urls: Vec<String>,
        mention_bindings: Vec<MentionBinding>,
        pending_pastes: Vec<(String, String)>,
    }
);
crate::alab_editor_upgrade::native_state!(
    super::SavedPrompt,
    SavedPromptWire,
    #[serde(remote = "super::SavedPrompt", deny_unknown_fields)]
    struct SavedPromptWire {
        draft: DraftState,
        attachments: AttachmentState,
        vim_history: vim_history::VimHistory,
        pending_slash_history: Option<crate::bottom_pane::chat_composer_history::HistoryEntry>,
        sparkle: sparkle::SparkleDraft,
    }
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event_sender::AppEventSender;
    use crate::bottom_pane::textarea::TextElementSnapshot;
    use codex_alab_tui::upgrade_editor_graph_scope as graph;
    use crossterm::event::KeyModifiers;

    fn composer() -> ChatComposer {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut composer =
            ChatComposer::new(true, AppEventSender::new(tx), false, String::new(), false);
        composer.set_disable_paste_burst(true);
        composer
    }
    fn structured(composer: &mut ChatComposer) {
        composer.set_remote_image_urls(vec!["https://example.test/reference.png".into()]);
        composer.set_text_content_with_mention_bindings(
            "ask $MySkill about this image ".into(),
            Vec::new(),
            vec![PathBuf::from("local-image.png")],
            vec![MentionBinding {
                sigil: '$',
                mention: "MySkill".into(),
                path: "/tmp/MySkill/SKILL.md".into(),
            }],
        );
        composer.move_cursor_to_end();
        composer.handle_paste("long payload with café\n".repeat(400));
        composer.set_current_cursor(0);
    }
    fn rich_source() -> ChatComposer {
        let mut composer = composer();
        composer.set_vim_enabled(true);
        composer.insert_selected_mention(0..0, "$skill", Some("/original/skill/SKILL.md"));
        composer.attach_image(PathBuf::from("/original/photo.png"));
        composer.handle_paste("z".repeat(LARGE_PASTE_CHAR_THRESHOLD + 1));
        composer.popups.active = ActivePopup::None;
        key(&mut composer, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut composer, KeyCode::Char('0'), KeyModifiers::NONE);
        composer
    }
    fn snapshot(composer: &ChatComposer) -> serde_json::Value {
        serde_json::to_value(composer.snapshot_draft()).unwrap()
    }
    fn key(composer: &mut ChatComposer, key: KeyCode, modifiers: KeyModifiers) {
        composer.handle_key_event(KeyEvent::new(key, modifiers));
    }

    #[test]
    fn native_structured_saved_prompt_keeps_attachments_mentions_blobs_and_slash_history() {
        let mut source = composer();
        structured(&mut source);
        source.pending_slash_command_history =
            Some(crate::bottom_pane::chat_composer_history::HistoryEntry::new("/model".into()));
        source.sparkle.draft.set(sparkle::SparkleDraft::Command);
        let expected = snapshot(&source);
        let keymap = RuntimeKeymap::defaults();
        let saved = source.alab_take_prompt(&keymap);
        let bytes = graph::encode::<Vec<TextElementSnapshot>>(&saved, 4 * 1024 * 1024).unwrap();
        let restored: super::super::SavedPrompt =
            graph::decode::<Vec<TextElementSnapshot>, _>(&bytes, 4 * 1024 * 1024, 1024).unwrap();
        let mut destination = composer();
        destination.alab_restore_prompt(restored, &keymap);
        assert_eq!(snapshot(&destination), expected);
        assert_eq!(
            destination
                .pending_slash_command_history
                .as_ref()
                .unwrap()
                .text,
            "/model"
        );
        assert_eq!(
            destination.sparkle.draft.get(),
            sparkle::SparkleDraft::Command
        );
        assert!(!destination.draft.pending_pastes.is_empty());
        assert!(!destination.draft.mention_bindings.is_empty());
        assert_eq!(destination.attachments.local_images.len(), 1);
        assert_eq!(destination.attachments.remote_image_urls.len(), 1);
    }

    #[test]
    fn native_rich_register_paste_undo_and_redo_keep_metadata_after_checkpoint() {
        let mut source = rich_source();
        for ch in ['y', '$'] {
            key(&mut source, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        for (code, modifiers) in [
            (KeyCode::Char('i'), KeyModifiers::NONE),
            (KeyCode::Char('y'), KeyModifiers::CONTROL),
            (KeyCode::Esc, KeyModifiers::NONE),
            (KeyCode::Char('u'), KeyModifiers::NONE),
            (KeyCode::Char('i'), KeyModifiers::NONE),
            (KeyCode::Char('x'), KeyModifiers::NONE),
        ] {
            key(&mut source, code, modifiers);
        }
        vim_register::sync(&mut source);
        let bytes = graph::encode::<Vec<TextElementSnapshot>>(
            &(
                &source.draft,
                &source.attachments,
                &source.vim_register,
                &source.vim_history,
            ),
            4 * 1024 * 1024,
        )
        .unwrap();
        let wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            !wire["body"][2].as_array().unwrap().is_empty(),
            "fixture must have a live rich register before capture"
        );
        assert!(
            !wire["body"][3]["redo"].as_array().unwrap().is_empty(),
            "fixture must have native redo history before capture"
        );
        assert!(
            !wire["body"][3]["pending"].is_null(),
            "fixture must have a pending native edit before capture"
        );
        let mut invalid_payload = wire.clone();
        invalid_payload["body"][2][0]["payloads"]["18446744073709551615"] =
            serde_json::json!({"Paste": "foreign"});
        type DraftBundle = (
            DraftState,
            AttachmentState,
            vim_register::VimRegister,
            vim_history::VimHistory,
        );
        let error = graph::decode::<Vec<TextElementSnapshot>, DraftBundle>(
            &serde_json::to_vec(&invalid_payload).unwrap(),
            4 * 1024 * 1024,
            1024,
        )
        .err()
        .expect("foreign register element must be rejected");
        assert!(error.contains("absent element"), "{error}");
        let mut invalid_history = wire;
        invalid_history["body"][3]["undo"] =
            serde_json::to_value(vec![snapshot(&source); 65]).unwrap();
        let error = graph::decode::<Vec<TextElementSnapshot>, DraftBundle>(
            &serde_json::to_vec(&invalid_history).unwrap(),
            4 * 1024 * 1024,
            1024,
        )
        .err()
        .expect("oversized native history must be rejected");
        assert!(error.contains("history exceeds"), "{error}");
        let (draft, attachments, register, history): (
            DraftState,
            AttachmentState,
            vim_register::VimRegister,
            vim_history::VimHistory,
        ) = graph::decode::<Vec<TextElementSnapshot>, _>(&bytes, 4 * 1024 * 1024, 1024).unwrap();
        let mut destination = composer();
        destination.draft = draft;
        destination.attachments = attachments;
        destination.vim_register = register;
        destination.vim_history = history;
        assert_eq!(snapshot(&destination), snapshot(&source));
        for (code, modifiers) in [
            (KeyCode::Esc, KeyModifiers::NONE),
            (KeyCode::Char('u'), KeyModifiers::NONE),
            (KeyCode::Char('r'), KeyModifiers::CONTROL),
            (KeyCode::Char('u'), KeyModifiers::NONE),
            (KeyCode::Char('i'), KeyModifiers::NONE),
            (KeyCode::Char('y'), KeyModifiers::CONTROL),
            (KeyCode::Esc, KeyModifiers::NONE),
            (KeyCode::Char('u'), KeyModifiers::NONE),
            (KeyCode::Char('r'), KeyModifiers::CONTROL),
        ] {
            key(&mut source, code, modifiers);
            key(&mut destination, code, modifiers);
            assert_eq!(snapshot(&destination), snapshot(&source));
        }
        // A plain-text-only transfer could compare equal before paste but lose all these.
        assert_eq!(destination.attachments.local_images.len(), 2);
        assert_eq!(destination.draft.pending_pastes.len(), 2);
        assert_eq!(destination.draft.mention_bindings.len(), 2);
    }
}
