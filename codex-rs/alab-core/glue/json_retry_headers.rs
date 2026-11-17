// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve repeated retry advice without changing other header collision rules.

use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;
use http::header::RETRY_AFTER;

pub(super) fn insert(headers: &mut HeaderMap, name: HeaderName, value: HeaderValue) {
    if codex_alab_core::active() && name == RETRY_AFTER {
        headers.append(name, value);
    } else {
        headers.insert(name, value);
    }
}
