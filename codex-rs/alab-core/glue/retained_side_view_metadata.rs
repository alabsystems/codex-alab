// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! View/operation selectors are DATA. Only actual current Native A and an
//! existing installed private Side owner admit close/observation bookkeeping.
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
pub(crate) const KEY: &str = "alab.sideView";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideView {
    pub schema: u8,
    pub claim: String,
    pub thread_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Selection {
    Close {
        view: SideView,
        operation: String,
    },
    Observe {
        view: SideView,
        operation: String,
    },
    ReadResult {
        view: SideView,
        operation: String,
        parameters: Value,
    },
    AcknowledgeResult {
        view: SideView,
        operation: String,
        parameters: Value,
    },
}
fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
impl SideView {
    pub fn valid(&self) -> bool {
        self.schema == 1 && token(&self.claim) && !self.thread_id.is_empty()
    }
}
impl Selection {
    pub(crate) fn view(&self) -> &SideView {
        match self {
            Self::Close { view, .. }
            | Self::Observe { view, .. }
            | Self::ReadResult { view, .. }
            | Self::AcknowledgeResult { view, .. } => view,
        }
    }
    pub(crate) fn operation(&self) -> &str {
        match self {
            Self::Close { operation, .. }
            | Self::Observe { operation, .. }
            | Self::ReadResult { operation, .. }
            | Self::AcknowledgeResult { operation, .. } => operation,
        }
    }
    pub(crate) fn valid(&self) -> bool {
        self.view().valid()
            && token(self.operation())
            && match self {
                Self::ReadResult {
                    view, parameters, ..
                }
                | Self::AcknowledgeResult {
                    view, parameters, ..
                } => {
                    parameters.get("threadId").and_then(Value::as_str)
                        == Some(view.thread_id.as_str())
                }
                Self::Close { .. } | Self::Observe { .. } => true,
            }
    }
}

pub(crate) const OBSERVATION_KEY: &str = "alab.sideViewObservation";
pub(crate) use codex_alab_core::retained_side_cleanup as observation;

pub(crate) const READ_RESULT_KEY: &str = "alab.sideViewResult";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadResult {
    pub(super) schema: u8,
    pub(super) view: SideView,
    pub(super) operation: String,
    pub(super) reply: ReadReply,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum ReadReply {
    Success {
        status: codex_app_server_protocol::ThreadUnsubscribeStatus,
    },
    Rejected {
        code: i64,
        message: String,
        #[serde(rename = "hadData")]
        had_data: bool,
        value: Value,
    },
}
