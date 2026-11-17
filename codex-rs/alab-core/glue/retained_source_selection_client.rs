// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Distinct S2 codec over the SAME actual client and original Resume policy.
//! Public receipt values are correlation DATA, never Native/UI permission.
use super::*;
#[path = "retained_source_selection_metadata.rs"]
pub(crate) mod metadata;
pub use metadata::side_display::SideDisplayBoundary;
pub use metadata::side_display::SideDisplayItem;
#[derive(Clone)]
pub struct Receipt(
    pub(crate) metadata::Receipt,
    bool,
    Option<SideDisplayBoundary>,
);
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectCompletion {
    receipt: metadata::Receipt,
    complete_memory: bool,
    #[serde(deserialize_with = "required_side_boundary")]
    side_boundary: Option<SideDisplayBoundary>,
}
fn required_side_boundary<'de, D: serde::Deserializer<'de>>(
    reader: D,
) -> Result<Option<SideDisplayBoundary>, D::Error> {
    <Option<SideDisplayBoundary> as serde::Deserialize>::deserialize(reader)
}
impl Receipt {
    pub fn side_display_boundary(&self) -> Option<&SideDisplayBoundary> {
        self.2.as_ref()
    }

    pub fn frontier_data(&self) -> &str {
        &self.0.frontier
    }
    pub fn snapshot_data(&self) -> &str {
        &self.0.snapshot
    }
    pub fn thread_id_data(&self) -> &str {
        &self.0.thread_id
    }
    pub fn task_tools_available(&self) -> bool {
        self.0.task_tools_available
    }
    /// Actual response-owned complete memory cut, including a genuine empty
    /// cut. This DATA grants neither Native currentness nor UI completion.
    pub fn complete_memory_history(&self) -> bool {
        self.1
    }
    pub fn is_primary_presentation(&self) -> bool {
        self.0.presentation == metadata::Presentation::Primary
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
    pub fn side_view_data(&self) -> Option<crate::alab_initial_session_client::SideView> {
        (!self.receipt.is_primary_presentation()).then(|| {
            crate::alab_initial_session_client::SideView {
                schema: 1,
                claim: self.receipt.0.claim.clone(),
                thread_id: self.receipt.thread_id_data().into(),
            }
        })
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}
pub(crate) fn invalid(message: &'static str) -> TypedRequestError {
    TypedRequestError::Transport {
        method: "thread/resume".into(),
        source: IoError::new(ErrorKind::InvalidData, message),
    }
}
pub(crate) async fn send(
    handle: &AppServerRequestHandle,
    mut wire: codex_app_server_protocol::JSONRPCRequest,
    selection: &metadata::Selection,
) -> Result<serde_json::Value, TypedRequestError> {
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(invalid("Source projection requires actual physical client"));
    };
    if wire.method != "thread/resume" || !selection.valid() {
        return Err(invalid("invalid Source selection request"));
    }
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("Source params missing"))?;
    if params.get("threadId").and_then(serde_json::Value::as_str) != Some(selection.thread_id()) {
        return Err(invalid("Source selected Core disagrees"));
    }
    let meta = params
        .entry("_meta")
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| invalid("Source metadata not an object"))?;
    if [
        metadata::KEY,
        "alab.retainedFrontier",
        "alab.retainedConfigured",
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.clientConstructionPrepare",
    ]
    .into_iter()
    .any(|key| meta.contains_key(key))
    {
        return Err(invalid("Source projection metadata namespace occupied"));
    }
    meta.insert(
        metadata::KEY.into(),
        serde_json::to_value(selection).map_err(|source| TypedRequestError::Deserialize {
            method: "thread/resume".into(),
            source,
        })?,
    );
    decode_typed_response("thread/resume", remote.request_json_rpc(wire).await)
}
pub(crate) fn take_receipt(
    value: &mut serde_json::Value,
    project: &metadata::Project,
) -> Result<Receipt, TypedRequestError> {
    let body = value
        .as_object_mut()
        .ok_or_else(|| invalid("Source response not an object"))?;
    let meta = body
        .get_mut("_meta")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("Source result metadata missing"))?;
    let ProjectCompletion {
        receipt,
        complete_memory,
        side_boundary,
    } = serde_json::from_value(
        meta.remove(metadata::RESULT_KEY)
            .ok_or_else(|| invalid("Source selection receipt missing"))?,
    )
    .map_err(|source| TypedRequestError::Deserialize {
        method: "thread/resume".into(),
        source,
    })?;
    if !receipt.valid()
        || receipt.frontier != project.frontier
        || receipt.thread_id != project.thread_id
        || receipt.claim != project.claim
        || receipt.source_operation != project.source_operation
        || receipt.selection != project.selection
    {
        return Err(invalid(
            "Source projection changed actual operation/delivery",
        ));
    }
    if meta.is_empty() {
        body.remove("_meta");
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
        != Some(receipt.thread_id.as_str())
    {
        return Err(invalid("Source typed thread response disagrees"));
    }
    if side_boundary.as_ref().is_some_and(|boundary| {
        !boundary.valid()
            || !complete_memory
            || receipt.presentation != metadata::Presentation::SideConversation
    }) {
        return Err(invalid(
            "Side display cut is malformed or changed actual presentation",
        ));
    }
    Ok(Receipt(receipt, complete_memory, side_boundary))
}
/// The real UI owner retains this exact SDK frontier and construction operation
/// BEFORE this await. Only the first original Resume wire is replaced; all
/// original settings/history/hydration/started conversion still execute.
pub async fn scope_original_resume<T>(
    handle: &AppServerRequestHandle,
    frontier: &codex_alab_aterm::frontend_control::FrontendSelectionFrontier,
    prepared: &ClientConstructionOffer,
    thread_id: &str,
    original: impl std::future::Future<Output = T>,
) -> IoResult<(T, Option<Receipt>)> {
    if frontier.source_operation_data() != prepared.source_operation
        || frontier.selection_data() != prepared.selection
    {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "actual Native Source frontier disagrees with prepared operation",
        ));
    }
    let project = metadata::Project {
        history: metadata::HistoryIntent::CurrentDisplay,
        claim: prepared.claim.clone(),
        source_operation: prepared.source_operation.clone(),
        selection: prepared.selection.clone(),
        frontier: frontier.frontier_data().into(),
        thread_id: thread_id.into(),
        go: frontier.go_data().map(Into::into),
    };
    crate::retained_frontier::original_policy::scope_source_resume(handle, project, original).await
}
async fn complete(
    handle: &AppServerRequestHandle,
    receipt: &Receipt,
    commit: bool,
) -> Result<Receipt, TypedRequestError> {
    let wire = codex_app_server_protocol::JSONRPCRequest {
        id: RequestId::String(format!(
            "alab-source-{:016x}{:016x}",
            codex_alab_core::native_source::connection_request_prefix(),
            codex_alab_core::native_source::connection_request_prefix()
        )),
        method: "thread/resume".into(),
        params: Some(serde_json::json!({"threadId":receipt.thread_id_data()})),
        trace: None,
    };
    let selection = if commit {
        metadata::Selection::Activate(receipt.0.clone())
    } else {
        metadata::Selection::Observe(receipt.0.clone())
    };
    let value = send(handle, wire, &selection).await?;
    let actual: metadata::Receipt = serde_json::from_value(
        value
            .get("_meta")
            .and_then(|meta| meta.get(metadata::RESULT_KEY))
            .cloned()
            .ok_or_else(|| invalid("Source completion receipt missing"))?,
    )
    .map_err(|source| TypedRequestError::Deserialize {
        method: "thread/resume".into(),
        source,
    })?;
    if actual != receipt.0 {
        return Err(invalid("Source completion changed operation/delivery"));
    }
    Ok(Receipt(actual, receipt.1, receipt.2.clone()))
}
pub async fn observe(
    handle: &AppServerRequestHandle,
    receipt: &Receipt,
) -> Result<Observation, TypedRequestError> {
    Ok(Observation {
        receipt: complete(handle, receipt, false).await?,
    })
}
pub async fn activate(
    handle: &AppServerRequestHandle,
    receipt: &Receipt,
) -> Result<Activation, TypedRequestError> {
    Ok(Activation {
        receipt: complete(handle, receipt, true).await?,
    })
}

