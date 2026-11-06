// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Review ownership comes from the parent's typed lifecycle items. Forwarded
//! delegate starts are not a new parent turn.
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;

pub(super) fn entered(event: &EventMsg) -> Option<&str> {
    match event {
        EventMsg::ItemStarted(event) if matches!(event.item, TurnItem::EnteredReviewMode(_)) => {
            Some(&event.turn_id)
        }
        EventMsg::ItemCompleted(event) if matches!(event.item, TurnItem::EnteredReviewMode(_)) => {
            Some(&event.turn_id)
        }
        _ => None,
    }
}

pub(super) fn exited(event: &EventMsg) -> Option<&str> {
    match event {
        EventMsg::ItemCompleted(event) if matches!(event.item, TurnItem::ExitedReviewMode(_)) => {
            Some(&event.turn_id)
        }
        _ => None,
    }
}
