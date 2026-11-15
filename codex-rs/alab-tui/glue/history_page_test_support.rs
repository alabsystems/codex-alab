// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Adapters for upstream's original range-based pagination assertions.

use super::HistoryPageItem;
use super::group_turn_items;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use std::ops::Range;

pub(crate) fn page_items(turn_id: &str, items: &[ThreadItem]) -> Vec<HistoryPageItem> {
    items
        .iter()
        .map(|entry| HistoryPageItem {
            turn_id: turn_id.to_string(),
            payload: entry.payload.clone(),
        })
        .collect()
}

pub(crate) fn group_ranges<'turns>(
    items: &[HistoryPageItem],
    turns: &'turns [Turn],
) -> Vec<(Range<usize>, Option<&'turns Turn>)> {
    let mut start = 0;
    group_turn_items(items, turns)
        .into_iter()
        .map(|(items, turn)| {
            let end = start + items.len();
            let range = start..end;
            start = end;
            (range, turn)
        })
        .collect()
}
