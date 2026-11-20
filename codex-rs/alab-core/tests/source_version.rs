// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The entry point installs the process-wide identity, just as the CLI does.

use codex_alab_core::version;

#[test]
fn executable_uses_the_source_client_line_for_an_unstamped_workspace() {
    let workspace = env!("CARGO_PKG_VERSION");
    let line = include_str!("../../../alab/client-version").trim();
    let stamped_client = option_env!("CODEX_ALAB_BUILD_VERSION");
    let product = option_env!("CODEX_ALAB_PRODUCT_VERSION").filter(|value| !value.is_empty());
    let display = product.or(stamped_client).unwrap_or(workspace);
    let expected_client = stamped_client.unwrap_or(if workspace == "0.0.0" {
        line
    } else {
        workspace
    });

    codex_alab_core::initialize_running_build!().expect("initialize the executable");
    assert_eq!(version::running_version("caller"), expected_client);
    assert_eq!(version::product_version("caller"), display);
    assert_eq!(
        version::alab(),
        if display == "0.0.0" { "dev" } else { display }
    );
    assert_eq!(version::vendor_updates_allowed(), product.is_none());
}
