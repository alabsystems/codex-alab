// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
pub(super) fn policy(transport: &ReqwestTransport) -> crate::NetworkPolicy {
    match &transport.client {
        TransportClient::Fixed(client) => match &client.backend {
            HttpClientBackend::Routed(pool) => pool.http_client_factory.network_policy().clone(),
            HttpClientBackend::Direct(_) => crate::NetworkPolicy::unmanaged(),
        },
        TransportClient::RouteAware(pool) => pool.http_client_factory.network_policy().clone(),
    }
}
