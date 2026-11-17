// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact parked async-question DATA including each original draft/deadline.
//! Parent-private native fields remain private; no callback or reply is created.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;
crate::alab_editor_upgrade::native_state!(
    PendingQuestion,
    PendingQuestionWire,
    #[serde(remote = "PendingQuestion", deny_unknown_fields)]
    struct PendingQuestionWire {
        message_id: String,
        question_id: String,
        question: AsyncUserInputQuestion,
        #[serde(with = "scroll")]
        options_state: ScrollState,
        draft: ComposerDraft,
        #[serde(with = "codex_alab_tui::upgrade_editor_clock::optional")]
        expires_at: Option<Instant>,
    }
);
crate::alab_editor_upgrade::native_state!(
    QuestionState,
    QuestionStateWire,
    #[serde(remote = "QuestionState", deny_unknown_fields)]
    struct QuestionStateWire {
        pending: Vec<PendingQuestion>,
        current_idx: usize,
        expanded: bool,
        seen_ids: HashSet<String>,
        answered_ids: HashSet<String>,
    }
);
mod scroll {
    use super::*;
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Data {
        #[serde(deserialize_with = "required")]
        selected_idx: Option<usize>,
        scroll_top: usize,
    }
    fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
    where
        D: serde::Deserializer<'de>,
        T: Deserialize<'de>,
    {
        T::deserialize(d)
    }
    pub(super) fn serialize<S: serde::Serializer>(
        value: &ScrollState,
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        Data {
            selected_idx: value.selected_idx,
            scroll_top: value.scroll_top,
        }
        .serialize(s)
    }
    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<ScrollState, D::Error> {
        let value = Data::deserialize(d)?;
        Ok(ScrollState {
            selected_idx: value.selected_idx,
            scroll_top: value.scroll_top,
        })
    }
}
impl QuestionState {
    pub(crate) fn alab_validate_retained_backend_questions(&self) -> io::Result<()> {
        if !self.pending.is_empty() && self.current_idx >= self.pending.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "parked async-question index differs",
            ));
        }
        let mut ids = HashSet::new();
        for question in &self.pending {
            if !ids.insert(&question.question_id) || !self.seen_ids.contains(&question.message_id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "parked original async-question identity differs",
                ));
            }
        }
        Ok(())
    }
}
