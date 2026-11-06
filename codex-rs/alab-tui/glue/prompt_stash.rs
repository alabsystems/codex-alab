// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A single temporary prompt per conversation, kept only in this CLI process.
//!
//! Move the native draft, attachments and Vim transaction state together: rebuilding
//! plain text would lose attachment element IDs, pending paste payloads, cursor,
//! mention targets and Replace-mode recovery. A second occupied prompt never replaces
//! the saved one. The map survives widget replacement and is keyed by the real thread
//! ID, so a new conversation cannot consume another conversation's saved prompt.

use super::super::AttachmentState;
use super::super::DraftState;
use super::super::sparkle::SparkleDraft;
use super::super::vim_history::VimHistory;
use crate::bottom_pane::BottomPane;
use crate::bottom_pane::ChatComposer;
use crate::bottom_pane::chat_composer_history::HistoryEntry;
use crate::key_hint;
use crate::key_hint::ShortcutHint;
use crate::keymap::KeymapContext;
use crate::keymap::RuntimeKeymap;
use codex_alab_tui::interface;
use codex_protocol::ThreadId;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;

const NOTICE_DURATION: Duration = Duration::from_secs(4);

struct SavedPrompt {
    draft: DraftState,
    attachments: AttachmentState,
    vim_history: VimHistory,
    pending_slash_history: Option<HistoryEntry>,
    sparkle: SparkleDraft,
}

fn prompts() -> &'static Mutex<HashMap<ThreadId, SavedPrompt>> {
    static PROMPTS: OnceLock<Mutex<HashMap<ThreadId, SavedPrompt>>> = OnceLock::new();
    PROMPTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ready(composer: &ChatComposer) -> bool {
    composer.input_enabled()
        && !composer.blocks_direct_input
        && !composer.history_search_active()
        && !composer.popup_active()
        && composer.draft.textarea.vim_query().is_none()
}

impl BottomPane {
    fn alab_stash_notice(&mut self, message: impl Into<String>) {
        let message: String = message.into();
        self.composer
            .show_footer_flash(message.into(), NOTICE_DURATION);
        self.record_composer_activity_at(Instant::now());
        self.request_redraw();
        self.request_redraw_in(NOTICE_DURATION);
    }

    /// Save an occupied prompt, or restore this conversation's saved prompt into an empty one.
    /// Refusals leave both prompts untouched. An absent session identity never owns a stash.
    pub(crate) fn alab_stash_prompt(&mut self) -> bool {
        if !interface::enabled() || !self.no_modal_or_popup_active() || !ready(&self.composer) {
            return false;
        }
        let Some(thread_id) = self.thread_id else {
            self.alab_stash_notice("Session is starting; stash will be available when ready");
            return false;
        };
        self.composer.flush_pending_paste_burst();
        self.composer
            .draft
            .paste_burst
            .clear_window_after_non_char();
        let mut prompts = prompts().lock().unwrap_or_else(PoisonError::into_inner);
        if self.composer.is_empty() {
            let Some(saved) = prompts.remove(&thread_id) else {
                drop(prompts);
                self.alab_stash_notice("No stashed prompt in this conversation");
                return false;
            };
            drop(prompts);
            self.composer.alab_restore_prompt(saved, &self.keymap);
            self.alab_stash_notice("Prompt restored");
            true
        } else {
            let Entry::Vacant(slot) = prompts.entry(thread_id) else {
                drop(prompts);
                self.alab_stash_notice(
                    "A prompt is already stashed; send or clear this draft before restoring",
                );
                return false;
            };
            slot.insert(self.composer.alab_take_prompt(&self.keymap));
            drop(prompts);
            let message = match self.alab_stash_shortcut_hint() {
                Some(hint) => format!(
                    "Prompt stashed · /stash or {} restores it in an empty prompt",
                    hint.display_label()
                ),
                None => "Prompt stashed · /stash restores it in an empty prompt".to_string(),
            };
            self.alab_stash_notice(message);
            true
        }
    }

