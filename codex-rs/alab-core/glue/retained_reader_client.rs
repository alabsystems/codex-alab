// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Managed bundle DATA on the SAME actual Remote channel. No credential,
//! AuthManager, token or Native permission is decoded from these results.
use super::*;
#[path = "retained_reader_metadata.rs"]
mod metadata;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::JSONRPCRequest;
use codex_config::CloudConfigBundle;
use codex_config::CloudConfigBundleLoadError;
use codex_config::CloudConfigBundleLoadErrorCode;
use codex_config::CloudConfigBundleLoader;
use codex_config::ConfigServer;
pub use metadata::AccountData;
use metadata::ErrorCode;
pub use metadata::ManagedCloudData;
use metadata::Outcome;
use metadata::ReadKind;
use metadata::Selection;
use metadata::Snapshot;
use std::io;

pub struct ManagedSnapshot {
    pub response: ConfigReadResponse,
    cloud: ManagedCloudData,
}
impl ManagedSnapshot {
    pub fn cloud_data(&self) -> &ManagedCloudData {
        &self.cloud
    }
    pub fn into_cloud_data(self) -> ManagedCloudData {
        self.cloud
    }
}
fn invalid(message: &'static str) -> TypedRequestError {
    use serde::de::Error as _;
    TypedRequestError::Deserialize {
        method: "config/read".into(),
        source: serde_json::Error::custom(message),
    }
}
pub async fn read_managed_snapshot(
    handle: &AppServerRequestHandle,
    service: &str,
    original: ClientRequest,
) -> Result<ManagedSnapshot, TypedRequestError> {
    let decoded = read_snapshot(handle, service, original, metadata::ReadKind::Managed).await?;
    let cloud = decoded
        .snapshot
        .cloud
        .ok_or_else(|| invalid("managed policy result missing"))?;
    Ok(ManagedSnapshot {
        response: decoded.response,
        cloud,
    })
}
pub async fn read_account(
    handle: &AppServerRequestHandle,
    service: &str,
    original: ClientRequest,
) -> Result<AccountData, TypedRequestError> {
    Ok(
        read_snapshot(handle, service, original, metadata::ReadKind::Account)
            .await?
            .snapshot
            .account,
    )
}
struct Decoded {
    response: ConfigReadResponse,
    snapshot: metadata::Snapshot,
}
async fn read_snapshot(
    handle: &AppServerRequestHandle,
    service: &str,
    original: ClientRequest,
    kind: metadata::ReadKind,
) -> Result<Decoded, TypedRequestError> {
    if original.method_name() != "config/read" {
        return Err(invalid(
            "retained managed snapshot requires original config/read",
        ));
    }
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(TypedRequestError::Transport {
            method: "config/read".into(),
            source: IoError::new(
                ErrorKind::Unsupported,
                "retained managed snapshot requires physical Remote IO",
            ),
        });
    };
    let selection = metadata::Selection {
        service: service.into(),
        operation: format!(
            "{:016x}{:016x}",
            codex_alab_core::native_source::connection_request_prefix(),
            codex_alab_core::native_source::connection_request_prefix()
        ),
        kind,
    };
    if !selection.valid() {
        return Err(invalid("retained managed service selector invalid"));
    }
    let mut wire: JSONRPCRequest =
        serde_json::from_value(serde_json::to_value(original).map_err(|source| {
            TypedRequestError::Deserialize {
                method: "config/read".into(),
                source,
            }
        })?)
        .map_err(|source| TypedRequestError::Deserialize {
            method: "config/read".into(),
            source,
        })?;
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("original managed config/read parameters missing"))?;
    let meta = params
        .entry("_meta")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("original managed metadata is not an object"))?;
    if [
        metadata::KEY,
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.retainedConfigured",
        "alab.retainedFrontier",
        "alab.clientConstructionPrepare",
        "alab.sourceSelectionFrontier",
    ]
    .into_iter()
    .any(|key| meta.contains_key(key))
    {
        return Err(invalid("original managed reader namespace occupied"));
    }
    meta.insert(
        metadata::KEY.into(),
        serde_json::to_value(&selection).map_err(|source| TypedRequestError::Deserialize {
            method: "config/read".into(),
            source,
        })?,
    );
    let value = decode_typed_response::<serde_json::Value>(
        "config/read",
        remote.request_json_rpc(wire).await,
    )?;
    decode_snapshot(value, &selection)
}
fn decode_snapshot(
    mut value: serde_json::Value,
    selection: &metadata::Selection,
) -> Result<Decoded, TypedRequestError> {
    let body = value
        .as_object_mut()
        .ok_or_else(|| invalid("managed snapshot result is not an object"))?;
    let meta = body
        .get_mut("_meta")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("managed snapshot metadata missing"))?;
    let snapshot: metadata::Snapshot = serde_json::from_value(
        meta.remove(metadata::RESULT_KEY)
            .ok_or_else(|| invalid("managed snapshot data missing"))?,
    )
    .map_err(|source| TypedRequestError::Deserialize {
        method: "config/read".into(),
        source,
    })?;
    if !snapshot.validates(selection) {
        return Err(invalid("managed snapshot correlation disagrees"));
    }
    if meta.is_empty() {
        body.remove("_meta");
    }
    let response =
        serde_json::from_value(value).map_err(|source| TypedRequestError::Deserialize {
            method: "config/read".into(),
            source,
        })?;
    Ok(Decoded { response, snapshot })
}

