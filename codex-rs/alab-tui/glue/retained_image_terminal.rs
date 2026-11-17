// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
impl<B> Terminal<B>
where
    B: Backend<Error = io::Error> + Write,
{
    pub(crate) fn alab_image_first_id(&self) -> u32 {
        self.tmux_kitty_image_base
            .unwrap_or(crate::transcript_image::FIRST_IMAGE_ID)
    }
}
