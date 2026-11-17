// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native text, editor transactions and selection. Layout/keymap caches are fresh.
use super::*;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(remote = "super::TextArea", deny_unknown_fields)]
struct TextAreaWire {
    prefix_insertion_revision: u64,
    text: String,
    single_line: bool,
    cursor_pos: usize,
    mouse_selection: Option<mouse::MouseSelection>,
    #[serde(with = "click")]
    last_click: Option<(std::time::Instant, u16, u16, u8)>,
    #[serde(skip)]
    rendered_area: Cell<Rect>,
    #[serde(skip)]
    wrap_cache: RefCell<Option<WrapCache>>,
    preferred_col: Option<usize>,
    elements: Vec<TextElement>,
    next_element_id: u64,
    kill_buffer: String,
    kill_buffer_kind: KillBufferKind,
    register_elements: register::RegisterElements,
    vim_enabled: bool,
    vim_mode: VimMode,
    vim_pending: VimPending,
    vim_commands: VimCommandState,
    vim_search: vim_search::VimSearch,
    vim_search_enabled: bool,
    #[serde(skip, default = "editor_keymap")]
    editor_keymap: Arc<EditorKeymap>,
    #[serde(skip, default = "vim_normal_keymap")]
    vim_normal_keymap: VimNormalKeymap,
    #[serde(skip, default = "vim_operator_keymap")]
    vim_operator_keymap: VimOperatorKeymap,
    #[serde(skip, default = "vim_search_keymap")]
    vim_search_keymap: VimSearchKeymap,
    #[serde(skip, default = "vim_text_object_keymap")]
    vim_text_object_keymap: VimTextObjectKeymap,
}
fn editor_keymap() -> Arc<EditorKeymap> {
    RuntimeKeymap::defaults().editor
}
fn vim_normal_keymap() -> VimNormalKeymap {
    RuntimeKeymap::defaults().vim_normal
}
fn vim_operator_keymap() -> VimOperatorKeymap {
    RuntimeKeymap::defaults().vim_operator
}
fn vim_search_keymap() -> VimSearchKeymap {
    RuntimeKeymap::defaults().vim_search
}
fn vim_text_object_keymap() -> VimTextObjectKeymap {
    RuntimeKeymap::defaults().vim_text_object
}

impl serde::Serialize for TextArea {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TextAreaWire::serialize(self, serializer)
    }
}
impl<'de> serde::Deserialize<'de> for TextArea {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let editor = TextAreaWire::deserialize(deserializer)?;
        editor
            .alab_validate_editor_upgrade()
            .map_err(serde::de::Error::custom)?;
        Ok(editor)
    }
}
impl TextArea {
    pub(in crate::bottom_pane) fn alab_upgrade_has_layout(&self) -> bool {
        let area = self.rendered_area.get();
        area.width > 0
            && area.height > 0
            && self
                .vim_query()
                .is_none_or(|query| query.editor.alab_upgrade_has_layout())
    }
    fn alab_validate_editor_upgrade(&self) -> Result<(), &'static str> {
        if self.next_element_id == 0 || !self.text.is_char_boundary(self.cursor_pos) {
            return Err("editor cursor outside UTF-8 boundary");
        }
        if let Some(selection) = &self.mouse_selection {
            selection.alab_validate_editor_upgrade(&self.text)?;
        }
        let mut ids = std::collections::HashSet::new();
        let mut ranges = Vec::with_capacity(self.elements.len());
        for element in &self.elements {
            if element.id == 0
                || element.id >= self.next_element_id
                || !ids.insert(element.id)
                || element.range.is_empty()
                || self.text.get(element.range.clone()).is_none()
            {
                return Err("invalid editor element identity or range");
            }
            if element.range.start < self.cursor_pos && self.cursor_pos < element.range.end {
                return Err("editor cursor inside atomic element");
            }
            ranges.push(element.range.clone());
        }
        ranges.sort_by_key(|range| range.start);
        if ranges.windows(2).any(|pair| pair[0].end > pair[1].start) {
            return Err("overlapping editor elements");
        }
        if self
            .last_click
            .is_some_and(|(_, _, _, count)| !(1..=3).contains(&count))
        {
            return Err("invalid editor click count");
        }
        Ok(())
    }
}

