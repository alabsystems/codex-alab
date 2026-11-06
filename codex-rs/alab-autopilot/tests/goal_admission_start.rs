// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#[path = "../glue/goal_start_recovery.rs"]
mod goal_start_recovery;

#[test]
fn provider_advice_never_moves_to_local_retry() {
    use codex_protocol::error::CodexErr;
    use std::time::Duration;

    let advice =
        codex_http_client::RetryAfter::from_delay(Duration::from_secs(60)).expect("advice");
    let local = CodexErr::Io(std::io::Error::from(std::io::ErrorKind::Interrupted))
        .with_retry_after(advice);
    assert!(!goal_start_recovery::retryable_start(&local));
}
