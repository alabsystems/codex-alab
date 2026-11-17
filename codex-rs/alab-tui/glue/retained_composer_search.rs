// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The real active Ctrl+R query, cancellation draft and suspended Vim commands.
use super::*;
crate::alab_editor_upgrade::native_state!(
    HistorySearchStatus,
    StatusWire,
    #[serde(remote = "HistorySearchStatus", deny_unknown_fields)]
    enum StatusWire {
        Idle,
        Searching,
        Match,
        NoMatch,
    }
);
crate::alab_editor_upgrade::native_state!(
    HistorySearchSession,
    SessionWire,
    #[serde(remote = "HistorySearchSession", deny_unknown_fields)]
    struct SessionWire {
        original_draft: ComposerDraft,
        preview_draft: Option<ComposerDraft>,
        original_vim_history: VimHistory,
        original_vim_state: VimPersistentState,
        query: String,
        status: HistorySearchStatus,
    }
);