    /// Describe the stash fallback outside search and popups. Resolve the same
    /// active editor/Vim contexts used for dispatch, including chord prefixes;
    /// an advertised shortcut must never promise to replace a configured action.
    pub(crate) fn alab_stash_shortcut_hint(&self) -> Option<ShortcutHint> {
        if !interface::enabled() {
            return None;
        }
        let binding = key_hint::ctrl(KeyCode::Char('s'));
        let (code, modifiers) = binding.parts();
        let key = KeyEvent::new(code, modifiers);
        let contexts = self
            .keymap_contexts()
            .with(KeymapContext::Global)
            .with(KeymapContext::Chat)
            .with(KeymapContext::Voice);
        let bound = crate::keymap::keymap_action_ids().any(|action| {
            // Ctrl+S means "newer match" only while history search owns input.
            // `ready` excludes that surface before this fallback can be considered.
            !(action.context == KeymapContext::Composer && action.action == "history_search_next")
                && contexts.contains_action(action)
                && crate::keymap::bindings_for_action(
                    &self.keymap,
                    action.context.config_name(),
                    action.action,
                )
                .is_some_and(|bindings| bindings.iter().any(|binding| binding.is_press(key)))
        }) || self
            .keymap
            .chords
            .leader
            .iter()
            .any(|binding| binding.is_press(key))
            || self.keymap.chords.bindings.iter().any(|binding| {
                contexts.contains_action(binding.action) && binding.chord.prefix.is_press(key)
            });
        (!bound).then_some(binding.into())
    }

    pub(crate) fn alab_shortcuts_help_hint(&self) -> Option<ShortcutHint> {
        self.keymap
            .primary_hint(KeymapContext::Composer, "toggle_shortcuts")
    }

    pub(crate) fn alab_stash_key(&mut self, key: KeyEvent) -> bool {
        if !interface::enabled()
            || key.kind != KeyEventKind::Press
            || !key_hint::ctrl(KeyCode::Char('s')).is_press(key)
            || !ready(&self.composer)
            || self.alab_stash_shortcut_hint().is_none()
        {
            return false;
        }
        self.alab_stash_prompt();
        true
    }
}

impl ChatComposer {
    fn alab_take_prompt(&mut self, keymap: &RuntimeKeymap) -> SavedPrompt {
        let mut fresh = DraftState::new();
        fresh.input_enabled = self.draft.input_enabled;
        fresh.input_disabled_placeholder = self.draft.input_disabled_placeholder.clone();
        fresh.disable_paste_burst = self.draft.disable_paste_burst;
        fresh.textarea.set_keymap_bindings(keymap);
        fresh.textarea.enable_vim_search();
        fresh
            .textarea
            .set_vim_enabled(self.draft.textarea.is_vim_enabled());
        if !self.draft.textarea.is_vim_normal_mode() {
            fresh.textarea.enter_vim_insert_mode();
        }
        let saved = SavedPrompt {
            draft: std::mem::replace(&mut self.draft, fresh),
            attachments: std::mem::take(&mut self.attachments),
            vim_history: std::mem::take(&mut self.vim_history),
            pending_slash_history: self.pending_slash_command_history.take(),
            sparkle: self.sparkle.draft.replace(SparkleDraft::Dismissed),
        };
        self.history.reset_navigation();
        self.sync_popups();
        saved
    }

