// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::AmazonBedrockModelProvider;
use super::auth::BedrockAuthSource;
use super::auth::bearer_token_region;
use super::mantle::region_from_config;
use codex_aws_auth::alab_local_catalog_region::locally_available_region;
use codex_login::CodexAuth;

impl AmazonBedrockModelProvider {
    pub(super) fn alab_local_mantle_region(&self) -> Option<String> {
        // An explicit endpoint owns its catalog regardless of ambient AWS state.
        if self.info.base_url.is_some() {
            return None;
        }
        match self.auth_source() {
            BedrockAuthSource::ManagedBearerToken => match self.managed_auth() {
                Some(CodexAuth::BedrockApiKey(auth)) => Some(auth.region),
                _ => None,
            },
            BedrockAuthSource::EnvBearerToken => bearer_token_region(&self.aws, std::env::var).ok(),
            _ => region_from_config(&self.aws)
                .or_else(|| locally_available_region(self.aws.profile.as_deref())),
        }
    }
}

#[cfg(test)]
#[path = "mantle_catalog_region_tests.rs"]
mod tests;

#[path = "unresolved_mantle_catalog.rs"]
mod unresolved;