/// A fresh captured-Core S2 snapshot plus its exact correlation receipt. This
/// owns no Start/Fork conversion and is deliberately not Clone: the original
/// UI keeps its already converted Started object and moves this projection
/// through its actual reconciliation/replay/install path.
pub struct ProjectedExisting {
    response: codex_app_server_protocol::ThreadResumeResponse,
    receipt: Receipt,
}
impl ProjectedExisting {
    pub fn into_parts(self) -> (codex_app_server_protocol::ThreadResumeResponse, Receipt) {
        (self.response, self.receipt)
    }
}
/// Project the exact accepted Core without constructing Resume parameters or
/// running another frontend Started conversion. The caller must supply the
/// complete ThreadResume request from the original UI parameter policy. The
/// server resolves the actual accepted Scope/Arc and serializes its original
/// listener/history tail; it never runs Core Resume, forks or rebuilds a Core.
/// The UI retains its original Start/Fork result separately and reconciles this
/// current snapshot explicitly. Metadata is never a Native/UI grant.
pub async fn project_existing(
    handle: &AppServerRequestHandle,
    frontier: &codex_alab_aterm::frontend_control::FrontendSelectionFrontier,
    prepared: &ClientConstructionOffer,
    original_projection: ClientRequest,
) -> Result<ProjectedExisting, TypedRequestError> {
    if !prepared.valid()
        || frontier.source_operation_data() != prepared.source_operation
        || frontier.selection_data() != prepared.selection
    {
        return Err(invalid(
            "actual Native Source frontier disagrees with prepared operation",
        ));
    }
    if original_projection.method_name() != "thread/resume" {
        return Err(invalid(
            "existing-Core projection requires original ThreadResume request",
        ));
    }
    let wire: codex_app_server_protocol::JSONRPCRequest =
        serde_json::from_value(serde_json::to_value(original_projection).map_err(|source| {
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
        .ok_or_else(|| invalid("original projection thread missing"))?
        .to_owned();
    let project = metadata::Project {
        history: metadata::HistoryIntent::CurrentDisplay,
        claim: prepared.claim.clone(),
        source_operation: prepared.source_operation.clone(),
        selection: prepared.selection.clone(),
        frontier: frontier.frontier_data().into(),
        thread_id,
        go: frontier.go_data().map(Into::into),
    };
    let mut value = send(handle, wire, &metadata::Selection::Project(project.clone())).await?;
    let receipt = take_receipt(&mut value, &project)?;
    let response =
        serde_json::from_value(value).map_err(|source| TypedRequestError::Deserialize {
            method: "thread/resume".into(),
            source,
        })?;
    Ok(ProjectedExisting { response, receipt })
}

#[cfg(test)]
#[path = "retained_source_memory_observation_tests.rs"]
mod memory_observation_tests;

impl SideDisplayBoundary {
    pub fn excludes(&self, turn: &str, item: &str) -> bool {
        self.before
            .binary_search_by(|key| {
                key.turn_id
                    .as_str()
                    .cmp(turn)
                    .then_with(|| key.item_id.as_str().cmp(item))
            })
            .is_ok()
    }
}
