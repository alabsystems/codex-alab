// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original TUI loader inputs for a later runtime forest capture. A resolved
//! thread config alone cannot reconstruct executable overrides or config layers.
use super::super::Config;
use super::super::ConfigOverrides;
use super::ExecConfiguration;
use std::sync::OnceLock;

static INVOCATION: OnceLock<Result<ExecConfiguration, String>> = OnceLock::new();

impl ExecConfiguration {
    /// Called at the actual TUI startup boundary, before configuration can be
    /// replaced by resume/fork/backend selection. Failure holds future upgrades
    /// rather than replacing these inputs with guessed defaults.
    pub fn record_tui_invocation(config: &Config, overrides: &ConfigOverrides) {
        if codex_alab_core::is_alab_binary() || codex_alab_core::active() {
            INVOCATION.get_or_init(|| {
                Self::capture(config, overrides).map_err(|error| error.to_string())
            });
        }
    }

    pub fn tui_invocation() -> Result<Self, String> {
        INVOCATION
            .get()
            .ok_or("original TUI loader inputs are unavailable")?
            .clone()
    }
}
