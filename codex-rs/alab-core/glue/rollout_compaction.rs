// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve typed compaction boundaries in both native history formats.

use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;

pub(super) fn persist(event: &EventMsg) -> bool {
    codex_alab_core::active()
        && match event {
            EventMsg::ItemStarted(event) => matches!(event.item, TurnItem::ContextCompaction(_)),
            EventMsg::ItemCompleted(event) => matches!(event.item, TurnItem::ContextCompaction(_)),
            _ => false,
        }
}
