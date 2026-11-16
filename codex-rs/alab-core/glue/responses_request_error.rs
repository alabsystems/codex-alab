// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The caller has decoded the complete error and matched its exact request code.
//! Neither an error type nor opaque message text establishes an invalid request.

use crate::error::ApiError;

pub(super) fn invalid_request(message: Option<String>) -> ApiError {
    ApiError::InvalidRequest {
        message: message
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| "Invalid request.".to_string()),
    }
}
