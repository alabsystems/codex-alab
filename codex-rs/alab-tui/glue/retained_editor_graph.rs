// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! One enclosing whole-App editor graph without exposing textarea internals.
//! Callers borrow all actual native editors in their one frame; no scope may
//! nest or cross an await. Encoding/decoding DATA grants no runtime permission.
use codex_alab_tui::upgrade_editor_graph_scope as graph;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io;

pub(crate) fn configure(editor: &mut super::TextArea, keymap: &crate::keymap::RuntimeKeymap) {
    editor.set_keymap_bindings(keymap);
    if let Some(query) = editor.vim_query_mut() {
        configure(&mut query.editor, keymap);
    }
}

pub(crate) fn encode(value: &impl Serialize, max_bytes: usize) -> io::Result<Vec<u8>> {
    graph::encode::<Vec<super::textarea::TextElementSnapshot>>(value, max_bytes)
        .map_err(io::Error::other)
}

pub(crate) fn decode<B: DeserializeOwned>(
    bytes: &[u8],
    max_bytes: usize,
    max_generations: usize,
) -> io::Result<B> {
    graph::decode::<Vec<super::textarea::TextElementSnapshot>, B>(bytes, max_bytes, max_generations)
        .map_err(io::Error::other)
}
