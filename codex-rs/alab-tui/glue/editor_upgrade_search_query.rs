// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native editor data only; these projections neither commit nor restore a UI.
use super::*;

crate::alab_editor_upgrade::native_state!(
    super::SearchDirection,
    SearchDirectionWire,
    #[serde(remote = "super::SearchDirection", deny_unknown_fields)]
    enum SearchDirectionWire {
        Forward,
        Backward,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::SearchQuery,
    SearchQueryWire,
    #[serde(remote = "super::SearchQuery", deny_unknown_fields)]
    struct SearchQueryWire {
        text: String,
        direction: SearchDirection,
    }
);
