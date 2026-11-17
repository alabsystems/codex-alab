// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact producer hooks for original passive answer/command/separator cells.
use super::*;
use crate::app::alab_side_history_data::Source;
use std::sync::PoisonError;
impl SideCompanion {
    pub(super) fn alab_note_answer(
        &mut self,
        turn: &str,
        item: &str,
        index: usize,
        payload: Option<ThreadItemPayload>,
    ) {
        let Some(cell) = self.cells.get(index) else {
            return;
        };
        self.display
            .alab_history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .note(
                vec![Source::Item {
                    turn: turn.to_owned(),
                    item: item.to_owned(),
                }],
                payload.into_iter().collect(),
                std::slice::from_ref(cell),
            );
    }
    pub(super) fn alab_note_end(&mut self, turn: &str, index: usize) {
        let Some(cell) = self.cells.get(index) else {
            return;
        };
        self.display
            .alab_history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .note(
                vec![Source::TurnEnd {
                    turn: turn.to_owned(),
                }],
                Vec::new(),
                std::slice::from_ref(cell),
            );
    }
    pub(crate) fn alab_rebase_answer(&mut self, sources: &[Source]) {
        if let Some(answer) = self.answer.as_mut().filter(|answer| {
            sources.contains(&Source::Item {
                turn: answer.turn_id.clone(),
                item: answer.item_id.clone(),
            })
        }) {
            // No new-prefix key is reconciled from the older snapshot. These
            // pending bytes belong to the exact covered original answer.
            answer.pending.clear();
        }
    }
}
