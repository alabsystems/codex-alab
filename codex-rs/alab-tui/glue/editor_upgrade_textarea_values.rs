// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native editor data only; these projections neither commit nor restore a UI.
use super::*;

crate::alab_editor_upgrade::native_state!(
    super::TextElement,
    TextElementWire,
    #[serde(remote = "super::TextElement", deny_unknown_fields)]
    struct TextElementWire {
        id: u64,
        range: Range<usize>,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::TextElementSnapshot,
    TextElementSnapshotWire,
    #[serde(remote = "super::TextElementSnapshot", deny_unknown_fields)]
    struct TextElementSnapshotWire {
        id: u64,
        range: Range<usize>,
        text: String,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::KillBufferKind,
    KillBufferKindWire,
    #[serde(remote = "super::KillBufferKind", deny_unknown_fields)]
    enum KillBufferKindWire {
        /// Characterwise kills and yanks paste at the cursor.
        Characterwise,
        /// Linewise kills and yanks paste as whole lines below the cursor line.
        Linewise,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::KillBufferSnapshot,
    KillBufferSnapshotWire,
    #[serde(remote = "super::KillBufferSnapshot", deny_unknown_fields)]
    struct KillBufferSnapshotWire {
        text: String,
        kind: KillBufferKind,
        #[serde(with = "codex_alab_tui::upgrade_editor_graph_scope::strong")]
        elements: Arc<Vec<TextElementSnapshot>>,
    }
);

#[cfg(unix)]
#[path = "editor_upgrade_textarea.rs"]
mod textarea;
