// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Frozen managed-policy DATA from the actual retained backend loader. This
//! wire owns no EMA binding, source/reader handle, token, or Native permission.
use codex_config::CloudConfigBundle;
use codex_config::CloudConfigBundleLoadErrorCode;
use codex_config::CloudConfigBundleSnapshot;
use codex_config::ConfigServer;
use serde::Deserialize;
use serde::Serialize;
pub const KEY: &str = "alab.retainedReader";
pub const RESULT_KEY: &str = "alab.retainedReaderSnapshot";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub service: String,
    pub operation: String,
    pub kind: ReadKind,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReadKind {
    Managed,
    Account,
}
impl Selection {
    pub fn valid(&self) -> bool {
        token(&self.service) && token(&self.operation)
    }
}
fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub schema: u32,
    pub service: String,
    pub operation: String,
    pub kind: ReadKind,
    #[serde(deserialize_with = "required")]
    pub cloud: Option<ManagedCloudData>,
    pub account: AccountData,
}

/// Selected account identifiers and original UI predicates only. No token,
/// source/reader handle, network binding or Native permission is serialized.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountData {
    pub(super) authenticated: bool,
    pub(super) chatgpt_auth: bool,
    pub(super) managed_chatgpt: bool,
    pub(super) fedramp: bool,
    #[serde(deserialize_with = "required")]
    pub(super) account: Option<String>,
    #[serde(deserialize_with = "required")]
    pub(super) user: Option<String>,
}
impl AccountData {
    pub fn from_actual(auth: Option<&codex_login::CodexAuth>) -> Self {
        Self {
            authenticated: auth.is_some(),
            chatgpt_auth: auth.is_some_and(codex_login::CodexAuth::is_chatgpt_auth),
            managed_chatgpt: matches!(auth, Some(codex_login::CodexAuth::Chatgpt(_))),
            fedramp: auth.is_some_and(codex_login::CodexAuth::is_fedramp_account),
            account: auth.and_then(codex_login::CodexAuth::get_account_id),
            user: auth.and_then(codex_login::CodexAuth::get_chatgpt_user_id),
        }
    }
}
impl std::fmt::Debug for AccountData {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("AccountData(<private account observation; no credential>)")
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedCloudData {
    pub(super) outcome: Outcome,
    #[serde(deserialize_with = "required")]
    pub(super) config_server: Option<ConfigServer>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum Outcome {
    Loaded {
        #[serde(deserialize_with = "required")]
        bundle: Option<CloudConfigBundle>,
    },
    Failed {
        code: ErrorCode,
        #[serde(deserialize_with = "required")]
        status: Option<u16>,
        message: String,
    },
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ErrorCode {
    Auth,
    Timeout,
    RequestFailed,
    InvalidBundle,
    Internal,
}
impl ManagedCloudData {
    pub fn from_actual(snapshot: &CloudConfigBundleSnapshot) -> Self {
        let outcome = match &snapshot.bundle {
            Ok(bundle) => Outcome::Loaded {
                bundle: bundle.clone(),
            },
            Err(error) => Outcome::Failed {
                code: match error.code() {
                    CloudConfigBundleLoadErrorCode::Auth => ErrorCode::Auth,
                    CloudConfigBundleLoadErrorCode::Timeout => ErrorCode::Timeout,
                    CloudConfigBundleLoadErrorCode::RequestFailed => ErrorCode::RequestFailed,
                    CloudConfigBundleLoadErrorCode::InvalidBundle => ErrorCode::InvalidBundle,
                    CloudConfigBundleLoadErrorCode::Internal => ErrorCode::Internal,
                },
                status: error.status_code(),
                message: error.to_string(),
            },
        };
        Self {
            outcome,
            config_server: snapshot.config_server.clone(),
        }
    }
}
impl std::fmt::Debug for ManagedCloudData {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("ManagedCloudData(<private frozen policy; no credential authority>)")
    }
}

fn required<'de, D, T>(decoder: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(decoder)
}
