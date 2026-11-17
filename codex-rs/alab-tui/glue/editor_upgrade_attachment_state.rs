// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native attachment metadata, with no file reads or external image effects.
use super::*;
crate::alab_editor_upgrade::native_state!(
    super::AttachmentState,
    AttachmentStateWire,
    #[serde(remote = "super::AttachmentState", deny_unknown_fields)]
    struct AttachmentStateWire {
        local_images: Vec<LocalImageAttachment>,
        remote_image_urls: Vec<String>,
        selected_remote_image_index: Option<usize>,
    }
);
