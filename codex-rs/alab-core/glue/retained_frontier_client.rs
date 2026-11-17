// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Existing thread/resume RPC on the SAME actual retained client. Metadata
//! selects its protected exact-Core path; it never grants Native/local roles.
use super::*;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::ThreadResumeResponse;
use serde::de::Error as _;
#[path = "retained_frontier_metadata.rs"]
mod metadata;

#[derive(Clone)]
pub struct Receipt(metadata::Receipt, bool);
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectCompletion {
    receipt: metadata::Receipt,
    complete_memory: bool,
}
impl Receipt {
    pub fn frontier_data(&self) -> &str {
        &self.0.frontier
    }
    pub fn snapshot_data(&self) -> &str {
        &self.0.snapshot
    }
    pub fn thread_id_data(&self) -> &str {
        &self.0.thread_id
    }
    /// Actual backend observation carried with this same projection receipt.
    /// It is not inferred from OwnedLocal, URL, JSON transport or target labels.
    pub fn task_tools_available(&self) -> bool {
        self.0.task_tools_available
    }
    /// Actual response-owned complete memory cut, including a genuine empty
    /// cut. This DATA grants neither Native currentness nor UI completion.
    pub fn complete_memory_history(&self) -> bool {
        self.1
    }
}
impl std::fmt::Debug for Receipt {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RetainedProjectionReceipt(<Source data; requires real native/UI path>)")
    }
}
pub struct Projection {
    pub response: ThreadResumeResponse,
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
pub struct Activation {
    receipt: Receipt,
}
impl Activation {
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}

fn invalid(message: &'static str) -> TypedRequestError {
    TypedRequestError::Deserialize {
        method: "thread/resume".into(),
        source: serde_json::Error::custom(message),
    }
}
async fn send(
    handle: &AppServerRequestHandle,
    mut wire: JSONRPCRequest,
    selection: &metadata::Selection,
) -> Result<serde_json::Value, TypedRequestError> {
    if wire.method != "thread/resume" || !selection.valid() {
        return Err(invalid("invalid retained projection selection"));
    }
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(TypedRequestError::Transport {
            method: wire.method,
            source: IoError::new(
                ErrorKind::Unsupported,
                "retained frontier requires its actual physical client",
            ),
        });
    };
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("retained parameters missing"))?;
    if params.get("threadId").and_then(serde_json::Value::as_str) != Some(selection.thread_id()) {
        return Err(invalid("retained primary selection disagrees"));
    }
    let meta = params
        .entry(metadata::META)
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| invalid("retained metadata is not an object"))?;
    if meta.contains_key(metadata::KEY) {
        return Err(invalid("retained metadata namespace occupied"));
    }
    meta.insert(
        metadata::KEY.into(),
        serde_json::to_value(selection).map_err(|source| TypedRequestError::Deserialize {
            method: "thread/resume".into(),
            source,
        })?,
    );
    let response = remote.request_json_rpc(wire).await;
    decode_typed_response("thread/resume", response)
}
pub async fn project(
    handle: &AppServerRequestHandle,
    original: ClientRequest,
    frontier: &str,
) -> Result<Projection, TypedRequestError> {
    if original.method_name() != "thread/resume" {
        return Err(invalid(
            "retained projection requires the original resume request",
        ));
    }
    // Use the original exhaustive serialization, preserving request id and
    // history arguments. The backend deliberately never applies stale config.
    let wire: JSONRPCRequest =
        serde_json::from_value(serde_json::to_value(original).map_err(|source| {
            TypedRequestError::Deserialize {
                method: "thread/resume".into(),
                source,
            }
        })?)
        .map_err(|source| TypedRequestError::Deserialize {
            method: "thread/resume".into(),
            source,
        })?;
    let thread_id = wire
        .params
        .as_ref()
        .and_then(|params| params.get("threadId"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("retained primary id missing"))?
        .to_owned();
    let selection = metadata::Selection::Project(metadata::Project {
        history: metadata::HistoryIntent::CurrentDisplay,
        frontier: frontier.into(),
        thread_id: thread_id.clone(),
        candidate: None,
    });
    decode_projection(send(handle, wire, &selection).await?, frontier, &thread_id)
}
fn take_projection_receipt(
    value: &mut serde_json::Value,
    frontier: &str,
    thread_id: &str,
) -> Result<Receipt, TypedRequestError> {
    let body = value
        .as_object_mut()
        .ok_or_else(|| invalid("retained response is not an object"))?;
    let meta = body
        .get_mut(metadata::META)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("retained response metadata missing"))?;
    let ProjectCompletion {
        receipt,
        complete_memory,
    } = serde_json::from_value(
        meta.remove(metadata::RESULT_KEY)
            .ok_or_else(|| invalid("retained response receipt missing"))?,
    )
    .map_err(|source| TypedRequestError::Deserialize {
        method: "thread/resume".into(),
        source,
    })?;
    if !receipt.valid() || receipt.frontier != frontier || receipt.thread_id != thread_id {
        return Err(invalid("retained response receipt disagrees"));
    }
    if meta.is_empty() {
        body.remove(metadata::META);
    }
    if complete_memory
        && (body
            .get("thread")
            .and_then(|thread| thread.get("turns"))
            .is_none_or(|turns| !turns.is_array())
            || ["turnsBackwardsCursor", "itemsBackwardsCursor"]
                .into_iter()
                .any(|key| body.get(key).is_some_and(|cursor| !cursor.is_null())))
    {
        return Err(invalid(
            "complete memory snapshot is missing turns or has a durable cursor",
        ));
    }
    if body
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(serde_json::Value::as_str)
        != Some(thread_id)
    {
        return Err(invalid("retained response belongs to another primary"));
    }
    Ok(Receipt(receipt, complete_memory))
}
fn decode_projection(
    mut value: serde_json::Value,
    frontier: &str,
    thread_id: &str,
) -> Result<Projection, TypedRequestError> {
    let receipt = take_projection_receipt(&mut value, frontier, thread_id)?;
    let response: ThreadResumeResponse =
        serde_json::from_value(value).map_err(|source| TypedRequestError::Deserialize {
            method: "thread/resume".into(),
            source,
        })?;
    Ok(Projection { response, receipt })
}
/// Record the genuine backend Native activation stage while its SAME Source
/// Hold remains closed. This completion DATA cannot release it or grant App.
pub async fn observe(
    handle: &AppServerRequestHandle,
    receipt: &Receipt,
) -> Result<Observation, TypedRequestError> {
    let id = format!(
        "alab-retained-observe-{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    );
    let wire = JSONRPCRequest {
        id: RequestId::String(id),
        method: "thread/resume".into(),
        params: Some(serde_json::json!({"threadId":receipt.thread_id_data()})),
        trace: None,
    };
    let actual = decode_activation(
        send(
            handle,
            wire,
            &metadata::Selection::Observe(receipt.0.clone()),
        )
        .await?,
        receipt,
    )?;
    Ok(Observation {
        receipt: actual.receipt,
    })
}
pub async fn activate(
    handle: &AppServerRequestHandle,
    receipt: &Receipt,
) -> Result<Activation, TypedRequestError> {
    let id = format!(
        "alab-retained-frontier-{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    );
    // The actual UI first observes the stage and promotes real App custody.
    // The backend's final Native query enforces it; this DATA cannot substitute.
    // This is an acknowledgment on the same client, never a reconstructed
    // start/resume config. The protected server path cannot enter resume_inner.
    let wire = JSONRPCRequest {
        id: RequestId::String(id),
        method: "thread/resume".into(),
        params: Some(serde_json::json!({"threadId":receipt.thread_id_data()})),
        trace: None,
    };
    decode_activation(
        send(
            handle,
            wire,
            &metadata::Selection::Activate(receipt.0.clone()),
        )
        .await?,
        receipt,
    )
}
fn decode_activation(
    mut value: serde_json::Value,
    receipt: &Receipt,
) -> Result<Activation, TypedRequestError> {
    let actual = value
        .as_object_mut()
        .and_then(|body| body.get_mut(metadata::META))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|meta| meta.remove(metadata::RESULT_KEY))
        .ok_or_else(|| invalid("retained activation metadata missing"))?;
    let actual: metadata::Receipt =
        serde_json::from_value(actual).map_err(|source| TypedRequestError::Deserialize {
            method: "thread/resume".into(),
            source,
        })?;
    if actual != receipt.0 {
        return Err(invalid("retained activation completion disagrees"));
    }
    Ok(Activation {
        receipt: Receipt(actual, receipt.1),
    })
}

#[cfg(test)]
#[path = "retained_frontier_client_tests.rs"]
mod tests;

#[path = "retained_frontier_original_policy.rs"]
pub(crate) mod original_policy;
pub use original_policy::active as original_policy_active;
pub use original_policy::complete_memory_history as original_complete_memory_history;
pub use original_policy::scope_original_resume;
