// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Closed checkpoint DATA preserves error.data absence versus explicit null.
use super::*;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum SavedReply {
    Success {
        value: serde_json::Value,
    },
    Error {
        code: i64,
        message: String,
        #[serde(rename = "hadData")]
        had_data: bool,
        value: serde_json::Value,
    },
}
impl SavedReply {
    pub(super) fn capture(reply: &RequestResult) -> Self {
        match reply {
            Ok(value) => Self::Success {
                value: value.clone(),
            },
            Err(error) => Self::Error {
                code: error.code,
                message: error.message.clone(),
                had_data: error.data.is_some(),
                value: error.data.clone().unwrap_or(serde_json::Value::Null),
            },
        }
    }
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::Success { .. } => true,
            Self::Error {
                had_data, value, ..
            } => *had_data || value.is_null(),
        }
    }
    pub(super) fn original(&self) -> RequestResult {
        match self {
            Self::Success { value } => Ok(value.clone()),
            Self::Error {
                code,
                message,
                had_data,
                value,
            } => Err(codex_app_server_protocol::JSONRPCErrorError {
                code: *code,
                message: message.clone(),
                data: had_data.then(|| value.clone()),
            }),
        }
    }
    pub(super) fn succeeded(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

pub(super) fn observation_value(
    value: serde_json::Value,
    key: &str,
) -> IoResult<serde_json::Value> {
    let body = value
        .as_object()
        .filter(|body| body.len() == 1)
        .ok_or_else(|| invalid("Source observation body changed"))?;
    let meta = body
        .get("_meta")
        .and_then(serde_json::Value::as_object)
        .filter(|meta| meta.len() == 1)
        .ok_or_else(|| invalid("Source observation metadata changed"))?;
    meta.get(key)
        .cloned()
        .ok_or_else(|| invalid("Source observation key changed"))
}