impl Snapshot {
    pub fn validates(&self, selected: &Selection) -> bool {
        self.schema == 1
            && selected.valid()
            && self.service == selected.service
            && self.operation == selected.operation
            && self.kind == selected.kind
            && (self.cloud.is_some() == (self.kind == ReadKind::Managed))
            && self.account.valid_data()
    }
}
impl AccountData {
    fn valid_data(&self) -> bool {
        (self.authenticated
            || (!self.chatgpt_auth
                && !self.managed_chatgpt
                && !self.fedramp
                && self.account.is_none()
                && self.user.is_none()))
            && (!self.managed_chatgpt || self.chatgpt_auth)
    }
    pub fn authenticated(&self) -> bool {
        self.authenticated
    }
    pub fn preference_user(&self) -> Option<String> {
        match &self.user {
            Some(user) => Some(format!("chatgpt:{user}")),
            None if self.chatgpt_auth => None,
            None => Some("local".into()),
        }
    }
    pub fn identity(&self) -> Option<(&str, &str)> {
        Some((self.account.as_deref()?, self.user.as_deref()?))
    }
    pub fn notice_identity(&self) -> Option<(&str, &str)> {
        if !self.managed_chatgpt || self.fedramp {
            return None;
        }
        self.identity()
    }
}

impl ManagedCloudData {
    fn result(&self) -> Result<Option<CloudConfigBundle>, CloudConfigBundleLoadError> {
        match &self.outcome {
            Outcome::Loaded { bundle } => Ok(bundle.clone()),
            Outcome::Failed {
                code,
                status,
                message,
            } => Err(CloudConfigBundleLoadError::new(
                match code {
                    ErrorCode::Auth => CloudConfigBundleLoadErrorCode::Auth,
                    ErrorCode::Timeout => CloudConfigBundleLoadErrorCode::Timeout,
                    ErrorCode::RequestFailed => CloudConfigBundleLoadErrorCode::RequestFailed,
                    ErrorCode::InvalidBundle => CloudConfigBundleLoadErrorCode::InvalidBundle,
                    ErrorCode::Internal => CloudConfigBundleLoadErrorCode::Internal,
                },
                *status,
                message.clone(),
            )),
        }
    }
    pub fn frozen_loader(&self) -> CloudConfigBundleLoader {
        let result = self.result();
        let loader = CloudConfigBundleLoader::new(async move { result });
        match &self.config_server {
            Some(config) => loader.with_config_server(config.clone()),
            None => loader,
        }
    }
    /// Successful frozen DATA may update the next frontend configuration.
    /// A failed load remains an actual error; it never substitutes an empty bundle.
    pub fn successful_parts(
        &self,
    ) -> io::Result<(Option<CloudConfigBundle>, Option<ConfigServer>)> {
        Ok((
            self.result().map_err(io::Error::other)?,
            self.config_server.clone(),
        ))
    }
}

#[cfg(test)]
#[path = "retained_reader_metadata_tests.rs"]
mod metadata_tests;

#[cfg(test)]
#[path = "retained_reader_client_tests.rs"]
mod tests;
