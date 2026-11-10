// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Positive identity evidence for a successful empty account reminder fetch.

use crate::JsonSchema;
use crate::TS;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct AlabAccountSecuritySetupIdentity {
    pub account_id: String,
    pub user_id: String,
}
