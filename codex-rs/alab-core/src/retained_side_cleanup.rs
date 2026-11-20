// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Source bookkeeping DATA only. Unowned says no current operation resource;
//! it never says there were no historical effects or supplies Native rights.
use serde::Deserialize;
use serde::Serialize;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    schema: u8,
    operation: String,
    claim: String,
    thread_id: String,
    state: State,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Observed,
    Unowned,
}
impl Observation {
    pub fn new(operation: String, claim: String, thread_id: String, state: State) -> Self {
        Self {
            schema: 1,
            operation,
            claim,
            thread_id,
            state,
        }
    }

    pub fn matches(&self, operation: &str, claim: &str, thread: &str) -> bool {
        self.schema == 1
            && self.operation == operation
            && self.claim == claim
            && self.thread_id == thread
    }
}

impl<'de> Deserialize<'de> for State {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        match String::deserialize(reader)?.as_str() {
            "observed" => Ok(Self::Observed),
            "unowned" => Ok(Self::Unowned),
            _ => Err(serde::de::Error::custom("unknown Source observation state")),
        }
    }
}
