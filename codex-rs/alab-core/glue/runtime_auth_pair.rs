// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve the credential/transport policy denial at runtime registration.
use super::RuntimeAuthError;
use codex_http_client::NetworkPolicyDenied;

pub(super) fn error(error: std::io::Error) -> RuntimeAuthError {
    match error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<NetworkPolicyDenied>())
    {
        Some(denied) => RuntimeAuthError::Policy(*denied),
        None => RuntimeAuthError::Unavailable,
    }
}

/// Retain runtime selection's original lock, refresh and registration future
/// behind a checked interface before provider/task-local wrappers embed it.
pub(super) fn select<'a>(
    binding: &'a std::sync::Arc<super::RuntimeAuthBinding>,
    selection: &'a crate::PlatformResourceSelection,
    expected_source: Option<u64>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<crate::RuntimeAuth, RuntimeAuthError>> + Send + 'a>,
> {
    Box::pin(binding.select_auth(selection, expected_source))
}
