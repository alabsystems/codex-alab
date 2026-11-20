// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original TUI loader inputs for a later runtime forest capture. A resolved
//! thread config alone cannot reconstruct executable overrides or config layers.
use super::super::Config;
use super::super::ConfigOverrides;
use super::ExecConfiguration;
use std::sync::OnceLock;

struct Original {
    configuration: ExecConfiguration,
    model_can_follow: bool,
}

static INVOCATION: OnceLock<Result<Original, String>> = OnceLock::new();

impl ExecConfiguration {
    /// Called at the actual TUI startup boundary, before configuration can be
    /// replaced by resume/fork/backend selection. Failure holds future upgrades
    /// rather than replacing these inputs with guessed defaults.
    pub fn record_tui_invocation(config: &Config, overrides: &ConfigOverrides) {
        if codex_alab_core::is_alab_binary() || codex_alab_core::active() {
            INVOCATION.get_or_init(|| {
                Ok(Original {
                    configuration: Self::capture(config, overrides)
                        .map_err(|error| error.to_string())?,
                    model_can_follow: overrides.model.is_none()
                        && config.alab_follows_saved_model(),
                })
            });
        }
    }

    pub fn tui_invocation() -> Result<Self, String> {
        INVOCATION
            .get()
            .ok_or("original TUI loader inputs are unavailable")?
            .as_ref()
            .map(|original| original.configuration.clone())
            .map_err(Clone::clone)
    }

    pub(super) fn unpinned_tui_invocation() -> Option<&'static Self> {
        let original = INVOCATION.get()?.as_ref().ok()?;
        original.model_can_follow.then_some(&original.configuration)
    }
}
