// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact original app/startup source builder, with no Config reconstruction.
use super::*;
pub(crate) fn create(
    codex_home: &Path,
    cli: Vec<(String, toml::Value)>,
    mut harness: ConfigOverrides,
    loader: LoaderOverrides,
    cloud: CloudConfigBundleLoader,
) -> ConfigBuilder {
    harness.cwd = None;
    ConfigBuilder::default()
        .codex_home(codex_home.to_path_buf())
        .cli_overrides(cli)
        .harness_overrides(harness)
        .loader_overrides(loader)
        .cloud_config_bundle(cloud)
}

#[cfg(test)]
#[path = "owned_worktree_source_builder_tests.rs"]
mod tests;
