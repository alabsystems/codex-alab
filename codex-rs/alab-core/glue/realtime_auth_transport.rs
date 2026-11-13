// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The runtime selection owns the credential; effective config owns destination policy.

use crate::config::Config;
use codex_http_client::HttpClientFactory;
use codex_model_provider_info::ModelProviderInfo;

pub(super) fn transport_for_runtime(
    provider: &ModelProviderInfo,
    config: &Config,
    current: HttpClientFactory,
) -> HttpClientFactory {
    if provider.env_key.is_some() || provider.experimental_bearer_token.is_some() {
        // Native realtime preparation first binds the login factory, then selects
        // a distinct explicit-key runtime even when the credential bytes match.
        // Return the same configured destination restrictions before the native
        // selected runtime adds its own source fence. Do not pair two managers.
        config.http_client_factory()
    } else {
        current
    }
}
