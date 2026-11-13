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
