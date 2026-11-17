// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Convert the rate-limit parser's accepted milliseconds without an integer cast.

use std::time::Duration;

pub(super) fn milliseconds(value: f64) -> Option<Duration> {
    Duration::try_from_secs_f64(value / 1_000.0).ok()
}
