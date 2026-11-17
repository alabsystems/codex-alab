// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve bounded native undo/redo transactions without evicting any on decode.
use super::*;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(remote = "super::VimHistory", deny_unknown_fields)]
struct VimHistoryWire {
    undo: VecDeque<ComposerDraft>,
    redo: VecDeque<ComposerDraft>,
    pending: Option<ComposerDraft>,
}
impl serde::Serialize for VimHistory {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        VimHistoryWire::serialize(self, serializer)
    }
}
impl<'de> serde::Deserialize<'de> for VimHistory {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let history = VimHistoryWire::deserialize(deserializer)?;
        let bytes = history
            .undo
            .iter()
            .chain(history.redo.iter())
            .try_fold(0usize, |sum, draft| {
                sum.checked_add(draft.vim_history_bytes())
            });
        if history.undo.len().saturating_add(history.redo.len()) > MAX_VIM_UNDO_STEPS
            || bytes.is_none_or(|bytes| bytes > MAX_VIM_UNDO_BYTES)
            || history
                .pending
                .as_ref()
                .is_some_and(|draft| draft.vim_history_bytes() > MAX_VIM_UNDO_BYTES)
        {
            return Err(serde::de::Error::custom(
                "editor history exceeds native limits",
            ));
        }
        Ok(history)
    }
}
