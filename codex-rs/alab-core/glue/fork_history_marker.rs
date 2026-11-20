// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::context::TurnAborted;
use crate::tasks::InterruptedTurnHistoryMarker;
use codex_context_fragments::ContextualUserFragment;
use codex_context_fragments::RenderedFragment;
use codex_protocol::models::ResponseItem;

/// Build the model-visible marker for a copied, unfinished turn.
pub(super) fn interrupted_turn_history_marker(
    marker: InterruptedTurnHistoryMarker,
) -> Option<ResponseItem> {
    if !codex_alab_core::active() {
        return crate::tasks::interrupted_turn_history_marker(marker);
    }

    let role = match marker {
        InterruptedTurnHistoryMarker::Disabled => return None,
        InterruptedTurnHistoryMarker::ContextualUser => "user",
        InterruptedTurnHistoryMarker::Developer => "developer",
    };
    // The classification distinguishes harness guidance from user authorization.
    let (_, content) = TurnAborted::new(concat!(
        "This history was copied before its last turn had a recorded outcome. ",
        "A pending tool call may already have acted. ",
        "Check the current state before repeating it.",
    ))
    .render_fragment()
    .into_parts();
    Some(RenderedFragment::new(role, content).into())
}
