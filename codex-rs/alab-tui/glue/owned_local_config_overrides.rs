// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original TUI's pure config-override mapping, callable by both local
//! preparation paths after their genuine CLI and interactive OSS selection.
use super::Arg0DispatchPaths;
use super::Cli;
use super::ConfigOverrides;
use super::SandboxMode;
use codex_protocol::protocol::AskForApproval;
use std::path::PathBuf;

pub(crate) fn configuration_overrides(
    cli: &Cli,
    arg0_paths: &Arg0DispatchPaths,
    approval_policy: Option<AskForApproval>,
    sandbox_mode: Option<SandboxMode>,
    cwd_override: Option<PathBuf>,
    model_provider_override: Option<&str>,
) -> ConfigOverrides {
    let model = if let Some(model) = &cli.model {
        Some(model.clone())
    } else if cli.oss {
        model_provider_override
            .and_then(codex_utils_oss::get_default_model_for_oss_provider)
            .map(std::borrow::ToOwned::to_owned)
    } else {
        None
    };
    ConfigOverrides {
        model,
        approval_policy,
        sandbox_mode,
        cwd: cwd_override,
        model_provider: model_provider_override.map(str::to_owned),
        codex_self_exe: arg0_paths.codex_self_exe.clone(),
        codex_linux_sandbox_exe: arg0_paths.codex_linux_sandbox_exe.clone(),
        show_raw_agent_reasoning: cli.oss.then_some(true),
        bypass_hook_trust: cli.bypass_hook_trust.then_some(true),
        additional_writable_roots: cli.add_dir.clone(),
        ..Default::default()
    }
}
