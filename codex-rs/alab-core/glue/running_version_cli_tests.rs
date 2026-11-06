// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual version consumers run in isolated test children, never the shared test process.

use clap::CommandFactory;
use codex_model_provider_info::ModelProviderInfo;
#[path = "running_version_process.rs"]
mod process;

#[test]
fn current_consumers_report_the_immutable_running_build_in_separate_processes()
-> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    process::run(module_path!())?;
    assert!(!codex_alab_core::active());
    Ok(())
}

#[test]
fn running_version_consumer_child() {
    let Some((version, whole)) = process::identity(module_path!()) else {
        return;
    };
    assert!(!codex_alab_core::active());
    assert_eq!(
        codex_alab_core::version::running_version("caller-fallback"),
        "caller-fallback"
    );
    // Decision 8: codex-alab's own line is what it reports as itself; the client version,
    // upstream's numbering, is what the server-facing readers below take.
    // codex-alab's own version beside the client version: the same count and commit on its
    // own line.
    let product = match version {
        "0.165.0-dev.1+gaaaaaaaaa" => "0.1.0-dev.1+gaaaaaaaaa",
        "0.166.0-dev.2+gbbbbbbbbb" => "0.2.0-dev.2+gbbbbbbbbb",
        other => panic!("no product version for {other}"),
    };
    codex_alab_core::version::initialize_package(
        version,
        Some(product),
        Some("0.164.0"),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    )
    .expect("first running identity");
    assert!(
        !codex_alab_core::active(),
        "identity initialization cannot activate behavior"
    );
    assert_eq!(codex_alab_core::version::alab(), product);
    assert_eq!(
        codex_alab_core::version::running_version("fallback"),
        version
    );
    assert_eq!(codex_alab_tui::session::version(), product);
    let report = codex_alab_tui::upgrade::update_report("fixture-source");
    assert!(
        report.contains(&format!("Running codex-alab {product}")),
        "{report}"
    );
    assert!(
        report.contains(&format!("Codex client version: {version}")),
        "{report}"
    );
    for command in [
        super::MultitoolCli::command(),
        codex_tui::Cli::command(),
        codex_exec::Cli::command(),
        codex_cloud_tasks::Cli::command(),
    ] {
        assert_eq!(command.get_version(), Some(product));
    }
    let provider = ModelProviderInfo::create_openai_provider(None);
    assert_eq!(
        provider
            .http_headers
            .as_ref()
            .and_then(|headers| headers.get("version"))
            .map(|value| value.as_str()),
        Some(version)
    );
    let user_agent = codex_login::default_client::get_codex_user_agent();
    assert!(
        user_agent.contains(&format!("/{version} (")),
        "{user_agent}"
    );
    assert_eq!(codex_models_manager::client_version_to_whole(), whole);
    assert!(codex_alab_core::version::initialize("9.9.9", None, None).is_err());
    assert!(codex_alab_core::version::initialize(version, None, None).is_err());
    assert_eq!(codex_alab_tui::session::version(), product);
    println!("{}", process::COMPLETE);
}
