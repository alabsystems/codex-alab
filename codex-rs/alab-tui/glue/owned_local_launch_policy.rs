// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Small original CLI policy bodies shared by foreground and backend preparation.
use super::*;

pub(crate) fn permissions(
    cli: &Cli,
) -> (
    Option<SandboxMode>,
    Option<codex_protocol::protocol::AskForApproval>,
) {
    if cli.dangerously_bypass_approvals_and_sandbox {
        (
            Some(SandboxMode::DangerFullAccess),
            Some(AskForApproval::Never.to_core()),
        )
    } else {
        (
            cli.sandbox_mode.map(Into::<SandboxMode>::into),
            cli.approval_policy.map(Into::into),
        )
    }
}
pub(crate) fn config_overrides(
    cli: &mut Cli,
) -> std::io::Result<(Vec<(String, toml::Value)>, usize)> {
    cli.shared
        .take_auto_review_config_overrides(&mut cli.config_overrides);
    let before_search = cli.config_overrides.raw_overrides.len();
    if cli.web_search {
        cli.config_overrides
            .raw_overrides
            .push("web_search=\"live\"".to_owned());
    }
    codex_utils_cli::CliConfigOverrides {
        raw_overrides: cli.config_overrides.raw_overrides.clone(),
    }
    .parse_overrides()
    .map(|overrides| (overrides, before_search))
    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
}
pub(crate) fn profile(
    cli: &Cli,
    codex_home: &Path,
    mut loader: LoaderOverrides,
) -> LoaderOverrides {
    if let Some(profile) = cli.config_profile_v2.as_ref() {
        loader.user_config_path = Some(resolve_profile_v2_config_path(codex_home, profile));
        loader.user_config_profile = Some(profile.clone());
    }
    loader
}