crate::alab_editor_upgrade::native_state!(
    super::TextAreaState,
    TextAreaStateWire,
    #[serde(remote = "super::TextAreaState", deny_unknown_fields)]
    struct TextAreaStateWire {
        scroll: usize,
        manual_cursor: Option<usize>,
    }
);

mod click {
    use codex_alab_tui::upgrade_editor_clock::Snapshot;
    use serde::Deserialize;
    use serde::Serialize;
    use std::time::Instant;
    type Click = (Instant, u16, u16, u8);
    pub fn serialize<S: serde::Serializer>(
        value: &Option<Click>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .map(|(at, x, y, count)| Snapshot::capture(at).map(|at| (at, x, y, count)))
            .transpose()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Click>, D::Error> {
        Option::<(Snapshot, u16, u16, u8)>::deserialize(deserializer)?
            .map(|(at, x, y, count)| at.restore().map(|at| (at, x, y, count)))
            .transpose()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_tui::upgrade_editor_graph_scope as graph;

    fn round_trip(source: &TextArea) -> TextArea {
        let bytes = graph::encode::<Vec<TextElementSnapshot>>(source, 65536).unwrap();
        graph::decode::<Vec<TextElementSnapshot>, _>(&bytes, 65536, 128).unwrap()
    }
    fn checkpoint(source: &TextArea) -> serde_json::Value {
        serde_json::from_slice(&graph::encode::<Vec<TextElementSnapshot>>(source, 65536).unwrap())
            .unwrap()
    }

    #[test]
    fn native_whole_textarea_keeps_pending_commands_replace_recovery_and_atomic_elements() {
        for keys in [vec!['d', 'f'], vec!['R', 'x', 'y']] {
            let mut source = TextArea::new();
            source.set_text_clearing_elements("one x two x [image]");
            source.add_element_range(12..19).unwrap();
            source.set_vim_enabled(true);
            for key in &keys {
                source.input(KeyCode::Char(*key).into());
            }
            let mut restored = round_trip(&source);
            assert_eq!(checkpoint(&restored), checkpoint(&source));
            let rest = if keys[0] == 'd' {
                vec![KeyCode::Char('x')]
            } else {
                vec![KeyCode::Backspace, KeyCode::Esc, KeyCode::Char('.')]
            };
            for key in rest {
                source.input(key.into());
                restored.input(key.into());
                assert_eq!(restored.text(), source.text());
                assert_eq!(restored.cursor(), source.cursor());
                assert_eq!(
                    restored.text_element_snapshots(),
                    source.text_element_snapshots()
                );
                assert_eq!(checkpoint(&restored), checkpoint(&source));
            }
        }
    }

    #[test]
    fn native_whole_textarea_keeps_its_independent_search_query_editor() {
        let mut source = TextArea::new();
        source.set_text_clearing_elements("one é two é");
        source.set_vim_enabled(true);
        source.enable_vim_search();
        for key in ['/', 'é'] {
            source.input(KeyCode::Char(key).into());
        }
        assert!(source.vim_query().is_some());
        let mut restored = round_trip(&source);
        assert_eq!(checkpoint(&restored), checkpoint(&source));
        for key in [KeyCode::Enter, KeyCode::Char('n')] {
            source.input(key.into());
            restored.input(key.into());
            assert_eq!(checkpoint(&restored), checkpoint(&source));
        }
    }

    #[test]
    fn native_whole_textarea_rejects_invalid_utf8_cursor_and_atomic_element_ranges() {
        let mut source = TextArea::new();
        source.set_text_clearing_elements("é [image]");
        source.add_element_range(3..10).unwrap();
        let original = checkpoint(&source);
        for cursor in [1, 4, 100] {
            let mut wire = original.clone();
            wire["body"]["cursor_pos"] = cursor.into();
            assert!(
                graph::decode::<Vec<TextElementSnapshot>, TextArea>(
                    &serde_json::to_vec(&wire).unwrap(),
                    65536,
                    128
                )
                .is_err()
            );
        }
        let mut wire = original;
        wire["body"]["elements"][0]["range"]["end"] = 100.into();
        assert!(
            graph::decode::<Vec<TextElementSnapshot>, TextArea>(
                &serde_json::to_vec(&wire).unwrap(),
                65536,
                128
            )
            .is_err()
        );
    }
}
