// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Existing actual config/read transport and exact typed response. Completion
//! means configured UI observation only; the original automatic Hold stays shut.
use super::*;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::JSONRPCRequest;
use serde::de::Error as _;
#[path = "retained_configured_metadata.rs"]
mod metadata;
#[derive(Clone)]
pub struct Receipt(metadata::Receipt);
impl Receipt {
    pub fn frontier_data(&self) -> &str {
        &self.0.frontier
    }
    pub fn snapshot_data(&self) -> &str {
        &self.0.snapshot
    }
    pub fn runtime_data(&self) -> &str {
        &self.0.runtime
    }
    pub fn slot_version_data(&self) -> u64 {
        self.0.version
    }
}
impl std::fmt::Debug for Receipt {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("ConfiguredUiReceipt(<DATA; original Hold stays closed>)")
    }
}
pub struct Projection {
    pub response: ConfigReadResponse,
    receipt: Receipt,
}
impl Projection {
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}
pub struct Observation {
    receipt: Receipt,
}
impl Observation {
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}
/// Does not open Source admission or release startup submissions. A later
/// actual selected-Core/UI frontier is still required for automatic work.
pub struct ConfigurationCommit {
    receipt: Receipt,
}
impl ConfigurationCommit {
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}
fn invalid(message: &'static str) -> TypedRequestError {
    TypedRequestError::Deserialize {
        method: "config/read".into(),
        source: serde_json::Error::custom(message),
    }
}
async fn send(
    handle: &AppServerRequestHandle,
    mut wire: JSONRPCRequest,
    selection: &metadata::Selection,
) -> Result<serde_json::Value, TypedRequestError> {
    if wire.method != "config/read" || !selection.valid() {
        return Err(invalid("invalid configured projection selection"));
    }
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(TypedRequestError::Transport {
            method: wire.method,
            source: IoError::new(
                ErrorKind::Unsupported,
                "configured frontier requires actual physical client",
            ),
        });
    };
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("configured original parameters missing"))?;
    let meta = params
        .entry(metadata::META)
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| invalid("configured metadata is not an object"))?;
    if [
        metadata::KEY,
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.retainedFrontier",
        "alab.clientConstructionPrepare",
        "alab.sourceSelectionFrontier",
    ]
    .iter()
    .any(|key| meta.contains_key(*key))
    {
        return Err(invalid("configured metadata namespace occupied"));
    }
    meta.insert(
        metadata::KEY.into(),
        serde_json::to_value(selection).map_err(|source| TypedRequestError::Deserialize {
            method: "config/read".into(),
            source,
        })?,
    );
    decode_typed_response("config/read", remote.request_json_rpc(wire).await)
}
pub async fn project(
    handle: &AppServerRequestHandle,
    original: ClientRequest,
    frontier: &str,
) -> Result<Projection, TypedRequestError> {
    if original.method_name() != "config/read" {
        return Err(invalid(
            "configured projection requires original config/read",
        ));
    }
    let wire: JSONRPCRequest =
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
    let value = send(
        handle,
        wire,
        &metadata::Selection::Project {
            frontier: frontier.into(),
        },
    )
    .await?;
    decode_projection(value, frontier)
}
fn remove_receipt(value: &mut serde_json::Value) -> Result<Receipt, TypedRequestError> {
    let body = value
        .as_object_mut()
        .ok_or_else(|| invalid("configured result is not an object"))?;
    let meta = body
        .get_mut(metadata::META)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("configured result metadata missing"))?;
    let receipt: metadata::Receipt = serde_json::from_value(
        meta.remove(metadata::RESULT_KEY)
            .ok_or_else(|| invalid("configured completion missing"))?,
    )
    .map_err(|source| TypedRequestError::Deserialize {
        method: "config/read".into(),
        source,
    })?;
    if !receipt.valid() {
        return Err(invalid("invalid configured completion"));
    }
    if meta.is_empty() {
        body.remove(metadata::META);
    }
    Ok(Receipt(receipt))
}
fn decode_projection(
    mut value: serde_json::Value,
    frontier: &str,
) -> Result<Projection, TypedRequestError> {
    let receipt = remove_receipt(&mut value)?;
    if receipt.frontier_data() != frontier {
        return Err(invalid("configured frontier disagrees"));
    }
    let response =
        serde_json::from_value(value).map_err(|source| TypedRequestError::Deserialize {
            method: "config/read".into(),
            source,
        })?;
    Ok(Projection { response, receipt })
}
async fn complete(
    handle: &AppServerRequestHandle,
    receipt: &Receipt,
    commit: bool,
) -> Result<Receipt, TypedRequestError> {
    let id = format!(
        "alab-configured-{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    );
    let wire = JSONRPCRequest {
        id: RequestId::String(id),
        method: "config/read".into(),
        params: Some(serde_json::json!({})),
        trace: None,
    };
    let selection = if commit {
        metadata::Selection::Commit(receipt.0.clone())
    } else {
        metadata::Selection::Observe(receipt.0.clone())
    };
    let mut value = send(handle, wire, &selection).await?;
    let actual = remove_receipt(&mut value)?;
    if actual.0 != receipt.0 {
        return Err(invalid("configured completion disagrees"));
    }
    Ok(actual)
}
pub async fn observe(
    handle: &AppServerRequestHandle,
    receipt: &Receipt,
) -> Result<Observation, TypedRequestError> {
    Ok(Observation {
        receipt: complete(handle, receipt, false).await?,
    })
}
pub async fn commit(
    handle: &AppServerRequestHandle,
    receipt: &Receipt,
) -> Result<ConfigurationCommit, TypedRequestError> {
    Ok(ConfigurationCommit {
        receipt: complete(handle, receipt, true).await?,
    })
}

#[cfg(test)]
#[path = "retained_configured_client_tests.rs"]
mod tests;

#[path = "retained_configured_original.rs"]
pub(crate) mod original;
pub use original::ConfiguredProjection;
pub use original::ConfiguredReplay;
pub use original::project_scope;
