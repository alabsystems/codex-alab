// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
pub(super) fn policy(provider: &RuntimeRequestAuth) -> codex_http_client::NetworkPolicy {
    provider.auth.http_client_factory().network_policy().clone()
}
pub(super) fn copy(
    provider: &RuntimeRequestAuth,
    headers: &mut HeaderMap,
) -> Result<(), AuthError> {
    codex_api::alab_auth_source::with_policy_sync(policy(provider), || {
        headers.extend(provider.auth.auth_headers(provider.mode)?);
        Ok(())
    })
}
pub(super) fn headers(provider: &RuntimeRequestAuth, headers: HeaderMap) -> AuthHeadersFuture<'_> {
    let auth = provider.auth.clone();
    let mode = provider.mode;
    let policy = policy(provider);
    Box::pin(async move {
        if auth.auth().alab_credential_epoch().legacy_snapshot_export()
            && codex_api::alab_auth_source::legacy_snapshot_export_available(&policy)
        {
            return Ok(replace_auth_headers(headers, auth.auth_headers(mode)?));
        }
        codex_api::alab_auth_source::select_headers(policy, async move {
            Ok(replace_auth_headers(headers, auth.auth_headers(mode)?))
        })
        .await
    })
}
