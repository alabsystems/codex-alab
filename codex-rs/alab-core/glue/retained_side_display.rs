// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Display DATA from a real canonical occurrence. Actual Scope, original
//! listener, Native origin, writer cut and last Core Current stay the caller's
//! independent requirements; this reducer cannot authorize any of them.
use super::*;
use codex_core::RetainedSideMarker;
use std::collections::HashSet;

pub(super) struct Display {
    pub(super) turns: Vec<Turn>,
    pub(super) before: HashSet<(String, String)>,
}
pub(super) fn build(
    core: &CodexThread,
    boundary: &codex_core::RecordedSideBoundary,
    records: &[RolloutItem],
    mode: ThreadHistoryMode,
) -> Result<Display, JSONRPCErrorError> {
    boundary
        .check_owner(core)
        .map_err(|error| invalid_request(error.to_string()))?;
    capture_data(
        records,
        mode,
        boundary.marker_data(),
        boundary.turn_data(),
        boundary.was_queued_data(),
    )
    .map_err(invalid_request)
}

// Private lower DATA seam. Tests never create a Native/Core receipt to call it.
#[cfg(test)]
fn build_data(
    records: &[RolloutItem],
    mode: ThreadHistoryMode,
    marker: &RetainedSideMarker,
    queued_turn: &str,
    was_queued: bool,
) -> Result<Vec<Turn>, &'static str> {
    Ok(capture_data(records, mode, marker, queued_turn, was_queued)?.turns)
}
fn capture_data(
    records: &[RolloutItem],
    mode: ThreadHistoryMode,
    marker: &RetainedSideMarker,
    queued_turn: &str,
    was_queued: bool,
) -> Result<Display, &'static str> {
    let is_boundary = |record: &RolloutItem| {
        matches!(record,
            RolloutItem::ResponseItem(item)
            if item.metadata.as_ref().and_then(|metadata| metadata.alab_side_boundary.as_ref()) == Some(marker)
        )
    };
    if records.iter().filter(|record| is_boundary(record)).count() != 1 {
        return Err("actual Side canonical cut is missing or ambiguous");
    }
    let mut builder = codex_app_server_protocol::ThreadHistoryBuilder::new();
    let mut before_items = HashSet::new();
    let mut before_turns = HashSet::new();
    let mut visible_items = HashSet::new();
    let mut new_turns = HashSet::new();
    let mut after = false;
    for record in records {
        // Privacy/truncation precedes the SAME reducer just as the original
        // protected memory display path; canonical IDs/counters are preserved.
        if let Some(retained) = codex_rollout::persisted_rollout_item(record, mode) {
            let changed =
                builder.handle_retained_history_item_with_changes(retained.as_ref(), mode);
            for item in changed.changed_items {
                let key = (item.turn_id.clone(), item.item.payload.id().to_owned());
                if after {
                    if !before_items.contains(&key) {
                        visible_items.insert(key);
                    }
                } else {
                    before_turns.insert(item.turn_id);
                    before_items.insert(key);
                }
            }
            for turn in changed.changed_turns {
                if after {
                    if !before_turns.contains(&turn.turn_id) {
                        new_turns.insert(turn.turn_id);
                    }
                } else {
                    before_turns.insert(turn.turn_id);
                }
            }
        }
        // This is an actual structured occurrence within the caller's captured
        // canonical records. Boundary text or an arbitrary matching ID cannot
        // construct the private Core receipt required by build().
        after |= is_boundary(record);
    }
    let turns = builder
        .finish()
        .into_iter()
        .filter_map(|mut turn| {
            turn.items.retain(|item| {
                visible_items.contains(&(turn.id.clone(), item.payload.id().to_owned()))
            });
            // A queued delimiter may land inside a genuinely running turn. Retain
            // its ORIGINAL empty active metadata, never synthesize TurnStarted.
            let actual_queued_active = was_queued
                && turn.id == queued_turn
                && turn.status == codex_app_server_protocol::TurnStatus::InProgress;
            (!turn.items.is_empty() || new_turns.contains(&turn.id) || actual_queued_active)
                .then_some(turn)
        })
        .collect();
    Ok(Display {
        turns,
        before: before_items,
    })
}

#[cfg(test)]
#[path = "retained_side_display_tests.rs"]
mod tests;
