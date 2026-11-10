// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::time::Duration;

impl super::FileAudioDurations {
    pub(crate) fn capture_exec(&self) -> BTreeMap<String, Duration> {
        self.0.clone()
    }

    pub(crate) fn restore_exec(values: BTreeMap<String, Duration>) -> Result<Self, String> {
        if values
            .iter()
            .any(|(id, duration)| id.is_empty() || duration.is_zero())
        {
            return Err("invalid captured audio duration".into());
        }
        Ok(Self(values))
    }
}
