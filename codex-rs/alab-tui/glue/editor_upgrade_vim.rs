// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Data-only native Vim states used by the editor runtime checkpoint.
use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;

    #[test]
    fn native_pending_find_operator_keeps_its_next_keystroke_meaning() {
        for operator in ['d', 'y', 'c'] {
            let mut source = TextArea::new();
            source.set_text_clearing_elements("one x two x");
            source.set_vim_enabled(true);
            source.input(KeyCode::Char(operator).into());
            source.input(KeyCode::Char('f').into());
            assert!(matches!(source.vim_pending, VimPending::Find { .. }));
            let pending = serde_json::to_vec(&source.vim_pending).unwrap();
            let mode = serde_json::to_vec(&source.vim_mode).unwrap();

            let mut destination = TextArea::new();
            destination.set_text_clearing_elements(source.text());
            destination.set_vim_enabled(true);
            destination.set_cursor(source.cursor());
            destination.vim_pending = serde_json::from_slice(&pending).unwrap();
            destination.vim_mode = serde_json::from_slice(&mode).unwrap();
            source.input(KeyCode::Char('x').into());
            destination.input(KeyCode::Char('x').into());
            assert_eq!(destination.text(), source.text());
            assert_eq!(destination.cursor(), source.cursor());
            assert_eq!(destination.vim_mode, source.vim_mode);
            assert_eq!(destination.vim_pending, source.vim_pending);
            assert_eq!(destination.kill_buffer, source.kill_buffer);
        }
        assert!(
            serde_json::from_str::<VimPending>(
                r#"{"Find":{"motion":"Forward","operator":"Delete","extra":true}}"#
            )
            .is_err()
        );
        assert!(serde_json::from_str::<VimPending>(r#""unknown-pending-command""#).is_err());
    }
}

crate::alab_editor_upgrade::native_state!(
    super::VimMode,
    VimModeWire,
    #[serde(remote = "super::VimMode", deny_unknown_fields)]
    enum VimModeWire {
        /// Normal mode routes printable keys to movement, operators, and mode transitions.
        Normal,
        /// Insert mode routes input through the regular editor keymap until Escape is pressed.
        Insert,
        /// Replace mode overwrites existing graphemes through the regular editor keymap.
        Replace,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimOperator,
    VimOperatorWire,
    #[serde(remote = "super::VimOperator", deny_unknown_fields)]
    enum VimOperatorWire {
        Delete,
        Yank,
        Change,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimPending,
    VimPendingWire,
    #[serde(remote = "super::VimPending", deny_unknown_fields)]
    enum VimPendingWire {
        None,
        Operator(VimOperator),
        TextObject {
            operator: VimOperator,
            scope: VimTextObjectScope,
        },
        Replace,
        Find {
            motion: VimFindMotion,
            operator: Option<VimOperator>,
        },
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimFindMotion,
    VimFindMotionWire,
    #[serde(remote = "super::VimFindMotion", deny_unknown_fields)]
    enum VimFindMotionWire {
        Forward,
        Backward,
        TillForward,
        TillBackward,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimMotion,
    VimMotionWire,
    #[serde(remote = "super::VimMotion", deny_unknown_fields)]
    enum VimMotionWire {
        Left,
        Right,
        Up,
        Down,
        WordForward,
        WordBackward,
        WordEnd,
        LineStart,
        LineEnd,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimTextObjectScope,
    VimTextObjectScopeWire,
    #[serde(remote = "super::VimTextObjectScope", deny_unknown_fields)]
    enum VimTextObjectScopeWire {
        Inner,
        Around,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::VimTextObject,
    VimTextObjectWire,
    #[serde(remote = "super::VimTextObject", deny_unknown_fields)]
    enum VimTextObjectWire {
        Word,
        BigWord,
        Parentheses,
        Brackets,
        Braces,
        DoubleQuote,
        SingleQuote,
        Backtick,
    }
);
