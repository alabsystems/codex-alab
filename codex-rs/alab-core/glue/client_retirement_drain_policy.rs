// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Cleanup dispatch/join policy only. Caller owns real SDK resources and closes
//! fresh Source acceptance first. No callback result grants Native authority.
use tokio_util::task::TaskTracker;

pub(super) async fn drain_retirements<T>(
    tracker: &TaskTracker,
    records: &[T],
    mut start: impl FnMut(&T) -> Result<(), String>,
    mut inspect: impl FnMut(&T) -> Result<(), String>,
) -> Result<(), String> {
    let mut error = None;
    for record in records {
        if let Err(refused) = start(record) {
            error.get_or_insert(refused);
        }
    }
    // Even a dispatch refusal cannot abandon jobs already tracked, including
    // original jobs which predate this loop and later exact retirement jobs.
    tracker.wait().await;
    for record in records {
        if let Err(unobserved) = inspect(record) {
            error.get_or_insert(unobserved);
        }
    }
    error.map_or(Ok(()), Err)
}

#[cfg(test)]
#[path = "client_retirement_drain_policy_tests.rs"]
mod tests;
