// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Configured-runtime/empty-selected-slot correlation DATA. These values never
//! establish an empty slot, a Core, Native Go/currentness or effect permission.
use serde::Deserialize;
use serde::Serialize;
pub(crate) const META: &str = "_meta";
pub(crate) const KEY: &str = "alab.retainedConfigured";
pub(crate) const RESULT_KEY: &str = "alab.retainedConfiguration";
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Selection {
    Project { frontier: String },
    Observe(Receipt),
    Commit(Receipt),
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    pub frontier: String,
    pub snapshot: String,
    pub runtime: String,
    pub version: u64,
}
impl Receipt {
    pub(crate) fn valid(&self) -> bool {
        token(&self.frontier) && token(&self.snapshot) && token(&self.runtime)
    }
}
impl Selection {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Project { frontier } => token(frontier),
            Self::Observe(value) | Self::Commit(value) => value.valid(),
        }
    }
}
fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
