// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Search-entry state, including its independent native query editor.
use super::*;

crate::alab_editor_upgrade::native_state!(
    super::VimSearch,
    VimSearchWire,
    #[serde(remote = "super::VimSearch", deny_unknown_fields)]
    struct VimSearchWire {
        input: Option<Box<SearchInput>>,
        last: SearchQuery,
    }
);
crate::alab_editor_upgrade::native_state!(
    super::SearchInput,
    SearchInputWire,
    #[serde(remote = "super::SearchInput", deny_unknown_fields)]
    struct SearchInputWire {
        editor: TextArea,
        direction: SearchDirection,
    }
);
