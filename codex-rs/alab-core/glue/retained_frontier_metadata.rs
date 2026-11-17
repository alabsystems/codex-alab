// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source correlation DATA shared by the real client/server codec. Native A,
//! constructor/currentness/Go checks are independent; no value grants authority.
use serde::Deserialize;
use serde::Serialize;
pub(crate) const META: &str = "_meta";
pub(crate) const KEY: &str = "alab.retainedFrontier";
pub(crate) const RESULT_KEY: &str = "alab.retainedProjection";
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
    pub frontier: String,
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    pub frontier: String,
    pub snapshot: String,
    pub thread_id: String,
    /// Source observation of this exact accepted Scope/Core binding, not a grant.
    pub task_tools_available: bool,
}
impl Receipt {
    pub fn valid(&self) -> bool {
        token(&self.frontier) && token(&self.snapshot) && !self.thread_id.is_empty()
    }
}
impl Selection {
    pub fn valid(&self) -> bool {
        match self {
            Self::Project(value) => {
                token(&value.frontier)
                    && !value.thread_id.is_empty()
                    && value.candidate.as_deref().is_none_or(token)
            }
            Self::Observe(value) | Self::Activate(value) => value.valid(),
        }
    }
    pub fn thread_id(&self) -> &str {
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
