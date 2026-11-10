// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Earned-reset recovery under a cross-process custody lock.
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::State;
#[cfg(unix)]
pub use unix::ordinary_weekly_wall;
#[cfg(unix)]
pub use unix::preflight;
#[cfg(unix)]
pub use unix::recover_weekly;

#[cfg(not(unix))]
#[derive(Default)]
pub struct State;
#[cfg(not(unix))]
impl State {
    pub async fn bind(_: &codex_login::AuthManager) -> Self {
        Self
    }
}
#[cfg(not(unix))]
pub fn ordinary_weekly_wall(_: &codex_protocol::error::UsageLimitReachedError) -> bool {
    false
}
#[cfg(not(unix))]
pub async fn recover_weekly(
    _: &codex_login::AuthManager,
    _: &std::path::Path,
    _: &str,
    _: codex_http_client::HttpClientFactory,
    _: &str,
    _: &State,
    _: Option<i64>,
) -> bool {
    false
}

#[cfg(not(unix))]
pub async fn preflight(
    _: &codex_login::AuthManager,
    _: &std::path::Path,
    _: &str,
    _: codex_http_client::HttpClientFactory,
    _: &str,
    _: &State,
) {
}
