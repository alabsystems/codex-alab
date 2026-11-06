// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the standalone executor's real argument parser in isolated identities.

use clap::CommandFactory;
#[path = "running_version_process.rs"]
mod process;

#[test]
fn standalone_display_uses_product_identity_without_activating_alab() -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    process::run(module_path!())?;
    assert!(!codex_alab_core::active());
    Ok(())
}

#[test]
#[expect(
    clippy::print_stdout,
    reason = "exact child completion handshake for the owning test"
)]
fn running_version_consumer_child() {
    let Some((client, _)) = process::identity(module_path!()) else {
        return;
    };
    assert!(!codex_alab_core::active());
    assert_eq!(
        super::ExecServerCli::command().get_version(),
        Some(env!("CARGO_PKG_VERSION")),
    );
    let product = match client {
        "0.165.0-dev.1+gaaaaaaaaa" => "0.1.0-dev.1+codex.0.164.0.gaaaaaaaaa",
        "0.166.0-dev.2+gbbbbbbbbb" => "0.2.0-dev.2+codex.0.165.0.gbbbbbbbbb",
        other => panic!("no product version for {other}"),
    };
    codex_alab_core::version::initialize_package(client, Some(product), None, None)
        .expect("first running identity");
    assert!(!codex_alab_core::active());
    assert_eq!(
        codex_alab_core::version::running_version("client-fallback"),
        client,
    );
    let command = super::ExecServerCli::command();
    assert_eq!(command.get_version(), Some(product));
    let version = command
        .try_get_matches_from(["codex-exec-server", "--version"])
        .expect_err("--version exits with the displayed identity");
    assert_eq!(version.kind(), clap::error::ErrorKind::DisplayVersion);
    assert_eq!(
        version.to_string(),
        format!("codex-exec-server {product}\n")
    );
    assert!(!codex_alab_core::active());
    println!("{}", process::COMPLETE);
}
