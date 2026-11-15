// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Passive live runtime metadata. Never persisted goal state or execution authority.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum AlabGoalPersistencePhase {
    Ready,
    Writing,
    Held,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct AlabGoalRuntimeUpdatedNotification {
    pub thread_id: String,
    /// Opaque native runtime incarnation. Its bytes are never compared for order.
    pub incarnation: String,
    /// Exact native server-sink lifetime; reconnects to a new server cannot reuse its sequence.
    pub transport: String,
    /// Ordered within one authenticated native host, across every thread.
    #[serde(default)]
    #[ts(type = "number")]
    pub generation: u64,
    /// Assigned by the native server sink, ordered within that backend transport.
    #[serde(default)]
    #[ts(type = "number")]
    pub sequence: u64,
    #[ts(type = "number")]
    pub revision: u64,
    /// Absent on retirement; an older peer or unavailable runtime remains unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub persistence: Option<AlabGoalPersistencePhase>,
    pub writes_pending: bool,
    /// Passive stream overflow invalidates observations, never goal/accounting state.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reset: bool,
}

/// Local enqueue custody only. This type never crosses the wire or grants goal authority.
pub struct AlabGoalRuntimeSnapshot {
    update: AlabGoalRuntimeUpdatedNotification,
    finish: Option<Box<dyn FnOnce(AlabGoalRuntimeDelivery) + Send>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlabGoalRuntimeDelivery {
    Sent,
    Cancelled,
    Closed,
}

impl AlabGoalRuntimeSnapshot {
    pub fn new(
        update: AlabGoalRuntimeUpdatedNotification,
        finish: impl FnOnce(AlabGoalRuntimeDelivery) + Send + 'static,
    ) -> Self {
        Self {
            update,
            finish: Some(Box::new(finish)),
        }
    }

    pub fn update(&self) -> &AlabGoalRuntimeUpdatedNotification {
        &self.update
    }

    pub fn finish(mut self, outcome: AlabGoalRuntimeDelivery) {
        if let Some(finish) = self.finish.take() {
            finish(outcome);
        }
    }
}

impl Drop for AlabGoalRuntimeSnapshot {
    fn drop(&mut self) {
        if let Some(finish) = self.finish.take() {
            finish(AlabGoalRuntimeDelivery::Cancelled);
        }
    }
}
