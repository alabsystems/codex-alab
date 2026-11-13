// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Recover the request's selected export, never a manager's newer cache entry.
use crate::ProviderUnauthorizedRecovery;
use crate::SharedModelProvider;
use codex_api::SharedAuthProvider;
use codex_protocol::error::Result;

/// The provider and credentials selected for the request whose response was rejected.
pub struct RequestRecovery<'a> {
    provider: &'a SharedModelProvider,
    auth: &'a SharedAuthProvider,
}

impl<'a> RequestRecovery<'a> {
    pub fn new(provider: &'a SharedModelProvider, auth: &'a SharedAuthProvider) -> Self {
        Self { provider, auth }
    }

    pub fn provider(&self) -> &SharedModelProvider {
        self.provider
    }

    pub async fn recover(&self) -> Result<ProviderUnauthorizedRecovery> {
        let provider = self.provider;
        if provider
            .info()
            .gateway_oauth
            .as_ref()
            .is_some_and(codex_model_provider_info::GatewayOAuthConfig::uses_credential_export)
        {
            return Ok(match self.auth.alab_gateway_recovery() {
                Some(recovery) => {
                    if recovery.await {
                        ProviderUnauthorizedRecovery::Recovered
                    } else {
                        ProviderUnauthorizedRecovery::NotConfigured
                    }
                }
                None => ProviderUnauthorizedRecovery::NotConfigured,
            });
        }
        provider.recover_from_unauthorized().await
    }
}
