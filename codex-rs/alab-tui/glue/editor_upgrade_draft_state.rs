// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native draft data; runtime installation belongs to the joined UI checkpoint.
use super::*;

crate::alab_editor_upgrade::native_state!(
    super::ComposerMentionBinding,
    ComposerMentionBindingWire,
    #[serde(remote = "super::ComposerMentionBinding", deny_unknown_fields)]
    struct ComposerMentionBindingWire {
        sigil: char,
        mention: String,
        path: String,
    }
);
crate::alab_editor_upgrade::native_state!(
    super::DraftState,
    DraftStateWire,
    #[serde(remote = "super::DraftState", deny_unknown_fields)]
    struct DraftStateWire {
        textarea: TextArea,
        textarea_state: RefCell<TextAreaState>,
        is_bash_mode: bool,
        pending_pastes: Vec<(String, String)>,
        input_enabled: bool,
        input_disabled_placeholder: Option<String>,
        paste_burst: PasteBurst,
        disable_paste_burst: bool,
        skill_completion_space: Option<usize>,
        mention_bindings: HashMap<u64, ComposerMentionBinding>,
        recent_submission_mention_bindings: Vec<MentionBinding>,
    }
);
