// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::InputResult;
use super::SlashCommand;
use super::SlashCommandItem;
use crate::bottom_pane::BottomPane;
use crate::key_hint::KeyBindingListExt;
use crossterm::event::KeyEvent;

impl BottomPane {
    /// Let the native dispatcher consume only a complete, visible local picker command.
    pub(crate) fn alab_submit_autopilot_menu(&mut self, key: KeyEvent) -> Option<InputResult> {
        let composer = &mut self.composer;
        if !composer.submit_keys.is_pressed(key)
            || composer.handles_key_as_editing(key)
            || !composer.attachments.is_empty()
            || !composer.draft.pending_pastes.is_empty()
        {
            return None;
        }
        let text = composer.draft.textarea.text();
        let inline = composer.slash_input().inline_command(text)?;
        if text.contains(['\r', '\n'])
            || !matches!(
                inline.command,
                SlashCommandItem::Builtin(SlashCommand::Alab)
            )
            || !inline.rest.trim().eq_ignore_ascii_case("menu")
        {
            return None;
        }
        composer.try_dispatch_slash_command_with_args()
    }
}
