// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve the original chunking hysteresis; capture never makes a new decision.
use super::*;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    catch_up: bool,
    #[serde(deserialize_with = "required")]
    below_exit_threshold_since: Option<Snapshot>,
    #[serde(deserialize_with = "required")]
    last_catch_up_exit_at: Option<Snapshot>,
}
impl Data {
    pub(crate) fn capture(actual: &AdaptiveChunkingPolicy) -> io::Result<Self> {
        let AdaptiveChunkingPolicy {
            mode,
            below_exit_threshold_since,
            last_catch_up_exit_at,
        } = actual;
        Ok(Self {
            catch_up: matches!(mode, ChunkingMode::CatchUp),
            below_exit_threshold_since: below_exit_threshold_since
                .map(Snapshot::capture)
                .transpose()
                .map_err(invalid)?,
            last_catch_up_exit_at: last_catch_up_exit_at
                .map(Snapshot::capture)
                .transpose()
                .map_err(invalid)?,
        })
    }
    pub(crate) fn restore(self) -> io::Result<AdaptiveChunkingPolicy> {
        Ok(AdaptiveChunkingPolicy {
            mode: if self.catch_up {
                ChunkingMode::CatchUp
            } else {
                ChunkingMode::Smooth
            },
            below_exit_threshold_since: self
                .below_exit_threshold_since
                .map(|x| x.restore())
                .transpose()
                .map_err(invalid)?,
            last_catch_up_exit_at: self
                .last_catch_up_exit_at
                .map(|x| x.restore())
                .transpose()
                .map_err(invalid)?,
        })
    }
}