    fn alab_restore_prompt(&mut self, saved: SavedPrompt, keymap: &RuntimeKeymap) {
        let current = std::mem::replace(&mut self.draft, saved.draft);
        let current_vim_enabled = current.textarea.is_vim_enabled();
        let vim_preference_changed = self.draft.textarea.is_vim_enabled() != current_vim_enabled;
        self.draft.input_enabled = current.input_enabled;
        self.draft.input_disabled_placeholder = current.input_disabled_placeholder;
        self.draft.disable_paste_burst = current.disable_paste_burst;
        self.draft.textarea.set_keymap_bindings(keymap);
        self.draft.textarea_state.get_mut().follow_cursor();
        self.attachments = saved.attachments;
        self.vim_history = saved.vim_history;
        self.pending_slash_command_history = saved.pending_slash_history;
        self.sparkle.draft.set(saved.sparkle);
        // A setting changed while the prompt was parked remains authoritative.
        // Native mode setup also clears old undo/Replace state for that transition.
        if vim_preference_changed {
            self.set_vim_enabled(current_vim_enabled);
        }
        self.history.reset_navigation();
        self.sync_popups();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event_sender::AppEventSender;
    use crate::bottom_pane::MentionBinding;
    use codex_config::types::KeybindingSpec;
    use codex_config::types::KeybindingsSpec;
    use codex_config::types::TuiKeymap;
    use crossterm::event::KeyModifiers;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    struct Conversation(ThreadId);

    impl Conversation {
        fn new() -> Self {
            Self(ThreadId::new())
        }
    }

    impl Drop for Conversation {
        fn drop(&mut self) {
            prompts().lock().unwrap().remove(&self.0);
        }
    }

    fn scope() -> interface::Scope {
        let state = Box::leak(Box::new(interface::State::new()));
        let scope = interface::scoped(state);
        interface::configure(true);
        scope
    }

    fn pane(conversation: &Conversation) -> BottomPane {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut pane = crate::bottom_pane::tests::test_pane(AppEventSender::new(tx));
        pane.set_history_metadata(conversation.0, 0, 0);
        pane.composer.set_disable_paste_burst(true);
        pane
    }

    fn ctrl_s() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
    }

    fn structured_prompt(pane: &mut BottomPane) {
        pane.composer
            .set_remote_image_urls(vec!["https://example.test/reference.png".to_string()]);
        pane.composer.set_text_content_with_mention_bindings(
            "ask $MySkill about this image ".into(),
            Vec::new(),
            vec![PathBuf::from("local-image.png")],
            vec![MentionBinding {
                sigil: '$',
                mention: "MySkill".into(),
                path: "/tmp/MySkill/SKILL.md".into(),
            }],
        );
        pane.composer.move_cursor_to_end();
        pane.composer
            .handle_paste("long payload with café\n".repeat(400));
        pane.composer.set_current_cursor(4);
    }

    #[test]
    fn structured_prompt_round_trip_keeps_attachment_ids_mentions_blobs_and_cursor() {
        let _scope = scope();
        let conversation = Conversation::new();
        let mut pane = pane(&conversation);
        structured_prompt(&mut pane);
        let before = pane.composer.snapshot_draft();
        assert!(!before.pending_pastes.is_empty());
        assert!(!before.mention_bindings.is_empty());
        assert!(pane.alab_stash_key(ctrl_s()));
        assert!(pane.composer.is_empty());

        pane.composer.insert_str("an intervening question");
        let occupied = pane.composer.snapshot_draft();
        assert!(!pane.alab_stash_prompt());
        assert_eq!(pane.composer.snapshot_draft(), occupied);

        pane.composer
            .set_text_content(String::new(), Vec::new(), Vec::new());
        pane.composer
            .attach_image(PathBuf::from("scratch-image.png"));
        let attachment_only = pane.composer.snapshot_draft();
        assert!(!pane.alab_stash_prompt());
        assert_eq!(pane.composer.snapshot_draft(), attachment_only);
        pane.composer
            .set_text_content(String::new(), Vec::new(), Vec::new());
        assert!(pane.alab_stash_prompt());
        assert_eq!(pane.composer.snapshot_draft(), before);
        assert!(!prompts().lock().unwrap().contains_key(&conversation.0));
    }

    #[test]
    fn each_conversation_recovers_its_prompt_after_widget_replacement() {
        let _scope = scope();
        let first = Conversation::new();
        let second = Conversation::new();
        let mut original = pane(&first);
        structured_prompt(&mut original);
        let first_draft = original.composer.snapshot_draft();
        assert!(original.alab_stash_prompt());
        drop(original);

        let mut other = pane(&second);
        assert!(!other.alab_stash_prompt());
        assert!(other.composer.is_empty());
        other.composer.insert_str("second conversation");
        assert!(other.alab_stash_prompt());

        let mut restored = pane(&first);
        assert!(restored.alab_stash_prompt());
        assert_eq!(restored.composer.snapshot_draft(), first_draft);
        assert!(other.alab_stash_prompt());
        assert_eq!(other.composer.current_text(), "second conversation");
    }

