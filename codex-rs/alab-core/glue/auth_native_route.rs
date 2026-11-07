// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_alab_core::native_source_lifetime::NetworkScope;
impl AuthRouteConfig {
    pub(crate) fn alab_with_native_source(&self, scope: Result<NetworkScope, ()>) -> Self {
        let bind = |factory: &HttpClientFactory| {
            factory.clone().with_network_policy(
                factory
                    .network_policy()
                    .clone()
                    .with_native_source(scope.clone()),
            )
        };
        Self {
            http_client_factory: bind(&self.http_client_factory),
            application_network_policy: self
                .application_network_policy
                .clone()
                .with_native_source(scope.clone()),
            local_bootstrap_factory: self.local_bootstrap_factory.as_ref().map(bind),
        }
    }
}
