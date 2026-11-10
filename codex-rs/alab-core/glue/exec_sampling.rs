// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

impl super::SamplingProgress {
    pub(in crate::context_manager) fn capture_exec_sampling(&self) -> (u64, u64) {
        let Self {
            last_sequence,
            sampled_through,
        } = self;
        (*last_sequence, *sampled_through)
    }

    pub(in crate::context_manager) fn restore_exec_sampling(
        last_sequence: u64,
        sampled_through: u64,
    ) -> Result<Self, String> {
        if sampled_through > last_sequence {
            return Err("sampled input exceeds its assigned sequence".into());
        }
        Ok(Self {
            last_sequence,
            sampled_through,
        })
    }
}