    #[test]
    fn vim_replace_recovery_and_undo_survive_stashing() {
        let _scope = scope();
        let conversation = Conversation::new();
        let mut pane = pane(&conversation);
        pane.composer.set_vim_enabled(true);
        pane.composer
            .set_text_content("abcdef".into(), Vec::new(), Vec::new());
        pane.composer.handle_key_event(KeyCode::Char('R').into());
        pane.composer.handle_key_event(KeyCode::Char('X').into());
        assert_eq!(pane.composer.current_text(), "Xbcdef");
        assert!(pane.composer.draft.textarea.is_vim_replace_mode());
        assert!(pane.alab_stash_key(ctrl_s()));
        assert!(pane.alab_stash_key(ctrl_s()));
        assert!(pane.composer.draft.textarea.is_vim_replace_mode());
        pane.composer.handle_key_event(KeyCode::Backspace.into());
        assert_eq!(pane.composer.current_text(), "abcdef");

        pane.composer.handle_key_event(KeyCode::Esc.into());
        pane.composer.handle_key_event(KeyCode::Char('x').into());
        let edited = pane.composer.current_text();
        assert_ne!(edited, "abcdef");
        assert!(pane.alab_stash_prompt());
        assert!(pane.alab_stash_prompt());
        assert!(pane.composer.draft.textarea.is_vim_normal_mode());
        pane.composer.handle_key_event(KeyCode::Char('u').into());
        assert_eq!(pane.composer.current_text(), "abcdef");
    }

    #[test]
    fn changing_vim_preference_while_parked_keeps_the_new_preference() {
        let _scope = scope();
        for initially_enabled in [true, false] {
            let conversation = Conversation::new();
            let mut pane = pane(&conversation);
            pane.set_vim_enabled(initially_enabled);
            structured_prompt(&mut pane);
            let original = pane.composer.snapshot_draft();
            assert!(pane.alab_stash_prompt());
            pane.set_vim_enabled(!initially_enabled);
            assert!(pane.alab_stash_prompt());
            assert_eq!(pane.composer.snapshot_draft(), original);
            assert_eq!(
                pane.composer.draft.textarea.is_vim_enabled(),
                !initially_enabled
            );
            assert_eq!(
                pane.composer.draft.textarea.is_vim_normal_mode(),
                !initially_enabled
            );
            assert!(!pane.composer.draft.textarea.is_vim_replace_mode());
        }
    }

    #[test]
    fn the_scratch_prompt_keeps_native_vim_search() {
        let _scope = scope();
        let conversation = Conversation::new();
        let mut pane = pane(&conversation);
        pane.set_vim_enabled(true);
        pane.composer
            .set_text_content("parked prompt".into(), Vec::new(), Vec::new());
        assert!(pane.alab_stash_prompt());
        pane.composer.insert_str("fresh search target");
        pane.composer.handle_key_event(KeyCode::Esc.into());
        pane.composer.handle_key_event(KeyCode::Char('/').into());
        assert!(pane.composer.draft.textarea.vim_query().is_some());
        assert!(!pane.alab_stash_prompt());
        pane.composer.handle_key_event(KeyCode::Esc.into());
        pane.composer
            .set_text_content(String::new(), Vec::new(), Vec::new());
        assert!(pane.alab_stash_prompt());
        assert_eq!(pane.composer.current_text(), "parked prompt");
    }

    #[test]
    fn pending_keystroke_is_flushed_before_the_prompt_is_parked() {
        let _scope = scope();
        let conversation = Conversation::new();
        let mut pane = pane(&conversation);
        pane.composer.set_disable_paste_burst(false);
        pane.composer.handle_key_event(KeyCode::Char('z').into());
        let expected = pane.composer.recovery_snapshot().text;
        assert_eq!(expected, "z");
        assert!(pane.alab_stash_key(ctrl_s()));
        assert!(pane.alab_stash_prompt());
        assert_eq!(pane.composer.current_text(), expected);
    }

