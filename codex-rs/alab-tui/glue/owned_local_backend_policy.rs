// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original fallible configuration engines. Interactive error presentation
//! remains at its existing foreground caller; backend preparation retains the
//! actual error and every live resource until cleanup is observed.
use super::*;

pub(super) async fn bootstrap(
    codex_home: &Path,
    cwd: Option<&AbsolutePathBuf>,
    cli_kv_overrides: Vec<(String, codex_config::TomlValue)>,
    loader_overrides: LoaderOverrides,
    strict_config: bool,
    cloud_config_bundle: CloudConfigBundleLoader,
) -> std::io::Result<ConfigTomlLoadResult> {
    load_config_toml_with_layer_stack(
        codex_home,
        cwd,
        cli_kv_overrides,
        codex_config::ConfigLoadOptions {
            loader_overrides,
            strict_config,
            cloud_config_bundle,
        },
    )
    .await
}

/// The original ordinary config engine, with the original absent fallback and
/// pre-allocation worktree owner. Errors stay fallible for backend cleanup.
pub(super) async fn config(
    cli_kv_overrides: Vec<(String, toml::Value)>,
    overrides: ConfigOverrides,
    loader_overrides: LoaderOverrides,
    cloud_config_bundle: CloudConfigBundleLoader,
    strict_config: bool,
) -> std::io::Result<Config> {
    super::load_config_with_worktree_source_policy(
        cli_kv_overrides,
        overrides,
        loader_overrides,
        cloud_config_bundle,
        strict_config,
        None,
        None,
    )
    .await
}

#[cfg(test)]
#[path = "owned_local_policy_tests.rs"]
mod tests;
