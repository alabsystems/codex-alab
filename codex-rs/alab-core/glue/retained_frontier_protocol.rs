// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Ordered same-primary projection-tail DATA. Only the actual Native origin,
//! retained Core/lease and real frontend replay/render/input owners can admit
//! continuation; this payload and its labels grant no role or readiness.
use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct RetainedFrontierNotification {
    pub frontier: String,
    pub snapshot: String,
    pub thread_id: String,
    pub outcome: RetainedFrontierOutcome,
}

/// A terminal failure after the real projection response cannot become a
/// second RPC response or be silently mistaken for an applied UI tail.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub enum RetainedFrontierOutcome {
    Ready {},
    Failed { reason: RetainedFrontierFailure },
}

/// Closed classification only. It identifies no underlying private payload,
/// physical cause, absent child, Native effect settlement or permission.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "v2/")]
pub enum RetainedFrontierFailure {
    Unavailable,
}

#[cfg(test)]
#[path = "retained_frontier_protocol_tests.rs"]
mod tests;
