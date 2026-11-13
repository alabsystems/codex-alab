// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Session-local trusted audio metadata crosses native memory handoff, never model requests.

use std::collections::BTreeMap;
use std::time::Duration;

impl super::FileAudioDurations {
    pub(crate) fn capture_exec(&self) -> BTreeMap<String, Duration> {
        self.0.clone()
    }

    pub(crate) fn restore_exec(values: BTreeMap<String, Duration>) -> Result<Self, String> {
        if values.iter().any(|(id, duration)| {
            id.is_empty()
                || duration.is_zero()
                || duration.subsec_nanos() % 1_000_000 != 0
                || duration.as_millis() > u128::from(u64::MAX)
        }) {
            return Err("file audio metadata has an invalid ID or duration".into());
        }
        Ok(Self(values))
    }
}
