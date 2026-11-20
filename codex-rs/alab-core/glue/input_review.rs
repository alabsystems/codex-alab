// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// The decision made after Core applied its input hooks while a finding was pending.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputReview {
    Corrected(String),
    Held(String),
}
