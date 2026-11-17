// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Configured-runtime projection tail with no fabricated thread or history.
//! These fields are completion DATA, never a no-Core or Native grant.
use super::RetainedFrontierOutcome;
use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct RetainedConfiguredNotification {
    pub frontier: String,
    pub snapshot: String,
    pub runtime: String,
    pub version: u64,
    pub outcome: RetainedFrontierOutcome,
}

#[cfg(test)]
#[path = "retained_configured_protocol_tests.rs"]
mod tests;
