// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native memory handoff preserves delivery progress without acknowledging new input.

impl super::SamplingProgress {
    pub(crate) fn capture_exec(&self) -> (u64, u64) {
        (self.last_sequence, self.sampled_through)
    }

    pub(crate) fn restore_exec(
        (last_sequence, sampled_through): (u64, u64),
    ) -> Result<Self, String> {
        if sampled_through > last_sequence {
            return Err("sampled input exceeds its captured sequence".into());
        }
        Ok(Self {
            last_sequence,
            sampled_through,
        })
    }
}
