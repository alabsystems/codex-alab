// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Executor compatibility follows the executable's immutable client version.

use codex_build_info::BuildInfo;

pub(super) fn version(upstream: String) -> String {
    let client = codex_alab_core::version::running_version("");
    let build = BuildInfo::from_version(client);
    // Unknown source identities and malformed stamps retain upstream's fallback.
    // Identity also applies to helpers and vanilla mode, independently of active().
    if !build.is_source_build() && build.version().to_string() == client {
        client.to_owned()
    } else {
        upstream
    }
}

#[cfg(test)]
#[path = "executor_version_tests.rs"]
mod tests;
