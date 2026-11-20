// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep thread and turn identities while loading and projecting history.

use std::collections::HashMap;

use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ThreadResponseIdMismatch {
    #[error("{method} for {requested} returned thread ID {returned}")]
    Foreign {
        method: &'static str,
        requested: ThreadId,
        returned: ThreadId,
    },
    #[error("{method} for {requested} returned an invalid thread ID")]
    Invalid {
        method: &'static str,
        requested: ThreadId,
    },
}

pub(super) fn validate_thread_response(
    method: &'static str,
    requested: ThreadId,
    returned: &str,
) -> Result<(), ThreadResponseIdMismatch> {
    match ThreadId::from_string(returned) {
        Ok(actual) if actual == requested => Ok(()),
        Ok(returned) => Err(ThreadResponseIdMismatch::Foreign {
            method,
            requested,
            returned,
        }),
        Err(_) => Err(ThreadResponseIdMismatch::Invalid { method, requested }),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HistoryPageItem {
    pub turn_id: String,
    pub payload: ThreadItemPayload,
}

/// Return borrowed groups, so a turn boundary can never create an invalid slice range.
pub(crate) fn group_turn_items<'items, 'turns>(
    items: &'items [HistoryPageItem],
    turns: &'turns [Turn],
) -> Vec<(&'items [HistoryPageItem], Option<&'turns Turn>)> {
    let mut boundaries: HashMap<(&str, &str), Vec<&Turn>> = HashMap::new();
    let mut leading = Vec::new();
    let mut preceding = None;
    for turn in turns {
        if let Some(last) = turn.items.last() {
            let key = (turn.id.as_str(), last.id());
            boundaries.entry(key).or_default().push(turn);
            preceding = Some(key);
        } else if turn.status == TurnStatus::Interrupted
            && turn.items_view != TurnItemsView::NotLoaded
        {
            if let Some(key) = preceding {
                boundaries.entry(key).or_default().push(turn);
            } else {
                leading.push(turn);
            }
        }
    }

    let mut groups = Vec::new();
    // An interruption before all items belongs only to the page containing history's start.
    let first = turns
        .iter()
        .find_map(|turn| turn.items.first().map(|item| (turn.id.as_str(), item.id())));
    if items
        .first()
        .is_some_and(|item| Some((item.turn_id.as_str(), item.payload.id())) == first)
    {
        let empty: &[HistoryPageItem] = &[];
        groups.extend(leading.into_iter().map(|turn| (empty, Some(turn))));
    }
    // The page, rather than turn creation order, determines completion order. Item IDs
    // are scoped to their turn, including when different turns reuse the same ID.
    for group in items.split_inclusive(|item| {
        boundaries.contains_key(&(item.turn_id.as_str(), item.payload.id()))
    }) {
        let completion = group
            .last()
            .and_then(|item| boundaries.get(&(item.turn_id.as_str(), item.payload.id())));
        if let Some(completion) = completion {
            let mut group = group;
            for turn in completion {
                groups.push((group, Some(*turn)));
                group = &[];
            }
        } else {
            groups.push((group, None));
        }
    }
    groups
}

#[cfg(test)]
#[path = "history_page_test_support.rs"]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "history_page_tests.rs"]
mod tests;
