// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Distinct S2 correlation DATA. Native SourceTransition/SelectionCommit,
//! actual A, original listener and real UI/App resources remain independent.
use serde::Deserialize;
use serde::Serialize;
pub(crate) const KEY: &str = "alab.sourceSelectionFrontier";
pub(crate) const RESULT_KEY: &str = "alab.sourceSelectionProjection";
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Selection {
    Project(Project),
    Observe(Receipt),
    Activate(Receipt),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Project {
    /// Explicit UI snapshot request DATA, never Native/currentness authority.
    pub history: HistoryIntent,
    pub claim: String,
    pub source_operation: String,
    pub selection: String,
    pub frontier: String,
    pub thread_id: String,
    pub go: Option<String>,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Presentation {
    Primary,
    SideConversation,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    pub schema: u32,
    pub claim: String,
    pub source_operation: String,
    pub selection: String,
    pub frontier: String,
    pub snapshot: String,
    pub thread_id: String,
    pub task_tools_available: bool,
    pub presentation: Presentation,
}
impl Receipt {
    pub(crate) fn valid(&self) -> bool {
        self.schema == 1
            && [
                &self.claim,
                &self.source_operation,
                &self.selection,
                &self.frontier,
                &self.snapshot,
            ]
            .into_iter()
            .all(|value| token(value))
            && !self.thread_id.is_empty()
    }
}
impl Selection {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Project(value) => {
                [
                    &value.claim,
                    &value.source_operation,
                    &value.selection,
                    &value.frontier,
                ]
                .into_iter()
                .all(|value| token(value))
                    && !value.thread_id.is_empty()
                    && value.go.as_deref().is_none_or(token)
            }
            Self::Observe(value) | Self::Activate(value) => value.valid(),
        }
    }
    pub(crate) fn thread_id(&self) -> &str {
        match self {
            Self::Project(value) => &value.thread_id,
            Self::Observe(value) | Self::Activate(value) => &value.thread_id,
        }
    }
}
fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Original follows supplied history options; currentDisplay additionally asks
/// for the authoritative complete memory cut when no durable paging is available.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum HistoryIntent {
    Original,
    CurrentDisplay,
}

#[path = "retained_side_display_metadata.rs"]
pub(crate) mod side_display;
