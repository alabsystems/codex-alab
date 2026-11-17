// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native editor data only; these projections neither commit nor restore a UI.
use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_recorded_yank_keeps_generation_identity_during_replay() {
        use crate::bottom_pane::textarea::TextElementSnapshot;
        use codex_alab_tui::upgrade_editor_graph_scope as graph;
        use std::sync::Arc;
        use std::sync::Weak;

        #[derive(serde::Serialize, serde::Deserialize)]
        struct Recorded {
            #[serde(with = "graph::weak")]
            payload: Weak<Vec<TextElementSnapshot>>,
            command: VimAction,
        }
        let mut source = TextArea::new();
        source.set_text_clearing_elements("[image]");
        source.add_element_range(0..7).unwrap();
        source.set_vim_enabled(true);
        for key in ['y', '$'] {
            source.input(KeyCode::Char(key).into());
        }
        let snapshot = source.register_snapshot();
        let payload = Arc::downgrade(&snapshot.elements);
        let recorded = Recorded {
            payload,
            command: VimAction::Yank(Arc::new(snapshot)),
        };
        let bytes = graph::encode::<Vec<TextElementSnapshot>>(&recorded, 8192).unwrap();
        let restored: Recorded =
            graph::decode::<Vec<TextElementSnapshot>, _>(&bytes, 8192, 1).unwrap();
        let VimAction::Yank(snapshot) = &restored.command else {
            panic!("recorded yank lost");
        };
        assert!(Weak::ptr_eq(
            &restored.payload,
            &Arc::downgrade(&snapshot.elements)
        ));
        // Use the real semantic replay path; rich elements get fresh IDs on paste.
        let mut destination = TextArea::new();
        destination.set_vim_enabled(true);
        destination.apply_vim_editor_action(restored.command);
        assert_eq!(destination.text(), source.kill_buffer);
        assert_eq!(destination.text_element_snapshots().len(), 1);
        let Some(super::super::super::register::RegisterEvent::Pasted(elements, ids)) =
            destination.take_register_event()
        else {
            panic!("native replay did not emit rich paste event");
        };
        assert!(Weak::ptr_eq(&restored.payload, &Arc::downgrade(&elements)));
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn native_replace_recovery_and_dot_replay_survive_command_data_roundtrip() {
        let mut source = TextArea::new();
        source.set_text_clearing_elements("abcdefghij");
        source.set_vim_enabled(true);
        for key in ['R', 'x', 'y'] {
            source.input(KeyCode::Char(key).into());
        }
        assert!(source.is_vim_replace_mode());
        let bytes = serde_json::to_vec(&source.vim_commands).unwrap();
        let mut destination = TextArea::new();
        destination.set_text_clearing_elements(source.text());
        destination.set_vim_enabled(true);
        destination.set_cursor(source.cursor());
        destination.vim_mode = source.vim_mode;
        destination.vim_commands = serde_json::from_slice(&bytes).unwrap();
        for key in [KeyCode::Backspace, KeyCode::Esc, KeyCode::Char('.')] {
            source.input(key.into());
            destination.input(key.into());
            assert_eq!(destination.text(), source.text());
            assert_eq!(destination.cursor(), source.cursor());
            assert_eq!(destination.vim_mode, source.vim_mode);
            assert_eq!(
                serde_json::to_value(&destination.vim_commands).unwrap(),
                serde_json::to_value(&source.vim_commands).unwrap(),
            );
        }
    }
}

crate::alab_editor_upgrade::native_state!(
    super::VimEdit,
    VimEditWire,
    #[serde(remote = "super::VimEdit", deny_unknown_fields)]
    enum VimEditWire {
        Editor(VimEditorEdit),
        Text(String),
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimPersistentState,
    VimPersistentStateWire,
    #[serde(remote = "super::VimPersistentState", deny_unknown_fields)]
    struct VimPersistentStateWire {
        commands: VimCommandState,
        search: crate::vim_search::SearchQuery,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimEditorEdit,
    VimEditorEditWire,
    #[serde(remote = "super::VimEditorEdit", deny_unknown_fields)]
    struct VimEditorEditWire(VimAction);
);

crate::alab_editor_upgrade::native_state!(
    super::VimInsertPosition,
    VimInsertPositionWire,
    #[serde(remote = "super::VimInsertPosition", deny_unknown_fields)]
    enum VimInsertPositionWire {
        Cursor,
        AfterCursor,
        LineStart,
        LineEnd,
        OpenAbove,
        OpenBelow,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimEditTarget,
    VimEditTargetWire,
    #[serde(remote = "super::VimEditTarget", deny_unknown_fields)]
    enum VimEditTargetWire {
        Character,
        Line,
        LineEnd,
        Motion(VimMotion),
        Search(SearchQuery),
        TextObject {
            scope: VimTextObjectScope,
            object: VimTextObject,
        },
        Find {
            motion: VimFindMotion,
            target: char,
        },
        BufferJump {
            last: bool,
        },
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimAction,
    VimActionWire,
    #[serde(remote = "super::VimAction", deny_unknown_fields)]
    enum VimActionWire {
        Insert(VimInsertPosition),
        EnterReplaceMode,
        RestoreReplacedCharacter,
        Delete(VimEditTarget),
        Change(VimEditTarget),
        Replace(char),
        PasteAfter,
        Yank(std::sync::Arc<super::super::register::RegisterSnapshot>),
        DeleteBackward,
        DeleteForward,
        DeleteBackwardWord,
        DeleteForwardWord,
        KillLineStart,
        KillLine,
        KillLineEnd,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        MoveWordLeft,
        MoveWordRight,
        MoveLineStart { move_up_at_bol: bool },
        MoveLineEnd { move_down_at_eol: bool },
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimCommandState,
    VimCommandStateWire,
    #[serde(remote = "super::VimCommandState", deny_unknown_fields)]
    struct VimCommandStateWire {
        pending_change: Vec<VimEdit>,
        last_change: Vec<VimEdit>,
        changed: bool,
        replaying: bool,
        replace_steps: Vec<VimReplaceStep>,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimReplaceStep,
    VimReplaceStepWire,
    #[serde(remote = "super::VimReplaceStep", deny_unknown_fields)]
    struct VimReplaceStepWire {
        // Backspace also retraces attachments skipped before the replacement.
        cursor_before: usize,
        start: usize,
        inserted_len: usize,
        original: String,
    }
);
