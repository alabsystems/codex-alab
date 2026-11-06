// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `codex exec`'s report opens with codex-alab's own version (alab/README.md, Decision 8), never
//! the client version OpenAI's servers see (review of 2026-10-24: it still printed `OpenAI Codex
//! v<client version>` while `--version`, the TUI and `/alab version` said codex-alab's own).
//! The opening line is printed to stderr by the real `print_config_summary`, in isolated test
//! children that install the running identity and turn ALab on, never in the shared process.

use super::EventProcessorWithHumanOutput;
use crate::event_processor::EventProcessor;
use codex_core::config::ConfigBuilder;
use codex_protocol::SessionId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::SessionConfiguredEvent;
use codex_utils_absolute_path::test_support::PathBufExt;

#[path = "running_version_process.rs"]
mod process;

/// codex-alab's own version beside each child's client version: the same count and commit.
fn product_of(client: &str) -> &'static str {
    match client {
        "0.165.0-dev.1+gaaaaaaaaa" => "0.1.0-dev.1+gaaaaaaaaa",
        "0.166.0-dev.2+gbbbbbbbbb" => "0.2.0-dev.2+gbbbbbbbbb",
        other => panic!("no product version for {other}"),
    }
}

#[test]
fn the_exec_report_opens_with_codex_alab_s_own_version() -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    let stderr = process::run(module_path!())?;
    assert_eq!(stderr.len(), 2, "one report per identity");
    for (said, client) in stderr
        .iter()
        .zip(["0.165.0-dev.1+gaaaaaaaaa", "0.166.0-dev.2+gbbbbbbbbb"])
    {
        let first = said.lines().next().unwrap_or_default();
        assert_eq!(
            first,
            format!(
                "codex-alab v{} · based on OpenAI Codex 0.164.0",
                product_of(client)
            ),
            "{said}"
        );
        assert!(
            !said.contains(client),
            "the report names the client version: {said}"
        );
        assert!(said.contains("\nuser\nfixture prompt"), "{said}");
    }
    assert!(!codex_alab_core::active());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::print_stdout,
    reason = "exact child completion handshake for the owning test"
)]
async fn running_version_consumer_child() {
    let Some((client, _)) = process::identity(module_path!()) else {
        return;
    };
    let product = product_of(client);
    codex_alab_core::version::initialize_package(
        client,
        Some(product),
        Some("0.164.0"),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    )
    .unwrap_or_else(|err| panic!("first running identity: {err}"));
    codex_alab_core::activate();
    let codex_home = tempfile::tempdir().unwrap_or_else(|err| panic!("codex home: {err}"));
    let cwd = tempfile::tempdir().unwrap_or_else(|err| panic!("cwd: {err}"));
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .fallback_cwd(Some(cwd.path().to_path_buf()))
        .build()
        .await
        .unwrap_or_else(|err| panic!("default config: {err}"));
    let event = SessionConfiguredEvent {
        session_id: SessionId::new(),
        thread_id: ThreadId::new(),
        forked_from_id: None,
        parent_thread_id: None,
        thread_source: None,
        thread_name: None,
        model: "gpt-5.4".to_string(),
        model_provider_id: config.model_provider_id.clone(),
        service_tier: None,
        approval_policy: AskForApproval::Never,
        approvals_reviewer: config.approvals_reviewer,
        permission_profile: config.permissions.effective_permission_profile(),
        active_permission_profile: None,
        cwd: cwd.path().to_path_buf().abs(),
        reasoning_effort: None,
        network_proxy: None,
        rollout_path: None,
    };
    let mut report = EventProcessorWithHumanOutput::create_with_ansi(
        /*with_ansi*/ false, &config, /*last_message_path*/ None,
    );
    report.print_config_summary(&config, "fixture prompt", &event);
    println!("{}", process::COMPLETE);
}
