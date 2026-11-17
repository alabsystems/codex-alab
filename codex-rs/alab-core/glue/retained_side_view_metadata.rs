// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! View/operation selectors are DATA. Only actual current Native A and an
//! existing installed private Side owner admit close/observation bookkeeping.
use serde::Deserialize;
use serde::Serialize;
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
    Close { view: SideView, operation: String },
    Observe { view: SideView, operation: String },
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
            Self::Close { view, .. } | Self::Observe { view, .. } => view,
        }
    }
    pub(crate) fn operation(&self) -> &str {
        match self {
            Self::Close { operation, .. } | Self::Observe { operation, .. } => operation,
        }
    }
    pub(crate) fn valid(&self) -> bool {
        self.view().valid() && token(self.operation())
    }
}

pub(crate) const OBSERVATION_KEY: &str = "alab.sideViewObservation";
#[path = "retained_side_cleanup_observation.rs"]
pub(crate) mod observation;
