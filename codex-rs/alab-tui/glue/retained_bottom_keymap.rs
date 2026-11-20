// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native binding vectors and immutable aliases. No defaults/config/action factories.
use super::*;
use chords::alab_retained::BindingKey;
use codex_alab_tui::retained_arc_graph::Capture as Identities;
use codex_alab_tui::retained_arc_graph::Id;
use serde::Deserialize;
use serde::Serialize;
use std::io;
const MAX_NODES: usize = 1_000_000;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn keys(actual: &[KeyBinding], context: &str, action: &str) -> Vec<BindingKey> {
    chords::alab_retained::capture_bindings(actual, context, action)
}
fn restore_keys(
    actual: Vec<BindingKey>,
    graph: &Restored,
    context: &str,
    action: &str,
) -> io::Result<Vec<KeyBinding>> {
    graph.inventory.restore_bindings(actual, context, action)
}
/// Standalone mirrored native action bindings use this same typed dispatch map;
/// KeyData remains only raw physical binding DATA.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct BindingsData(Vec<BindingKey>);
impl BindingsData {
    pub(crate) fn capture(actual: &[KeyBinding], action: KeymapActionId) -> io::Result<Self> {
        if keymap_action_id(action.context.config_name(), action.action) != Some(action) {
            return Err(invalid("native binding source action identity changed"));
        }
        Ok(Self(keys(
            actual,
            action.context.config_name(),
            action.action,
        )))
    }
    pub(crate) fn restore(
        self,
        graph: &Restored,
        action: KeymapActionId,
    ) -> io::Result<Vec<KeyBinding>> {
        if keymap_action_id(action.context.config_name(), action.action) != Some(action) {
            return Err(invalid("native binding target action identity changed"));
        }
        restore_keys(self.0, graph, action.context.config_name(), action.action)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct EditorRef(Id);
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ChordRef(Id);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppKeymapData {
    change_host: Vec<BindingKey>,
    focus_main_composer: Vec<BindingKey>,
    focus_sidebar_content: Vec<BindingKey>,
    focus_sidebar_tabs: Vec<BindingKey>,
    hide_pane: Vec<BindingKey>,
    close_sidebar_tab: Vec<BindingKey>,
    sidebar_up: Vec<BindingKey>,
    sidebar_down: Vec<BindingKey>,
    focus_pane_left: Vec<BindingKey>,
    focus_pane_right: Vec<BindingKey>,
    minimize_pane_width: Vec<BindingKey>,
    maximize_pane_width: Vec<BindingKey>,
    equalize_pane_widths: Vec<BindingKey>,
    resize_panes: Vec<BindingKey>,
    move_divider_left: Vec<BindingKey>,
    move_divider_right: Vec<BindingKey>,
    toggle_pane_zoom: Vec<BindingKey>,
    open_agents: Vec<BindingKey>,
    open_transcript: Vec<BindingKey>,
    find_transcript: Vec<BindingKey>,
    select_transcript: Vec<BindingKey>,
    focus_activity: Vec<BindingKey>,
    open_warnings: Vec<BindingKey>,
    open_external_editor: Vec<BindingKey>,
    copy: Vec<BindingKey>,
    clear_terminal: Vec<BindingKey>,
    toggle_vim_mode: Vec<BindingKey>,
    toggle_fast_mode: Vec<BindingKey>,
    toggle_raw_output: Vec<BindingKey>,
    toggle_side_conversation: Vec<BindingKey>,
}
impl AppKeymapData {
    fn capture(actual: &AppKeymap, _graph: &mut Capture) -> io::Result<Self> {
        let AppKeymap {
            change_host,
            focus_main_composer,
            focus_sidebar_content,
            focus_sidebar_tabs,
            hide_pane,
            close_sidebar_tab,
            sidebar_up,
            sidebar_down,
            focus_pane_left,
            focus_pane_right,
            minimize_pane_width,
            maximize_pane_width,
            equalize_pane_widths,
            resize_panes,
            move_divider_left,
            move_divider_right,
            toggle_pane_zoom,
            open_agents,
            open_transcript,
            find_transcript,
            select_transcript,
            focus_activity,
            open_warnings,
            open_external_editor,
            copy,
            clear_terminal,
            toggle_vim_mode,
            toggle_fast_mode,
            toggle_raw_output,
            toggle_side_conversation,
        } = actual;
        Ok(Self {
            change_host: keys(change_host, "global", "change_host"),
            focus_main_composer: keys(focus_main_composer, "global", "focus_main_composer"),
            focus_sidebar_content: keys(focus_sidebar_content, "global", "focus_sidebar_content"),
            focus_sidebar_tabs: keys(focus_sidebar_tabs, "global", "focus_sidebar_tabs"),
            hide_pane: keys(hide_pane, "global", "hide_pane"),
            close_sidebar_tab: keys(close_sidebar_tab, "global", "close_sidebar_tab"),
            sidebar_up: keys(sidebar_up, "global", "sidebar_up"),
            sidebar_down: keys(sidebar_down, "global", "sidebar_down"),
            focus_pane_left: keys(focus_pane_left, "global", "focus_pane_left"),
            focus_pane_right: keys(focus_pane_right, "global", "focus_pane_right"),
            minimize_pane_width: keys(minimize_pane_width, "global", "minimize_pane_width"),
            maximize_pane_width: keys(maximize_pane_width, "global", "maximize_pane_width"),
            equalize_pane_widths: keys(equalize_pane_widths, "global", "equalize_pane_widths"),
            resize_panes: keys(resize_panes, "global", "resize_panes"),
            move_divider_left: keys(move_divider_left, "global", "move_divider_left"),
            move_divider_right: keys(move_divider_right, "global", "move_divider_right"),
            toggle_pane_zoom: keys(toggle_pane_zoom, "global", "toggle_pane_zoom"),
            open_agents: keys(open_agents, "global", "open_agents"),
            open_transcript: keys(open_transcript, "global", "open_transcript"),
            find_transcript: keys(find_transcript, "global", "find_transcript"),
            select_transcript: keys(select_transcript, "global", "select_transcript"),
            focus_activity: keys(focus_activity, "global", "focus_activity"),
            open_warnings: keys(open_warnings, "global", "open_warnings"),
            open_external_editor: keys(open_external_editor, "global", "open_external_editor"),
            copy: keys(copy, "global", "copy"),
            clear_terminal: keys(clear_terminal, "global", "clear_terminal"),
            toggle_vim_mode: keys(toggle_vim_mode, "global", "toggle_vim_mode"),
            toggle_fast_mode: keys(toggle_fast_mode, "global", "toggle_fast_mode"),
            toggle_raw_output: keys(toggle_raw_output, "global", "toggle_raw_output"),
            toggle_side_conversation: keys(
                toggle_side_conversation,
                "global",
                "toggle_side_conversation",
            ),
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<AppKeymap> {
        Ok(AppKeymap {
            change_host: restore_keys(self.change_host, graph, "global", "change_host")?,
            focus_main_composer: restore_keys(
                self.focus_main_composer,
                graph,
                "global",
                "focus_main_composer",
            )?,
            focus_sidebar_content: restore_keys(
                self.focus_sidebar_content,
                graph,
                "global",
                "focus_sidebar_content",
            )?,
            focus_sidebar_tabs: restore_keys(
                self.focus_sidebar_tabs,
                graph,
                "global",
                "focus_sidebar_tabs",
            )?,
            hide_pane: restore_keys(self.hide_pane, graph, "global", "hide_pane")?,
            close_sidebar_tab: restore_keys(
                self.close_sidebar_tab,
                graph,
                "global",
                "close_sidebar_tab",
            )?,
            sidebar_up: restore_keys(self.sidebar_up, graph, "global", "sidebar_up")?,
            sidebar_down: restore_keys(self.sidebar_down, graph, "global", "sidebar_down")?,
            focus_pane_left: restore_keys(
                self.focus_pane_left,
                graph,
                "global",
                "focus_pane_left",
            )?,
            focus_pane_right: restore_keys(
                self.focus_pane_right,
                graph,
                "global",
                "focus_pane_right",
            )?,
            minimize_pane_width: restore_keys(
                self.minimize_pane_width,
                graph,
                "global",
                "minimize_pane_width",
            )?,
            maximize_pane_width: restore_keys(
                self.maximize_pane_width,
                graph,
                "global",
                "maximize_pane_width",
            )?,
            equalize_pane_widths: restore_keys(
                self.equalize_pane_widths,
                graph,
                "global",
                "equalize_pane_widths",
            )?,
            resize_panes: restore_keys(self.resize_panes, graph, "global", "resize_panes")?,
            move_divider_left: restore_keys(
                self.move_divider_left,
                graph,
                "global",
                "move_divider_left",
            )?,
            move_divider_right: restore_keys(
                self.move_divider_right,
                graph,
                "global",
                "move_divider_right",
            )?,
            toggle_pane_zoom: restore_keys(
                self.toggle_pane_zoom,
                graph,
                "global",
                "toggle_pane_zoom",
            )?,
            open_agents: restore_keys(self.open_agents, graph, "global", "open_agents")?,
            open_transcript: restore_keys(
                self.open_transcript,
                graph,
                "global",
                "open_transcript",
            )?,
            find_transcript: restore_keys(
                self.find_transcript,
                graph,
                "global",
                "find_transcript",
            )?,
            select_transcript: restore_keys(
                self.select_transcript,
                graph,
                "global",
                "select_transcript",
            )?,
            focus_activity: restore_keys(self.focus_activity, graph, "global", "focus_activity")?,
            open_warnings: restore_keys(self.open_warnings, graph, "global", "open_warnings")?,
            open_external_editor: restore_keys(
                self.open_external_editor,
                graph,
                "global",
                "open_external_editor",
            )?,
            copy: restore_keys(self.copy, graph, "global", "copy")?,
            clear_terminal: restore_keys(self.clear_terminal, graph, "global", "clear_terminal")?,
            toggle_vim_mode: restore_keys(
                self.toggle_vim_mode,
                graph,
                "global",
                "toggle_vim_mode",
            )?,
            toggle_fast_mode: restore_keys(
                self.toggle_fast_mode,
                graph,
                "global",
                "toggle_fast_mode",
            )?,
            toggle_raw_output: restore_keys(
                self.toggle_raw_output,
                graph,
                "global",
                "toggle_raw_output",
            )?,
            toggle_side_conversation: restore_keys(
                self.toggle_side_conversation,
                graph,
                "global",
                "toggle_side_conversation",
            )?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatKeymapData {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    open_autopilot: Vec<BindingKey>,
    open_presets: Vec<BindingKey>,
    toggle_voice: Vec<BindingKey>,
    toggle_voice_mute: Vec<BindingKey>,
    chord_hints: ChordRef,
    interrupt_turn: Vec<BindingKey>,
    decrease_reasoning_effort: Vec<BindingKey>,
    increase_reasoning_effort: Vec<BindingKey>,
    previous_permission_mode: Vec<BindingKey>,
    next_permission_mode: Vec<BindingKey>,
    edit_queued_message: Vec<BindingKey>,
    prompt_stack_back: Vec<BindingKey>,
    skip_question: Vec<BindingKey>,
}
impl ChatKeymapData {
    fn capture(actual: &ChatKeymap, graph: &mut Capture) -> io::Result<Self> {
        let ChatKeymap {
            open_autopilot,
            open_presets,
            toggle_voice,
            toggle_voice_mute,
            chord_hints,
            interrupt_turn,
            decrease_reasoning_effort,
            increase_reasoning_effort,
            previous_permission_mode,
            next_permission_mode,
            edit_queued_message,
            prompt_stack_back,
            skip_question,
        } = actual;
        Ok(Self {
            open_autopilot: keys(open_autopilot, "chat", "open_autopilot"),
            open_presets: keys(open_presets, "chat", "open_presets"),
            toggle_voice: keys(toggle_voice, "chat", "toggle_voice"),
            toggle_voice_mute: keys(toggle_voice_mute, "chat", "toggle_voice_mute"),
            chord_hints: graph.chords(chord_hints)?,
            interrupt_turn: keys(interrupt_turn, "chat", "interrupt_turn"),
            decrease_reasoning_effort: keys(
                decrease_reasoning_effort,
                "chat",
                "decrease_reasoning_effort",
            ),
            increase_reasoning_effort: keys(
                increase_reasoning_effort,
                "chat",
                "increase_reasoning_effort",
            ),
            previous_permission_mode: keys(
                previous_permission_mode,
                "chat",
                "previous_permission_mode",
            ),
            next_permission_mode: keys(next_permission_mode, "chat", "next_permission_mode"),
            edit_queued_message: keys(edit_queued_message, "chat", "edit_queued_message"),
            prompt_stack_back: keys(prompt_stack_back, "chat", "prompt_stack_back"),
            skip_question: keys(skip_question, "chat", "skip_question"),
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<ChatKeymap> {
        Ok(ChatKeymap {
            open_autopilot: restore_keys(self.open_autopilot, graph, "chat", "open_autopilot")?,
            open_presets: restore_keys(self.open_presets, graph, "chat", "open_presets")?,
            toggle_voice: restore_keys(self.toggle_voice, graph, "chat", "toggle_voice")?,
            toggle_voice_mute: restore_keys(
                self.toggle_voice_mute,
                graph,
                "chat",
                "toggle_voice_mute",
            )?,
            chord_hints: graph.chords(self.chord_hints)?,
            interrupt_turn: restore_keys(self.interrupt_turn, graph, "chat", "interrupt_turn")?,
            decrease_reasoning_effort: restore_keys(
                self.decrease_reasoning_effort,
                graph,
                "chat",
                "decrease_reasoning_effort",
            )?,
            increase_reasoning_effort: restore_keys(
                self.increase_reasoning_effort,
                graph,
                "chat",
                "increase_reasoning_effort",
            )?,
            previous_permission_mode: restore_keys(
                self.previous_permission_mode,
                graph,
                "chat",
                "previous_permission_mode",
            )?,
            next_permission_mode: restore_keys(
                self.next_permission_mode,
                graph,
                "chat",
                "next_permission_mode",
            )?,
            edit_queued_message: restore_keys(
                self.edit_queued_message,
                graph,
                "chat",
                "edit_queued_message",
            )?,
            prompt_stack_back: restore_keys(
                self.prompt_stack_back,
                graph,
                "chat",
                "prompt_stack_back",
            )?,
            skip_question: restore_keys(self.skip_question, graph, "chat", "skip_question")?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComposerKeymapData {
    submit: Vec<BindingKey>,
    queue: Vec<BindingKey>,
    toggle_shortcuts: Vec<BindingKey>,
    history_search_previous: Vec<BindingKey>,
    history_search_next: Vec<BindingKey>,
}
impl ComposerKeymapData {
    fn capture(actual: &ComposerKeymap, _graph: &mut Capture) -> io::Result<Self> {
        let ComposerKeymap {
            submit,
            queue,
            toggle_shortcuts,
            history_search_previous,
            history_search_next,
        } = actual;
        Ok(Self {
            submit: keys(submit, "composer", "submit"),
            queue: keys(queue, "composer", "queue"),
            toggle_shortcuts: keys(toggle_shortcuts, "composer", "toggle_shortcuts"),
            history_search_previous: keys(
                history_search_previous,
                "composer",
                "history_search_previous",
            ),
            history_search_next: keys(history_search_next, "composer", "history_search_next"),
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<ComposerKeymap> {
        Ok(ComposerKeymap {
            submit: restore_keys(self.submit, graph, "composer", "submit")?,
            queue: restore_keys(self.queue, graph, "composer", "queue")?,
            toggle_shortcuts: restore_keys(
                self.toggle_shortcuts,
                graph,
                "composer",
                "toggle_shortcuts",
            )?,
            history_search_previous: restore_keys(
                self.history_search_previous,
                graph,
                "composer",
                "history_search_previous",
            )?,
            history_search_next: restore_keys(
                self.history_search_next,
                graph,
                "composer",
                "history_search_next",
            )?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorKeymapData {
    insert_newline: Vec<BindingKey>,
    move_left: Vec<BindingKey>,
    move_right: Vec<BindingKey>,
    move_up: Vec<BindingKey>,
    move_down: Vec<BindingKey>,
    move_word_left: Vec<BindingKey>,
    move_word_right: Vec<BindingKey>,
    move_line_start: Vec<BindingKey>,
    move_line_end: Vec<BindingKey>,
    delete_backward: Vec<BindingKey>,
    delete_forward: Vec<BindingKey>,
    delete_backward_word: Vec<BindingKey>,
    delete_forward_word: Vec<BindingKey>,
    kill_line_start: Vec<BindingKey>,
    kill_whole_line: Vec<BindingKey>,
    kill_line_end: Vec<BindingKey>,
    yank: Vec<BindingKey>,
}
impl EditorKeymapData {
    fn capture(actual: &EditorKeymap, _graph: &mut Capture) -> io::Result<Self> {
        let EditorKeymap {
            insert_newline,
            move_left,
            move_right,
            move_up,
            move_down,
            move_word_left,
            move_word_right,
            move_line_start,
            move_line_end,
            delete_backward,
            delete_forward,
            delete_backward_word,
            delete_forward_word,
            kill_line_start,
            kill_whole_line,
            kill_line_end,
            yank,
        } = actual;
        Ok(Self {
            insert_newline: keys(insert_newline, "editor", "insert_newline"),
            move_left: keys(move_left, "editor", "move_left"),
            move_right: keys(move_right, "editor", "move_right"),
            move_up: keys(move_up, "editor", "move_up"),
            move_down: keys(move_down, "editor", "move_down"),
            move_word_left: keys(move_word_left, "editor", "move_word_left"),
            move_word_right: keys(move_word_right, "editor", "move_word_right"),
            move_line_start: keys(move_line_start, "editor", "move_line_start"),
            move_line_end: keys(move_line_end, "editor", "move_line_end"),
            delete_backward: keys(delete_backward, "editor", "delete_backward"),
            delete_forward: keys(delete_forward, "editor", "delete_forward"),
            delete_backward_word: keys(delete_backward_word, "editor", "delete_backward_word"),
            delete_forward_word: keys(delete_forward_word, "editor", "delete_forward_word"),
            kill_line_start: keys(kill_line_start, "editor", "kill_line_start"),
            kill_whole_line: keys(kill_whole_line, "editor", "kill_whole_line"),
            kill_line_end: keys(kill_line_end, "editor", "kill_line_end"),
            yank: keys(yank, "editor", "yank"),
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<EditorKeymap> {
        Ok(EditorKeymap {
            insert_newline: restore_keys(self.insert_newline, graph, "editor", "insert_newline")?,
            move_left: restore_keys(self.move_left, graph, "editor", "move_left")?,
            move_right: restore_keys(self.move_right, graph, "editor", "move_right")?,
            move_up: restore_keys(self.move_up, graph, "editor", "move_up")?,
            move_down: restore_keys(self.move_down, graph, "editor", "move_down")?,
            move_word_left: restore_keys(self.move_word_left, graph, "editor", "move_word_left")?,
            move_word_right: restore_keys(
                self.move_word_right,
                graph,
                "editor",
                "move_word_right",
            )?,
            move_line_start: restore_keys(
                self.move_line_start,
                graph,
                "editor",
                "move_line_start",
            )?,
            move_line_end: restore_keys(self.move_line_end, graph, "editor", "move_line_end")?,
            delete_backward: restore_keys(
                self.delete_backward,
                graph,
                "editor",
                "delete_backward",
            )?,
            delete_forward: restore_keys(self.delete_forward, graph, "editor", "delete_forward")?,
            delete_backward_word: restore_keys(
                self.delete_backward_word,
                graph,
                "editor",
                "delete_backward_word",
            )?,
            delete_forward_word: restore_keys(
                self.delete_forward_word,
                graph,
                "editor",
                "delete_forward_word",
            )?,
            kill_line_start: restore_keys(
                self.kill_line_start,
                graph,
                "editor",
                "kill_line_start",
            )?,
            kill_whole_line: restore_keys(
                self.kill_whole_line,
                graph,
                "editor",
                "kill_whole_line",
            )?,
            kill_line_end: restore_keys(self.kill_line_end, graph, "editor", "kill_line_end")?,
            yank: restore_keys(self.yank, graph, "editor", "yank")?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VimNormalKeymapData {
    enter_insert: Vec<BindingKey>,
    append_after_cursor: Vec<BindingKey>,
    append_line_end: Vec<BindingKey>,
    insert_line_start: Vec<BindingKey>,
    open_line_below: Vec<BindingKey>,
    open_line_above: Vec<BindingKey>,
    enter_replace_mode: Vec<BindingKey>,
    move_left: Vec<BindingKey>,
    move_right: Vec<BindingKey>,
    move_up: Vec<BindingKey>,
    move_down: Vec<BindingKey>,
    move_word_forward: Vec<BindingKey>,
    move_word_backward: Vec<BindingKey>,
    move_word_end: Vec<BindingKey>,
    move_line_start: Vec<BindingKey>,
    move_line_end: Vec<BindingKey>,
    find_forward: Vec<BindingKey>,
    find_backward: Vec<BindingKey>,
    till_forward: Vec<BindingKey>,
    till_backward: Vec<BindingKey>,
    jump_top: Vec<BindingKey>,
    jump_bottom: Vec<BindingKey>,
    delete_char: Vec<BindingKey>,
    replace_char: Vec<BindingKey>,
    repeat_last_change: Vec<BindingKey>,
    substitute_char: Vec<BindingKey>,
    delete_to_line_end: Vec<BindingKey>,
    change_to_line_end: Vec<BindingKey>,
    yank_line: Vec<BindingKey>,
    paste_after: Vec<BindingKey>,
    start_delete_operator: Vec<BindingKey>,
    start_yank_operator: Vec<BindingKey>,
    start_change_operator: Vec<BindingKey>,
    undo: Vec<BindingKey>,
    redo: Vec<BindingKey>,
    cancel_operator: Vec<BindingKey>,
}
impl VimNormalKeymapData {
    fn capture(actual: &VimNormalKeymap, _graph: &mut Capture) -> io::Result<Self> {
        let VimNormalKeymap {
            enter_insert,
            append_after_cursor,
            append_line_end,
            insert_line_start,
            open_line_below,
            open_line_above,
            enter_replace_mode,
            move_left,
            move_right,
            move_up,
            move_down,
            move_word_forward,
            move_word_backward,
            move_word_end,
            move_line_start,
            move_line_end,
            find_forward,
            find_backward,
            till_forward,
            till_backward,
            jump_top,
            jump_bottom,
            delete_char,
            replace_char,
            repeat_last_change,
            substitute_char,
            delete_to_line_end,
            change_to_line_end,
            yank_line,
            paste_after,
            start_delete_operator,
            start_yank_operator,
            start_change_operator,
            undo,
            redo,
            cancel_operator,
        } = actual;
        Ok(Self {
            enter_insert: keys(enter_insert, "vim_normal", "enter_insert"),
            append_after_cursor: keys(append_after_cursor, "vim_normal", "append_after_cursor"),
            append_line_end: keys(append_line_end, "vim_normal", "append_line_end"),
            insert_line_start: keys(insert_line_start, "vim_normal", "insert_line_start"),
            open_line_below: keys(open_line_below, "vim_normal", "open_line_below"),
            open_line_above: keys(open_line_above, "vim_normal", "open_line_above"),
            enter_replace_mode: keys(enter_replace_mode, "vim_normal", "enter_replace_mode"),
            move_left: keys(move_left, "vim_normal", "move_left"),
            move_right: keys(move_right, "vim_normal", "move_right"),
            move_up: keys(move_up, "vim_normal", "move_up"),
            move_down: keys(move_down, "vim_normal", "move_down"),
            move_word_forward: keys(move_word_forward, "vim_normal", "move_word_forward"),
            move_word_backward: keys(move_word_backward, "vim_normal", "move_word_backward"),
            move_word_end: keys(move_word_end, "vim_normal", "move_word_end"),
            move_line_start: keys(move_line_start, "vim_normal", "move_line_start"),
            move_line_end: keys(move_line_end, "vim_normal", "move_line_end"),
            find_forward: keys(find_forward, "vim_normal", "find_forward"),
            find_backward: keys(find_backward, "vim_normal", "find_backward"),
            till_forward: keys(till_forward, "vim_normal", "till_forward"),
            till_backward: keys(till_backward, "vim_normal", "till_backward"),
            jump_top: keys(jump_top, "vim_normal", "jump_top"),
            jump_bottom: keys(jump_bottom, "vim_normal", "jump_bottom"),
            delete_char: keys(delete_char, "vim_normal", "delete_char"),
            replace_char: keys(replace_char, "vim_normal", "replace_char"),
            repeat_last_change: keys(repeat_last_change, "vim_normal", "repeat_last_change"),
            substitute_char: keys(substitute_char, "vim_normal", "substitute_char"),
            delete_to_line_end: keys(delete_to_line_end, "vim_normal", "delete_to_line_end"),
            change_to_line_end: keys(change_to_line_end, "vim_normal", "change_to_line_end"),
            yank_line: keys(yank_line, "vim_normal", "yank_line"),
            paste_after: keys(paste_after, "vim_normal", "paste_after"),
            start_delete_operator: keys(
                start_delete_operator,
                "vim_normal",
                "start_delete_operator",
            ),
            start_yank_operator: keys(start_yank_operator, "vim_normal", "start_yank_operator"),
            start_change_operator: keys(
                start_change_operator,
                "vim_normal",
                "start_change_operator",
            ),
            undo: keys(undo, "vim_normal", "undo"),
            redo: keys(redo, "vim_normal", "redo"),
            cancel_operator: keys(cancel_operator, "vim_normal", "cancel_operator"),
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<VimNormalKeymap> {
        Ok(VimNormalKeymap {
            enter_insert: restore_keys(self.enter_insert, graph, "vim_normal", "enter_insert")?,
            append_after_cursor: restore_keys(
                self.append_after_cursor,
                graph,
                "vim_normal",
                "append_after_cursor",
            )?,
            append_line_end: restore_keys(
                self.append_line_end,
                graph,
                "vim_normal",
                "append_line_end",
            )?,
            insert_line_start: restore_keys(
                self.insert_line_start,
                graph,
                "vim_normal",
                "insert_line_start",
            )?,
            open_line_below: restore_keys(
                self.open_line_below,
                graph,
                "vim_normal",
                "open_line_below",
            )?,
            open_line_above: restore_keys(
                self.open_line_above,
                graph,
                "vim_normal",
                "open_line_above",
            )?,
            enter_replace_mode: restore_keys(
                self.enter_replace_mode,
                graph,
                "vim_normal",
                "enter_replace_mode",
            )?,
            move_left: restore_keys(self.move_left, graph, "vim_normal", "move_left")?,
            move_right: restore_keys(self.move_right, graph, "vim_normal", "move_right")?,
            move_up: restore_keys(self.move_up, graph, "vim_normal", "move_up")?,
            move_down: restore_keys(self.move_down, graph, "vim_normal", "move_down")?,
            move_word_forward: restore_keys(
                self.move_word_forward,
                graph,
                "vim_normal",
                "move_word_forward",
            )?,
            move_word_backward: restore_keys(
                self.move_word_backward,
                graph,
                "vim_normal",
                "move_word_backward",
            )?,
            move_word_end: restore_keys(self.move_word_end, graph, "vim_normal", "move_word_end")?,
            move_line_start: restore_keys(
                self.move_line_start,
                graph,
                "vim_normal",
                "move_line_start",
            )?,
            move_line_end: restore_keys(self.move_line_end, graph, "vim_normal", "move_line_end")?,
            find_forward: restore_keys(self.find_forward, graph, "vim_normal", "find_forward")?,
            find_backward: restore_keys(self.find_backward, graph, "vim_normal", "find_backward")?,
            till_forward: restore_keys(self.till_forward, graph, "vim_normal", "till_forward")?,
            till_backward: restore_keys(self.till_backward, graph, "vim_normal", "till_backward")?,
            jump_top: restore_keys(self.jump_top, graph, "vim_normal", "jump_top")?,
            jump_bottom: restore_keys(self.jump_bottom, graph, "vim_normal", "jump_bottom")?,
            delete_char: restore_keys(self.delete_char, graph, "vim_normal", "delete_char")?,
            replace_char: restore_keys(self.replace_char, graph, "vim_normal", "replace_char")?,
            repeat_last_change: restore_keys(
                self.repeat_last_change,
                graph,
                "vim_normal",
                "repeat_last_change",
            )?,
            substitute_char: restore_keys(
                self.substitute_char,
                graph,
                "vim_normal",
                "substitute_char",
            )?,
            delete_to_line_end: restore_keys(
                self.delete_to_line_end,
                graph,
                "vim_normal",
                "delete_to_line_end",
            )?,
            change_to_line_end: restore_keys(
                self.change_to_line_end,
                graph,
                "vim_normal",
                "change_to_line_end",
            )?,
            yank_line: restore_keys(self.yank_line, graph, "vim_normal", "yank_line")?,
            paste_after: restore_keys(self.paste_after, graph, "vim_normal", "paste_after")?,
            start_delete_operator: restore_keys(
                self.start_delete_operator,
                graph,
                "vim_normal",
                "start_delete_operator",
            )?,
            start_yank_operator: restore_keys(
                self.start_yank_operator,
                graph,
                "vim_normal",
                "start_yank_operator",
            )?,
            start_change_operator: restore_keys(
                self.start_change_operator,
                graph,
                "vim_normal",
                "start_change_operator",
            )?,
            undo: restore_keys(self.undo, graph, "vim_normal", "undo")?,
            redo: restore_keys(self.redo, graph, "vim_normal", "redo")?,
            cancel_operator: restore_keys(
                self.cancel_operator,
                graph,
                "vim_normal",
                "cancel_operator",
            )?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VimOperatorKeymapData {
    delete_line: Vec<BindingKey>,
    yank_line: Vec<BindingKey>,
    motion_left: Vec<BindingKey>,
    motion_right: Vec<BindingKey>,
    motion_up: Vec<BindingKey>,
    motion_down: Vec<BindingKey>,
    motion_word_forward: Vec<BindingKey>,
    motion_word_backward: Vec<BindingKey>,
    motion_word_end: Vec<BindingKey>,
    motion_line_start: Vec<BindingKey>,
    motion_line_end: Vec<BindingKey>,
    motion_find_forward: Vec<BindingKey>,
    motion_find_backward: Vec<BindingKey>,
    motion_till_forward: Vec<BindingKey>,
    motion_till_backward: Vec<BindingKey>,
    motion_jump_top: Vec<BindingKey>,
    motion_jump_bottom: Vec<BindingKey>,
    select_inner_text_object: Vec<BindingKey>,
    select_around_text_object: Vec<BindingKey>,
    cancel: Vec<BindingKey>,
}
impl VimOperatorKeymapData {
    fn capture(actual: &VimOperatorKeymap, _graph: &mut Capture) -> io::Result<Self> {
        let VimOperatorKeymap {
            delete_line,
            yank_line,
            motion_left,
            motion_right,
            motion_up,
            motion_down,
            motion_word_forward,
            motion_word_backward,
            motion_word_end,
            motion_line_start,
            motion_line_end,
            motion_find_forward,
            motion_find_backward,
            motion_till_forward,
            motion_till_backward,
            motion_jump_top,
            motion_jump_bottom,
            select_inner_text_object,
            select_around_text_object,
            cancel,
        } = actual;
        Ok(Self {
            delete_line: keys(delete_line, "vim_operator", "delete_line"),
            yank_line: keys(yank_line, "vim_operator", "yank_line"),
            motion_left: keys(motion_left, "vim_operator", "motion_left"),
            motion_right: keys(motion_right, "vim_operator", "motion_right"),
            motion_up: keys(motion_up, "vim_operator", "motion_up"),
            motion_down: keys(motion_down, "vim_operator", "motion_down"),
            motion_word_forward: keys(motion_word_forward, "vim_operator", "motion_word_forward"),
            motion_word_backward: keys(
                motion_word_backward,
                "vim_operator",
                "motion_word_backward",
            ),
            motion_word_end: keys(motion_word_end, "vim_operator", "motion_word_end"),
            motion_line_start: keys(motion_line_start, "vim_operator", "motion_line_start"),
            motion_line_end: keys(motion_line_end, "vim_operator", "motion_line_end"),
            motion_find_forward: keys(motion_find_forward, "vim_operator", "motion_find_forward"),
            motion_find_backward: keys(
                motion_find_backward,
                "vim_operator",
                "motion_find_backward",
            ),
            motion_till_forward: keys(motion_till_forward, "vim_operator", "motion_till_forward"),
            motion_till_backward: keys(
                motion_till_backward,
                "vim_operator",
                "motion_till_backward",
            ),
            motion_jump_top: keys(motion_jump_top, "vim_operator", "motion_jump_top"),
            motion_jump_bottom: keys(motion_jump_bottom, "vim_operator", "motion_jump_bottom"),
            select_inner_text_object: keys(
                select_inner_text_object,
                "vim_operator",
                "select_inner_text_object",
            ),
            select_around_text_object: keys(
                select_around_text_object,
                "vim_operator",
                "select_around_text_object",
            ),
            cancel: keys(cancel, "vim_operator", "cancel"),
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<VimOperatorKeymap> {
        Ok(VimOperatorKeymap {
            delete_line: restore_keys(self.delete_line, graph, "vim_operator", "delete_line")?,
            yank_line: restore_keys(self.yank_line, graph, "vim_operator", "yank_line")?,
            motion_left: restore_keys(self.motion_left, graph, "vim_operator", "motion_left")?,
            motion_right: restore_keys(self.motion_right, graph, "vim_operator", "motion_right")?,
            motion_up: restore_keys(self.motion_up, graph, "vim_operator", "motion_up")?,
            motion_down: restore_keys(self.motion_down, graph, "vim_operator", "motion_down")?,
            motion_word_forward: restore_keys(
                self.motion_word_forward,
                graph,
                "vim_operator",
                "motion_word_forward",
            )?,
            motion_word_backward: restore_keys(
                self.motion_word_backward,
                graph,
                "vim_operator",
                "motion_word_backward",
            )?,
            motion_word_end: restore_keys(
                self.motion_word_end,
                graph,
                "vim_operator",
                "motion_word_end",
            )?,
            motion_line_start: restore_keys(
                self.motion_line_start,
                graph,
                "vim_operator",
                "motion_line_start",
            )?,
            motion_line_end: restore_keys(
                self.motion_line_end,
                graph,
                "vim_operator",
                "motion_line_end",
            )?,
            motion_find_forward: restore_keys(
                self.motion_find_forward,
                graph,
                "vim_operator",
                "motion_find_forward",
            )?,
            motion_find_backward: restore_keys(
                self.motion_find_backward,
                graph,
                "vim_operator",
                "motion_find_backward",
            )?,
            motion_till_forward: restore_keys(
                self.motion_till_forward,
                graph,
                "vim_operator",
                "motion_till_forward",
            )?,
            motion_till_backward: restore_keys(
                self.motion_till_backward,
                graph,
                "vim_operator",
                "motion_till_backward",
            )?,
            motion_jump_top: restore_keys(
                self.motion_jump_top,
                graph,
                "vim_operator",
                "motion_jump_top",
            )?,
            motion_jump_bottom: restore_keys(
                self.motion_jump_bottom,
                graph,
                "vim_operator",
                "motion_jump_bottom",
            )?,
            select_inner_text_object: restore_keys(
                self.select_inner_text_object,
                graph,
                "vim_operator",
                "select_inner_text_object",
            )?,
            select_around_text_object: restore_keys(
                self.select_around_text_object,
                graph,
                "vim_operator",
                "select_around_text_object",
            )?,
            cancel: restore_keys(self.cancel, graph, "vim_operator", "cancel")?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VimSearchKeymapData {
    forward: Vec<BindingKey>,
    backward: Vec<BindingKey>,
    next: Vec<BindingKey>,
    previous: Vec<BindingKey>,
}
impl VimSearchKeymapData {
    fn capture(actual: &VimSearchKeymap, _graph: &mut Capture) -> io::Result<Self> {
        let VimSearchKeymap {
            forward,
            backward,
            next,
            previous,
        } = actual;
        Ok(Self {
            forward: keys(forward, "vim_search", "forward"),
            backward: keys(backward, "vim_search", "backward"),
            next: keys(next, "vim_search", "next"),
            previous: keys(previous, "vim_search", "previous"),
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<VimSearchKeymap> {
        Ok(VimSearchKeymap {
            forward: restore_keys(self.forward, graph, "vim_search", "forward")?,
            backward: restore_keys(self.backward, graph, "vim_search", "backward")?,
            next: restore_keys(self.next, graph, "vim_search", "next")?,
            previous: restore_keys(self.previous, graph, "vim_search", "previous")?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VimTextObjectKeymapData {
    word: Vec<BindingKey>,
    big_word: Vec<BindingKey>,
    parentheses: Vec<BindingKey>,
    brackets: Vec<BindingKey>,
    braces: Vec<BindingKey>,
    double_quote: Vec<BindingKey>,
    single_quote: Vec<BindingKey>,
    backtick: Vec<BindingKey>,
    cancel: Vec<BindingKey>,
}
impl VimTextObjectKeymapData {
    fn capture(actual: &VimTextObjectKeymap, _graph: &mut Capture) -> io::Result<Self> {
        let VimTextObjectKeymap {
            word,
            big_word,
            parentheses,
            brackets,
            braces,
            double_quote,
            single_quote,
            backtick,
            cancel,
        } = actual;
        Ok(Self {
            word: keys(word, "vim_text_object", "word"),
            big_word: keys(big_word, "vim_text_object", "big_word"),
            parentheses: keys(parentheses, "vim_text_object", "parentheses"),
            brackets: keys(brackets, "vim_text_object", "brackets"),
            braces: keys(braces, "vim_text_object", "braces"),
            double_quote: keys(double_quote, "vim_text_object", "double_quote"),
            single_quote: keys(single_quote, "vim_text_object", "single_quote"),
            backtick: keys(backtick, "vim_text_object", "backtick"),
            cancel: keys(cancel, "vim_text_object", "cancel"),
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<VimTextObjectKeymap> {
        Ok(VimTextObjectKeymap {
            word: restore_keys(self.word, graph, "vim_text_object", "word")?,
            big_word: restore_keys(self.big_word, graph, "vim_text_object", "big_word")?,
            parentheses: restore_keys(self.parentheses, graph, "vim_text_object", "parentheses")?,
            brackets: restore_keys(self.brackets, graph, "vim_text_object", "brackets")?,
            braces: restore_keys(self.braces, graph, "vim_text_object", "braces")?,
            double_quote: restore_keys(
                self.double_quote,
                graph,
                "vim_text_object",
                "double_quote",
            )?,
            single_quote: restore_keys(
                self.single_quote,
                graph,
                "vim_text_object",
                "single_quote",
            )?,
            backtick: restore_keys(self.backtick, graph, "vim_text_object", "backtick")?,
            cancel: restore_keys(self.cancel, graph, "vim_text_object", "cancel")?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PagerKeymapData {
    scroll_up: Vec<BindingKey>,
    scroll_down: Vec<BindingKey>,
    page_up: Vec<BindingKey>,
    page_down: Vec<BindingKey>,
    half_page_up: Vec<BindingKey>,
    half_page_down: Vec<BindingKey>,
    jump_top: Vec<BindingKey>,
    jump_bottom: Vec<BindingKey>,
    close: Vec<BindingKey>,
    close_transcript: Vec<BindingKey>,
    find: Vec<BindingKey>,
    chord_hints: ChordRef,
}
impl PagerKeymapData {
    pub(crate) fn capture(actual: &PagerKeymap, graph: &mut Capture) -> io::Result<Self> {
        let PagerKeymap {
            scroll_up,
            scroll_down,
            page_up,
            page_down,
            half_page_up,
            half_page_down,
            jump_top,
            jump_bottom,
            close,
            close_transcript,
            find,
            chord_hints,
        } = actual;
        Ok(Self {
            scroll_up: keys(scroll_up, "pager", "scroll_up"),
            scroll_down: keys(scroll_down, "pager", "scroll_down"),
            page_up: keys(page_up, "pager", "page_up"),
            page_down: keys(page_down, "pager", "page_down"),
            half_page_up: keys(half_page_up, "pager", "half_page_up"),
            half_page_down: keys(half_page_down, "pager", "half_page_down"),
            jump_top: keys(jump_top, "pager", "jump_top"),
            jump_bottom: keys(jump_bottom, "pager", "jump_bottom"),
            close: keys(close, "pager", "close"),
            close_transcript: keys(close_transcript, "pager", "close_transcript"),
            find: keys(find, "pager", "find"),
            chord_hints: graph.chords(chord_hints)?,
        })
    }
    pub(crate) fn restore(self, graph: &Restored) -> io::Result<PagerKeymap> {
        Ok(PagerKeymap {
            scroll_up: restore_keys(self.scroll_up, graph, "pager", "scroll_up")?,
            scroll_down: restore_keys(self.scroll_down, graph, "pager", "scroll_down")?,
            page_up: restore_keys(self.page_up, graph, "pager", "page_up")?,
            page_down: restore_keys(self.page_down, graph, "pager", "page_down")?,
            half_page_up: restore_keys(self.half_page_up, graph, "pager", "half_page_up")?,
            half_page_down: restore_keys(self.half_page_down, graph, "pager", "half_page_down")?,
            jump_top: restore_keys(self.jump_top, graph, "pager", "jump_top")?,
            jump_bottom: restore_keys(self.jump_bottom, graph, "pager", "jump_bottom")?,
            close: restore_keys(self.close, graph, "pager", "close")?,
            close_transcript: restore_keys(
                self.close_transcript,
                graph,
                "pager",
                "close_transcript",
            )?,
            find: restore_keys(self.find, graph, "pager", "find")?,
            chord_hints: graph.chords(self.chord_hints)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListKeymapData {
    move_up: Vec<BindingKey>,
    move_down: Vec<BindingKey>,
    move_left: Vec<BindingKey>,
    move_right: Vec<BindingKey>,
    page_up: Vec<BindingKey>,
    page_down: Vec<BindingKey>,
    jump_top: Vec<BindingKey>,
    jump_bottom: Vec<BindingKey>,
    accept: Vec<BindingKey>,
    cancel: Vec<BindingKey>,
    chord_hints: ChordRef,
}
impl ListKeymapData {
    fn capture(actual: &ListKeymap, graph: &mut Capture) -> io::Result<Self> {
        let ListKeymap {
            move_up,
            move_down,
            move_left,
            move_right,
            page_up,
            page_down,
            jump_top,
            jump_bottom,
            accept,
            cancel,
            chord_hints,
        } = actual;
        Ok(Self {
            move_up: keys(move_up, "list", "move_up"),
            move_down: keys(move_down, "list", "move_down"),
            move_left: keys(move_left, "list", "move_left"),
            move_right: keys(move_right, "list", "move_right"),
            page_up: keys(page_up, "list", "page_up"),
            page_down: keys(page_down, "list", "page_down"),
            jump_top: keys(jump_top, "list", "jump_top"),
            jump_bottom: keys(jump_bottom, "list", "jump_bottom"),
            accept: keys(accept, "list", "accept"),
            cancel: keys(cancel, "list", "cancel"),
            chord_hints: graph.chords(chord_hints)?,
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<ListKeymap> {
        Ok(ListKeymap {
            move_up: restore_keys(self.move_up, graph, "list", "move_up")?,
            move_down: restore_keys(self.move_down, graph, "list", "move_down")?,
            move_left: restore_keys(self.move_left, graph, "list", "move_left")?,
            move_right: restore_keys(self.move_right, graph, "list", "move_right")?,
            page_up: restore_keys(self.page_up, graph, "list", "page_up")?,
            page_down: restore_keys(self.page_down, graph, "list", "page_down")?,
            jump_top: restore_keys(self.jump_top, graph, "list", "jump_top")?,
            jump_bottom: restore_keys(self.jump_bottom, graph, "list", "jump_bottom")?,
            accept: restore_keys(self.accept, graph, "list", "accept")?,
            cancel: restore_keys(self.cancel, graph, "list", "cancel")?,
            chord_hints: graph.chords(self.chord_hints)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentsKeymapData {
    resume: Vec<BindingKey>,
    search: Vec<BindingKey>,
    new_task: Vec<BindingKey>,
    new_worktree: Vec<BindingKey>,
    fork: Vec<BindingKey>,
    rename: Vec<BindingKey>,
    stop: Vec<BindingKey>,
    archive: Vec<BindingKey>,
    delete: Vec<BindingKey>,
    hide: Vec<BindingKey>,
    toggle_pin: Vec<BindingKey>,
    toggle_grouping: Vec<BindingKey>,
    chord_hints: ChordRef,
}
impl AgentsKeymapData {
    fn capture(actual: &AgentsKeymap, graph: &mut Capture) -> io::Result<Self> {
        let AgentsKeymap {
            resume,
            search,
            new_task,
            new_worktree,
            fork,
            rename,
            stop,
            archive,
            delete,
            hide,
            toggle_pin,
            toggle_grouping,
            chord_hints,
        } = actual;
        Ok(Self {
            resume: keys(resume, "agents", "resume"),
            search: keys(search, "agents", "search"),
            new_task: keys(new_task, "agents", "new_task"),
            new_worktree: keys(new_worktree, "agents", "new_worktree"),
            fork: keys(fork, "agents", "fork"),
            rename: keys(rename, "agents", "rename"),
            stop: keys(stop, "agents", "stop"),
            archive: keys(archive, "agents", "archive"),
            delete: keys(delete, "agents", "delete"),
            hide: keys(hide, "agents", "hide"),
            toggle_pin: keys(toggle_pin, "agents", "toggle_pin"),
            toggle_grouping: keys(toggle_grouping, "agents", "toggle_grouping"),
            chord_hints: graph.chords(chord_hints)?,
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<AgentsKeymap> {
        Ok(AgentsKeymap {
            resume: restore_keys(self.resume, graph, "agents", "resume")?,
            search: restore_keys(self.search, graph, "agents", "search")?,
            new_task: restore_keys(self.new_task, graph, "agents", "new_task")?,
            new_worktree: restore_keys(self.new_worktree, graph, "agents", "new_worktree")?,
            fork: restore_keys(self.fork, graph, "agents", "fork")?,
            rename: restore_keys(self.rename, graph, "agents", "rename")?,
            stop: restore_keys(self.stop, graph, "agents", "stop")?,
            archive: restore_keys(self.archive, graph, "agents", "archive")?,
            delete: restore_keys(self.delete, graph, "agents", "delete")?,
            hide: restore_keys(self.hide, graph, "agents", "hide")?,
            toggle_pin: restore_keys(self.toggle_pin, graph, "agents", "toggle_pin")?,
            toggle_grouping: restore_keys(
                self.toggle_grouping,
                graph,
                "agents",
                "toggle_grouping",
            )?,
            chord_hints: graph.chords(self.chord_hints)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalKeymapData {
    open_fullscreen: Vec<BindingKey>,
    open_thread: Vec<BindingKey>,
    approve: Vec<BindingKey>,
    approve_for_session: Vec<BindingKey>,
    approve_for_prefix: Vec<BindingKey>,
    deny: Vec<BindingKey>,
    decline: Vec<BindingKey>,
    cancel: Vec<BindingKey>,
    chord_hints: ChordRef,
}
impl ApprovalKeymapData {
    fn capture(actual: &ApprovalKeymap, graph: &mut Capture) -> io::Result<Self> {
        let ApprovalKeymap {
            open_fullscreen,
            open_thread,
            approve,
            approve_for_session,
            approve_for_prefix,
            deny,
            decline,
            cancel,
            chord_hints,
        } = actual;
        Ok(Self {
            open_fullscreen: keys(open_fullscreen, "approval", "open_fullscreen"),
            open_thread: keys(open_thread, "approval", "open_thread"),
            approve: keys(approve, "approval", "approve"),
            approve_for_session: keys(approve_for_session, "approval", "approve_for_session"),
            approve_for_prefix: keys(approve_for_prefix, "approval", "approve_for_prefix"),
            deny: keys(deny, "approval", "deny"),
            decline: keys(decline, "approval", "decline"),
            cancel: keys(cancel, "approval", "cancel"),
            chord_hints: graph.chords(chord_hints)?,
        })
    }
    fn restore(self, graph: &Restored) -> io::Result<ApprovalKeymap> {
        Ok(ApprovalKeymap {
            open_fullscreen: restore_keys(
                self.open_fullscreen,
                graph,
                "approval",
                "open_fullscreen",
            )?,
            open_thread: restore_keys(self.open_thread, graph, "approval", "open_thread")?,
            approve: restore_keys(self.approve, graph, "approval", "approve")?,
            approve_for_session: restore_keys(
                self.approve_for_session,
                graph,
                "approval",
                "approve_for_session",
            )?,
            approve_for_prefix: restore_keys(
                self.approve_for_prefix,
                graph,
                "approval",
                "approve_for_prefix",
            )?,
            deny: restore_keys(self.deny, graph, "approval", "deny")?,
            decline: restore_keys(self.decline, graph, "approval", "decline")?,
            cancel: restore_keys(self.cancel, graph, "approval", "cancel")?,
            chord_hints: graph.chords(self.chord_hints)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    app: AppKeymapData,
    chords: ChordRef,
    chat: ChatKeymapData,
    composer: ComposerKeymapData,
    editor: EditorRef,
    vim_normal: VimNormalKeymapData,
    vim_operator: VimOperatorKeymapData,
    vim_search: VimSearchKeymapData,
    vim_text_object: VimTextObjectKeymapData,
    pager: PagerKeymapData,
    list: ListKeymapData,
    agents: AgentsKeymapData,
    approval: ApprovalKeymapData,
}
impl Data {
    pub(crate) fn capture(actual: &RuntimeKeymap, graph: &mut Capture) -> io::Result<Self> {
        let RuntimeKeymap {
            app,
            chords,
            chat,
            composer,
            editor,
            vim_normal,
            vim_operator,
            vim_search,
            vim_text_object,
            pager,
            list,
            agents,
            approval,
        } = actual;
        Ok(Self {
            app: AppKeymapData::capture(app, graph)?,
            chords: graph.chords(chords)?,
            chat: ChatKeymapData::capture(chat, graph)?,
            composer: ComposerKeymapData::capture(composer, graph)?,
            editor: graph.editor(editor)?,
            vim_normal: VimNormalKeymapData::capture(vim_normal, graph)?,
            vim_operator: VimOperatorKeymapData::capture(vim_operator, graph)?,
            vim_search: VimSearchKeymapData::capture(vim_search, graph)?,
            vim_text_object: VimTextObjectKeymapData::capture(vim_text_object, graph)?,
            pager: PagerKeymapData::capture(pager, graph)?,
            list: ListKeymapData::capture(list, graph)?,
            agents: AgentsKeymapData::capture(agents, graph)?,
            approval: ApprovalKeymapData::capture(approval, graph)?,
        })
    }
    pub(crate) fn restore(self, graph: &Restored) -> io::Result<RuntimeKeymap> {
        Ok(RuntimeKeymap {
            app: self.app.restore(graph)?,
            chords: graph.chords(self.chords)?,
            chat: self.chat.restore(graph)?,
            composer: self.composer.restore(graph)?,
            editor: graph.editor(self.editor)?,
            vim_normal: self.vim_normal.restore(graph)?,
            vim_operator: self.vim_operator.restore(graph)?,
            vim_search: self.vim_search.restore(graph)?,
            vim_text_object: self.vim_text_object.restore(graph)?,
            pager: self.pager.restore(graph)?,
            list: self.list.restore(graph)?,
            agents: self.agents.restore(graph)?,
            approval: self.approval.restore(graph)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tables {
    schema: u8,
    inventory: chords::alab_retained::Inventory,
    editors: Vec<EditorKeymapData>,
    chords: Vec<chords::alab_retained::Data>,
}
pub(crate) struct Capture {
    editors: Identities<EditorKeymap>,
    chords: Identities<RuntimeChordKeymap>,
}
impl Default for Capture {
    fn default() -> Self {
        Self {
            editors: Identities::new(MAX_NODES),
            chords: Identities::new(MAX_NODES),
        }
    }
}
impl Capture {
    pub(crate) fn editor(&mut self, actual: &Arc<EditorKeymap>) -> io::Result<EditorRef> {
        self.editors
            .intern(actual)
            .map(|(id, _)| EditorRef(id))
            .map_err(invalid)
    }
    pub(crate) fn chords(&mut self, actual: &Arc<RuntimeChordKeymap>) -> io::Result<ChordRef> {
        self.chords
            .intern(actual)
            .map(|(id, _)| ChordRef(id))
            .map_err(invalid)
    }
    pub(crate) fn export(&mut self) -> io::Result<Tables> {
        // Clone genuine pinned Arc handles before borrowing this same table mutably.
        let editors = (0..self.editors.len())
            .map(|i| self.editors.get(Id(i)))
            .collect::<io::Result<Vec<_>>>()?;
        let chords = (0..self.chords.len())
            .map(|i| self.chords.get(Id(i)))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Tables {
            schema: 2,
            inventory: chords::alab_retained::Inventory::capture()?,
            editors: editors
                .iter()
                .map(|v| EditorKeymapData::capture(v, self))
                .collect::<io::Result<_>>()?,
            chords: chords
                .iter()
                .map(|v| chords::alab_retained::Data::capture(v))
                .collect(),
        })
    }
}
pub(crate) struct Restored {
    inventory: chords::alab_retained::Inventory,
    editors: Vec<Arc<EditorKeymap>>,
    chords: Vec<Arc<RuntimeChordKeymap>>,
}
impl Restored {
    pub(crate) fn new(data: Tables) -> io::Result<Self> {
        if data.schema != 2 || data.editors.len() > MAX_NODES || data.chords.len() > MAX_NODES {
            return Err(invalid("native keymap schema/capacity"));
        }
        data.inventory.validate()?;
        let mut actual = Self {
            inventory: data.inventory,
            editors: Vec::new(),
            chords: data
                .chords
                .into_iter()
                .map(|v| v.restore().map(Arc::new))
                .collect::<io::Result<_>>()?,
        };
        actual.editors = data
            .editors
            .into_iter()
            .map(|v| v.restore(&actual).map(Arc::new))
            .collect::<io::Result<_>>()?;
        Ok(actual)
    }
    pub(crate) fn editor(&self, id: EditorRef) -> io::Result<Arc<EditorKeymap>> {
        self.editors
            .get(id.0.0)
            .cloned()
            .ok_or_else(|| invalid("native editor keymap reference missing"))
    }
    pub(crate) fn chords(&self, id: ChordRef) -> io::Result<Arc<RuntimeChordKeymap>> {
        self.chords
            .get(id.0.0)
            .cloned()
            .ok_or_else(|| invalid("native chord keymap reference missing"))
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    use codex_config::types::KeybindingSpec;
    use codex_config::types::KeybindingsSpec;
    fn original() -> io::Result<RuntimeKeymap> {
        let mut config = codex_config::types::TuiKeymap::default();
        config.global.leader = Some(KeybindingsSpec::One(KeybindingSpec("super-k".into())));
        config.global.open_transcript = Some(KeybindingsSpec::One(KeybindingSpec(
            "leader super-t".into(),
        )));
        config.global.copy = Some(KeybindingsSpec::One(KeybindingSpec(
            "super-o super-c".into(),
        )));
        RuntimeKeymap::from_config(&config).map_err(invalid)
    }
    #[test]
    fn retained_autopilot_keymap_restores_custom_and_legacy_bindings() -> io::Result<()> {
        for configured in [false, true] {
            let source = if configured {
                "[chat]\nopen_autopilot = ['f19', 'f20 a']"
            } else {
                "[chat]\nopen_presets = 'f20 p'"
            };
            let config = toml::from_str(source).map_err(invalid)?;
            let original = RuntimeKeymap::from_config(&config).map_err(invalid)?;
            let mut capture = Capture::default();
            let data = Data::capture(&original, &mut capture)?;
            let mut wire = serde_json::to_value((data, capture.export()?)).map_err(invalid)?;
            if !configured {
                // The previous runtime had neither the binding field nor its dispatch token.
                assert!(wire[0]["chat"].get("open_autopilot").is_none());
                let entries = wire[1]["inventory"]["entries"].as_array_mut().unwrap();
                assert_eq!(entries.last().unwrap()["action"]["name"], "open_autopilot");
                entries.pop();
            }
            let (data, tables): (Data, Tables) = serde_json::from_value(wire).map_err(invalid)?;
            let restored = data.restore(&Restored::new(tables)?)?;
            assert_eq!(restored.chat.open_autopilot, original.chat.open_autopilot);
            assert_eq!(restored.chat.open_presets, original.chat.open_presets);
            assert_eq!(
                restored.primary_hint(KeymapContext::Chat, "open_autopilot"),
                original.primary_hint(KeymapContext::Chat, "open_autopilot")
            );
        }
        Ok(())
    }

    #[test]
    fn actual_resolved_native_keymap_reuses_raw_dispatch_tokens_and_original_chord_reducer()
    -> io::Result<()> {
        let original = original()?;
        assert!(
            original
                .app
                .open_transcript
                .iter()
                .copied()
                .any(chords::is_dispatch_token)
        );
        let mut capture = Capture::default();
        let data = Data::capture(&original, &mut capture)?;
        let bytes = serde_json::to_vec(&(data, capture.export()?)).map_err(invalid)?;
        let (data, tables): (Data, Tables) = serde_json::from_slice(&bytes).map_err(invalid)?;
        let graph = Restored::new(tables)?;
        let restored = data.restore(&graph)?;
        let mut original_graph = Capture::default();
        let mut restored_graph = Capture::default();
        assert_eq!(
            serde_json::to_value(Data::capture(&original, &mut original_graph)?)
                .map_err(invalid)?,
            serde_json::to_value(Data::capture(&restored, &mut restored_graph)?)
                .map_err(invalid)?
        );
        let mut a = KeyChordMatcher::default();
        let mut b = KeyChordMatcher::default();
        for event in [
            crossterm::event::KeyEvent::new(KeyCode::Char('k'), KeyModifiers::SUPER),
            crossterm::event::KeyEvent::new(KeyCode::Char('t'), KeyModifiers::SUPER),
        ] {
            assert_eq!(
                a.advance(event, &original.chords, KeymapContextSet::browsing()),
                b.advance(event, &restored.chords, KeymapContextSet::browsing())
            );
        }
        assert!(!a.is_pending() && !b.is_pending());
        Ok(())
    }
    #[test]
    fn global_keymap_table_preserves_shared_and_equal_distinct_editor_chord_arcs() -> io::Result<()>
    {
        let first = original()?;
        let same = first.clone();
        let mut distinct = first.clone();
        distinct.editor = Arc::new((*first.editor).clone());
        distinct.chords = Arc::new((*first.chords).clone());
        let chord_aliases = |keymap: &RuntimeKeymap| {
            [
                keymap.chords.clone(),
                keymap.chat.chord_hints.clone(),
                keymap.pager.chord_hints.clone(),
                keymap.list.chord_hints.clone(),
                keymap.agents.chord_hints.clone(),
                keymap.approval.chord_hints.clone(),
            ]
        };
        let original_aliases = [&first, &same, &distinct]
            .into_iter()
            .flat_map(chord_aliases)
            .collect::<Vec<_>>();
        let mut capture = Capture::default();
        let a = Data::capture(&first, &mut capture)?;
        let b = Data::capture(&same, &mut capture)?;
        let c = Data::capture(&distinct, &mut capture)?;
        let graph = Restored::new(capture.export()?)?;
        let a = a.restore(&graph)?;
        let b = b.restore(&graph)?;
        let c = c.restore(&graph)?;
        assert!(Arc::ptr_eq(&a.editor, &b.editor));
        assert!(!Arc::ptr_eq(&a.editor, &c.editor));
        assert!(Arc::ptr_eq(&a.chords, &b.chords));
        assert!(!Arc::ptr_eq(&a.chords, &c.chords));
        // Native leader registration may copy on write and only rebind Chat.
        // Preserve the actual complete alias matrix, including old pager/list
        // nodes and equal distinct objects, rather than assuming one allocation.
        let restored_aliases = [&a, &b, &c]
            .into_iter()
            .flat_map(chord_aliases)
            .collect::<Vec<_>>();
        for i in 0..original_aliases.len() {
            for j in 0..original_aliases.len() {
                assert_eq!(
                    Arc::ptr_eq(&original_aliases[i], &original_aliases[j]),
                    Arc::ptr_eq(&restored_aliases[i], &restored_aliases[j]),
                    "native chord alias {i}/{j}"
                );
            }
        }
        let mut capture = Capture::default();
        let mut bad = Data::capture(&first, &mut capture)?;
        bad.editor = EditorRef(Id(usize::MAX));
        assert!(bad.restore(&Restored::new(capture.export()?)?).is_err());
        Ok(())
    }
}

#[cfg(test)]
mod dispatch_controls {
    use super::*;
    use codex_config::types::KeybindingSpec;
    use codex_config::types::KeybindingsSpec;
    #[test]
    fn genuine_internal_occurrences_rebind_after_captured_inventory_reorder_and_keep_physical_keys()
    -> io::Result<()> {
        let mut config = codex_config::types::TuiKeymap::default();
        config.global.open_transcript = Some(KeybindingsSpec::One(KeybindingSpec(
            "super-k super-t".into(),
        )));
        config.global.copy = Some(KeybindingsSpec::One(KeybindingSpec(
            "super-o super-c".into(),
        )));
        let original = RuntimeKeymap::from_config(&config).map_err(invalid)?;
        let a = keymap_action_id("global", "open_transcript")
            .ok_or_else(|| invalid("actual transcript action missing"))?;
        let b = keymap_action_id("global", "copy")
            .ok_or_else(|| invalid("actual copy action missing"))?;
        let mut capture = Capture::default();
        let mut data = Data::capture(&original, &mut capture)?;
        let mut table = capture.export()?;
        // Model an actual prior action-order layout in captured DATA, not a fake Native role.
        table.inventory.test_reorder(a, b)?;
        table.inventory.test_retag(&mut data.app.open_transcript)?;
        table.inventory.test_retag(&mut data.app.copy)?;
        let native = Data::capture(&original, &mut Capture::default())?;
        assert_ne!(
            serde_json::to_value(&data.app.open_transcript).map_err(invalid)?,
            serde_json::to_value(&native.app.open_transcript).map_err(invalid)?
        );
        let restored = data.restore(&Restored::new(table)?)?;
        assert_eq!(restored.app.open_transcript, original.app.open_transcript);
        assert_eq!(restored.app.copy, original.app.copy);
        // Untagged physical bindings retain exact order and duplicates, even unusual native codes.
        let physical = vec![
            KeyBinding::new(KeyCode::BackTab, KeyModifiers::SHIFT),
            KeyBinding::new(KeyCode::Char('界'), KeyModifiers::ALT),
            KeyBinding::new(KeyCode::Char('界'), KeyModifiers::ALT),
        ];
        let mut capture = Capture::default();
        let data = BindingsData::capture(&physical, a)?;
        let graph = Restored::new(capture.export()?)?;
        assert_eq!(data.restore(&graph, a)?, physical);
        let wrong = BindingsData::capture(&original.app.open_transcript, a)?;
        // A genuine token associated with A cannot be installed into B's native field.
        assert!(
            wrong
                .0
                .iter()
                .any(|v| matches!(v, BindingKey::Dispatch { .. }))
        );
        assert!(wrong.restore(&graph, b).is_err());
        Ok(())
    }
    #[test]
    fn actual_inventory_duplicates_and_changed_source_token_are_refused() -> io::Result<()> {
        let original = RuntimeKeymap::defaults();
        let action = keymap_action_id("global", "open_transcript")
            .ok_or_else(|| invalid("native action missing"))?;
        let token = crate::keymap::dispatch_event(action)
            .ok_or_else(|| invalid("actual native dispatch missing"))?;
        let values = vec![KeyBinding::from_event(token)];
        let mut capture = Capture::default();
        let data = BindingsData::capture(&values, action)?;
        let mut table = capture.export()?;
        table.inventory.test_reorder(
            action,
            keymap_action_id("global", "copy")
                .ok_or_else(|| invalid("native copy action missing"))?,
        )?;
        let graph = Restored::new(table)?;
        assert!(data.restore(&graph, action).is_err());
        // The original typed map remains usable after that pure refusal.
        let mut capture = Capture::default();
        let data = Data::capture(&original, &mut capture)?;
        assert!(data.restore(&Restored::new(capture.export()?)?).is_ok());
        Ok(())
    }
}