    #[test]
    fn native_search_remaps_and_chord_prefixes_keep_ctrl_s() {
        let _scope = scope();
        let conversation = Conversation::new();
        let mut pane = pane(&conversation);
        pane.composer.insert_str("original draft");
        assert!(pane.alab_stash_shortcut_hint().is_some());
        pane.composer.begin_history_search();
        assert!(!pane.alab_stash_key(ctrl_s()));
        pane.handle_key_event(ctrl_s());
        assert!(pane.composer.history_search_active());
        pane.composer.cancel_history_search();
        assert_eq!(pane.composer.current_text(), "original draft");

        let mut config = TuiKeymap::default();
        config.composer.history_search_next = Some(KeybindingsSpec::Many(Vec::new()));
        config.editor.move_left = Some(KeybindingsSpec::One(KeybindingSpec("ctrl-s".into())));
        pane.set_keymap_bindings(&RuntimeKeymap::from_config(&config).unwrap());
        assert!(pane.alab_stash_shortcut_hint().is_none());
        let before = pane.composer.current_cursor();
        assert!(!pane.alab_stash_key(ctrl_s()));
        pane.handle_key_event(ctrl_s());
        assert_eq!(pane.composer.current_cursor(), before - 1);
        assert_eq!(pane.composer.current_text(), "original draft");

        config.editor.move_left = None;
        config.global.open_external_editor =
            Some(KeybindingsSpec::One(KeybindingSpec("ctrl-s ctrl-e".into())));
        pane.set_keymap_bindings(&RuntimeKeymap::from_config(&config).unwrap());
        assert!(pane.alab_stash_shortcut_hint().is_none());
        assert!(!pane.alab_stash_key(ctrl_s()));
        assert!(!prompts().lock().unwrap().contains_key(&conversation.0));
    }

    #[test]
    fn stash_feedback_uses_a_restore_command_when_the_shortcut_is_claimed() {
        let _scope = scope();
        let conversation = Conversation::new();
        let mut pane = pane(&conversation);
        let mut config = TuiKeymap::default();
        config.composer.history_search_next = Some(KeybindingsSpec::Many(Vec::new()));
        config.editor.move_left = Some(KeybindingsSpec::One(KeybindingSpec("ctrl-s".into())));
        pane.set_keymap_bindings(&RuntimeKeymap::from_config(&config).unwrap());
        pane.composer.insert_str("keep this prompt");
        assert!(pane.alab_stash_prompt());
        let flash = pane.composer.footer.flash.as_ref().unwrap();
        let text: String = flash
            .line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains("/stash restores"));
        assert!(!text.contains(&key_hint::ctrl(KeyCode::Char('s')).display_label()));
        assert!(!pane.alab_stash_key(ctrl_s()));
        assert!(pane.alab_stash_prompt());
        assert_eq!(pane.composer.current_text(), "keep this prompt");
    }

    #[test]
    fn native_off_and_disabled_or_unidentified_composers_are_untouched() {
        let _scope = scope();
        let conversation = Conversation::new();
        let mut pane = pane(&conversation);
        structured_prompt(&mut pane);
        let before = pane.composer.snapshot_draft();
        interface::set_enabled(false);
        assert!(!pane.alab_stash_key(ctrl_s()));
        assert!(!pane.alab_stash_prompt());
        assert_eq!(pane.composer.snapshot_draft(), before);
        interface::set_enabled(true);
        pane.set_composer_input_enabled(false, None);
        assert!(!pane.alab_stash_key(ctrl_s()));
        assert!(!pane.alab_stash_prompt());
        assert_eq!(pane.composer.snapshot_draft(), before);
        pane.set_composer_input_enabled(true, None);
        pane.composer.blocks_direct_input = true;
        assert!(!pane.alab_stash_prompt());
        pane.composer.blocks_direct_input = false;
        pane.thread_id = None;
        assert!(!pane.alab_stash_prompt());
        assert_eq!(pane.composer.snapshot_draft(), before);
        assert!(!prompts().lock().unwrap().contains_key(&conversation.0));
    }
}
