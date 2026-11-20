// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original approval view DATA. Restore borrows actual original callbacks and
//! keeps their replay-created choices/actions, AppEventSender and new requests.
use super::*;
use crate::app::app_server_requests::alab_retained_owners::ApprovalOwner;
use crate::bottom_pane::list_selection_view::alab_retained_approval_list::ListData;
use serde::Deserialize;
use serde::Serialize;
use serde_json::value::RawValue;
use std::io;
use std::os::unix::ffi::OsStrExt;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum RequestData {
    Exec {
        kind: CommandExecutionApprovalKind,
        thread_id: ThreadId,
        thread_label: Option<String>,
        id: String,
        environment_id: Option<String>,
        command: Vec<String>,
        reason: Option<String>,
        available_decisions: Vec<CommandExecutionApprovalDecision>,
        network_approval_context: Option<NetworkApprovalContext>,
        additional_permissions: Option<AdditionalPermissionProfile>,
    },
    Permissions {
        thread_id: ThreadId,
        thread_label: Option<String>,
        call_id: String,
        environment_id: Option<String>,
        reason: Option<String>,
        permissions: RequestPermissionProfile,
    },
    ApplyPatch {
        thread_id: ThreadId,
        thread_label: Option<String>,
        id: String,
        reason: Option<String>,
        cwd: Vec<u8>,
        changes: Vec<ChangeData>,
    },
    McpElicitation {
        thread_id: ThreadId,
        thread_label: Option<String>,
        server_name: String,
        request_id: RequestId,
        message: String,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChangeData {
    path: Vec<u8>,
    change: FileData,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum FileData {
    Add {
        content: String,
    },
    Delete {
        content: String,
    },
    Update {
        unified_diff: String,
        move_path: Option<Vec<u8>>,
    },
}
impl RequestData {
    pub(crate) fn capture(value: &ApprovalRequest) -> Self {
        match value {
            ApprovalRequest::Exec(r) => Self::Exec {
                kind: r.kind,
                thread_id: r.thread_id,
                thread_label: r.thread_label.clone(),
                id: r.id.clone(),
                environment_id: r.environment_id.clone(),
                command: r.command.clone(),
                reason: r.reason.clone(),
                available_decisions: r.available_decisions.clone(),
                network_approval_context: r.network_approval_context.clone(),
                additional_permissions: r.additional_permissions.clone(),
            },
            ApprovalRequest::Permissions(r) => Self::Permissions {
                thread_id: r.thread_id,
                thread_label: r.thread_label.clone(),
                call_id: r.call_id.clone(),
                environment_id: r.environment_id.clone(),
                reason: r.reason.clone(),
                permissions: r.permissions.clone(),
            },
            ApprovalRequest::ApplyPatch(r) => {
                let mut changes: Vec<_> = r
                    .changes
                    .iter()
                    .map(|(path, change)| ChangeData {
                        path: path.as_os_str().as_bytes().to_vec(),
                        change: match change {
                            FileChange::Add { content } => FileData::Add {
                                content: content.clone(),
                            },
                            FileChange::Delete { content } => FileData::Delete {
                                content: content.clone(),
                            },
                            FileChange::Update {
                                unified_diff,
                                move_path,
                            } => FileData::Update {
                                unified_diff: unified_diff.clone(),
                                move_path: move_path
                                    .as_ref()
                                    .map(|p| p.as_os_str().as_bytes().to_vec()),
                            },
                        },
                    })
                    .collect();
                changes.sort_by(|a, b| a.path.cmp(&b.path));
                Self::ApplyPatch {
                    thread_id: r.thread_id,
                    thread_label: r.thread_label.clone(),
                    id: r.id.clone(),
                    reason: r.reason.clone(),
                    cwd: r.cwd.as_path().as_os_str().as_bytes().to_vec(),
                    changes,
                }
            }
            ApprovalRequest::McpElicitation(r) => Self::McpElicitation {
                thread_id: r.thread_id,
                thread_label: r.thread_label.clone(),
                server_name: r.server_name.clone(),
                request_id: r.request_id.clone(),
                message: r.message.clone(),
            },
        }
    }
    pub(crate) fn thread_id_data(&self) -> ThreadId {
        match self {
            Self::Exec { thread_id, .. }
            | Self::Permissions { thread_id, .. }
            | Self::ApplyPatch { thread_id, .. }
            | Self::McpElicitation { thread_id, .. } => *thread_id,
        }
    }
    fn without_label(mut self) -> Self {
        match &mut self {
            Self::Exec { thread_label, .. }
            | Self::Permissions { thread_label, .. }
            | Self::ApplyPatch { thread_label, .. }
            | Self::McpElicitation { thread_label, .. } => *thread_label = None,
        }
        self
    }
    pub(crate) fn matches_native(&self, actual: &ApprovalRequest) -> io::Result<bool> {
        // Fresh actual thread labels are presentation metadata; never overwrite
        // a newer source label while restoring the original choice editing.
        Ok(
            serde_json::to_value(self.clone().without_label()).map_err(invalid)?
                == serde_json::to_value(Self::capture(actual).without_label()).map_err(invalid)?,
        )
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum DecisionData {
    Command {
        value: CommandExecutionApprovalDecision,
    },
    FileChange {
        value: FileChangeApprovalDecision,
    },
    Permissions {
        value: PermissionChoice,
    },
    McpElicitation {
        value: McpServerElicitationAction,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PermissionChoice {
    Turn,
    TurnStrict,
    Session,
    Deny,
}
fn decisions(view: &ApprovalOverlay) -> Vec<DecisionData> {
    activations::choices(&view.options)
}
/// Actual UI input/decision generation, never restored from DATA until this
/// same real destination is proved untouched. Overflow is sticky uncertainty.
pub(crate) struct EditingGeneration(Option<u64>);
impl Default for EditingGeneration {
    fn default() -> Self {
        Self(Some(0))
    }
}
impl EditingGeneration {
    pub(crate) fn changed(&mut self) {
        self.0 = self.0.and_then(|v| v.checked_add(1));
    }
    fn original(&self) -> io::Result<u64> {
        self.0
            .ok_or_else(|| invalid("approval editing generation exhausted"))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema: u32,
    request: Option<RequestData>,
    queue: Vec<RequestData>,
    #[serde(deserialize_with = "required_editing")]
    editing: Option<activations::Editing>,
    queued_activations: Vec<activations::Data>,
}
fn required_editing<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<activations::Editing>, D::Error> {
    Deserialize::deserialize(decoder)
}
fn invalid(value: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, value.to_string())
}
impl Snapshot {
    fn decode(raw: &RawValue, max_bytes: usize) -> io::Result<Self> {
        if raw.get().len() > max_bytes {
            return Err(invalid("approval editing exceeds whole UI bound"));
        }
        let value: Self = serde_json::from_str(raw.get()).map_err(invalid)?;
        if value.schema != 2 || value.queue.len() > 256 || value.request.is_none() {
            return Err(invalid("invalid original approval graph"));
        }
        if let Some(editing) = &value.editing {
            editing.validate()?;
        }
        activations::validate_data(&value.queued_activations, &value.queue)?;
        if serde_json::to_value(&value).map_err(invalid)?
            != serde_json::from_str::<serde_json::Value>(raw.get()).map_err(invalid)?
        {
            return Err(invalid(
                "approval graph is not the closed native wire schema",
            ));
        }
        Ok(value)
    }
    fn requests(&self) -> impl Iterator<Item = &RequestData> {
        self.request.iter().chain(self.queue.iter())
    }
}
pub(crate) fn requests_data(raw: &RawValue, max_bytes: usize) -> io::Result<Vec<RequestData>> {
    Ok(Snapshot::decode(raw, max_bytes)?
        .requests()
        .cloned()
        .collect())
}
pub(crate) fn retained_positions(
    raw: &RawValue,
    keep: &[bool],
    max_bytes: usize,
) -> io::Result<Vec<usize>> {
    let data = Snapshot::decode(raw, max_bytes)?;
    if keep.len() != data.requests().count() {
        return Err(invalid(
            "approval callback mask changed original queue count",
        ));
    }
    let current = if keep[0] {
        0
    } else {
        match (1..keep.len()).rev().find(|index| keep[*index]) {
            Some(index) => index,
            None => return Ok(Vec::new()),
        }
    };
    Ok(std::iter::once(current)
        .chain((1..keep.len()).filter(|index| *index != current && keep[*index]))
        .collect())
}
pub(crate) fn editing_retaining(
    raw: &RawValue,
    keep: &[bool],
    max_bytes: usize,
) -> io::Result<Option<Box<RawValue>>> {
    let mut data = Snapshot::decode(raw, max_bytes)?;
    if keep.len() != data.requests().count() {
        return Err(invalid(
            "approval callback mask changed original queue count",
        ));
    }
    let mut original = data
        .request
        .take()
        .into_iter()
        .chain(std::mem::take(&mut data.queue))
        .map(Some)
        .collect::<Vec<_>>();
    let current = if keep[0] {
        0
    } else {
        match (1..keep.len()).rev().find(|index| keep[*index]) {
            Some(index) => index,
            None => return Ok(None),
        }
    };
    let order = std::iter::once(current)
        .chain((1..keep.len()).filter(|index| *index != current && keep[*index]))
        .collect::<Vec<_>>();
    if current != 0 {
        data.editing = data
            .queued_activations
            .iter()
            .find(|value| value.position == current)
            .map(|value| value.editing.clone());
    }
    let mut editing = Vec::new();
    for (position, old) in order.iter().enumerate().skip(1) {
        if let Some(value) = data
            .queued_activations
            .iter()
            .find(|value| value.position == *old)
        {
            let mut value = value.clone();
            value.position = position;
            editing.push(value);
        }
    }
    let mut rows = order
        .into_iter()
        .map(|index| {
            original
                .get_mut(index)
                .and_then(Option::take)
                .ok_or_else(|| invalid("original approval row mask is inconsistent"))
        })
        .collect::<io::Result<Vec<_>>>()?
        .into_iter();
    data.request = rows.next();
    data.queue = rows.collect();
    data.queued_activations = editing;
    Ok(Some(
        serde_json::value::to_raw_value(&data).map_err(invalid)?,
    ))
}
pub(crate) struct PreparedApproval(Snapshot);
impl PreparedApproval {
    pub(crate) fn decode(raw: &RawValue, max_bytes: usize) -> io::Result<Self> {
        Ok(Self(Snapshot::decode(raw, max_bytes)?))
    }
    pub(crate) fn validate_destination(&self, view: &ApprovalOverlay) -> io::Result<()> {
        view.alab_activations.check()?;
        let data = &self.0;
        if data.request.is_some() != view.current_request.is_some()
            || data.queue.len() > view.queue.len()
        {
            return Err(invalid("original approval destination changed"));
        }
        if let (Some(old), Some(actual)) = (&data.request, &view.current_request)
            && !old.matches_native(actual)?
        {
            return Err(invalid("original current approval request changed"));
        }
        for (old, actual) in data.queue.iter().zip(&view.queue) {
            if !old.matches_native(actual)? {
                return Err(invalid("original queued approval changed order or content"));
            }
        }
        if view.alab_editing.original()? == 0
            && let Some(editing) = &data.editing
        {
            editing.validate_destination(view)?;
        }
        Ok(())
    }
    pub(crate) fn install_bound(
        self,
        view: &mut ApprovalOverlay,
        owners: &[ApprovalOwner<'_>],
    ) -> io::Result<()> {
        self.validate_destination(view)?;
        view.alab_inputs.validate_owners(owners)?;
        let data = self.0;
        if owners.len() != data.requests().count()
            || !owners
                .iter()
                .zip(data.requests())
                .all(|(owner, request)| owner.matches(request))
        {
            return Err(invalid(
                "approval editing lacks actual original callback owners",
            ));
        }
        if view.alab_editing.original()? == 0
            && data
                .editing
                .as_ref()
                .is_some_and(activations::Editing::completed)
            && !owners
                .first()
                .is_some_and(ApprovalOwner::permits_completed_view)
        {
            return Err(invalid(
                "completed approval view lacks original publication/closed proof",
            ));
        }
        view.alab_install_queued_activations(&data.queued_activations, owners)?;
        Self(data).install_native_editing(view)
    }
    fn install_native_editing(self, view: &mut ApprovalOverlay) -> io::Result<()> {
        // Actual C2 input is newer than saved C1 editing, including when its
        // cursor returns to the same visible option. It is never overwritten.
        if view.alab_editing.original()? != 0 {
            return Ok(());
        }
        if let Some(editing) = self.0.editing {
            editing.install(view)?;
        }
        Ok(())
    }
}
impl ApprovalOverlay {
    pub(crate) fn alab_capture_approval_editing(
        &self,
        max_bytes: usize,
    ) -> io::Result<Box<RawValue>> {
        let data = Snapshot {
            schema: 2,
            request: self.current_request.as_ref().map(RequestData::capture),
            queue: self.queue.iter().map(RequestData::capture).collect(),
            editing: Some(activations::Editing::capture(self)?),
            queued_activations: self.alab_capture_queued_activations()?,
        };
        let raw = serde_json::value::to_raw_value(&data).map_err(invalid)?;
        let _decoded = Snapshot::decode(&raw, max_bytes)?;
        Ok(raw)
    }
}

#[cfg(test)]
#[path = "retained_approval_overlay_tests.rs"]
mod tests;

impl ApprovalOverlay {
    /// Reorder SAME actual requests/items only. Source replay sorts RPC IDs,
    /// whereas the original delayed queue promotes FIFO into a physical LIFO.
    /// Original set_current rebuilds actions from the actual policy and sender;
    /// no decoded request is used as a native row or callback constructor.
    pub(crate) fn alab_normalize_original_delayed(
        &mut self,
        requests: &[RequestData],
        ids: &[codex_app_server_protocol::RequestId],
        client: &codex_app_server_client::AppServerRequestHandle,
    ) -> io::Result<()> {
        if requests.len() != ids.len() {
            return Err(invalid("delayed approval identity count changed"));
        }
        let bindings = self.alab_inputs.actual_items();
        let actual = self
            .current_request
            .iter()
            .chain(self.queue.iter())
            .collect::<Vec<_>>();
        if actual.len() != bindings.len() {
            return Err(invalid("actual approval queue lost private bindings"));
        }
        let mut selected = Vec::new();
        for (expected, id) in requests.iter().zip(ids) {
            let found = bindings
                .iter()
                .enumerate()
                .filter(|(_, item)| item.as_ref().is_some_and(|item| item.id() == *id))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if found.len() > 1 {
                return Err(invalid("actual approval callback repeated"));
            }
            if let Some(&index) = found.first() {
                let item = bindings[index]
                    .as_ref()
                    .ok_or_else(|| invalid("actual approval item missing"))?;
                let owner = ApprovalOwner::from_actual(item, client)?;
                if !owner.matches(expected) || !expected.matches_native(actual[index])? {
                    return Err(invalid("actual delayed approval content changed"));
                }
                selected.push(index);
            }
        }
        if selected.is_empty() {
            return Ok(());
        }
        let first = selected[0];
        let mut order = vec![first];
        order.extend(selected[1..].iter().rev().copied());
        // Post-cut queue rows retain their original physical order. A newer
        // actual current row becomes the next original LIFO request after A.
        order.extend((1..actual.len()).filter(|index| !selected.contains(index)));
        if !selected.contains(&0) {
            order.push(0);
        }
        if order.iter().copied().eq(0..actual.len()) {
            return Ok(());
        }
        self.alab_activations.check()?;
        let new_item = bindings[first]
            .as_ref()
            .ok_or_else(|| invalid("actual first delayed item missing"))?
            .clone();
        let features = new_item.actual_features()?;
        let previous_item = bindings[0]
            .as_ref()
            .ok_or_else(|| invalid("actual previous delayed item missing"))?
            .clone();
        let previous_request = RequestData::capture(actual[0]);
        self.alab_validate_activation_switch(actual[first], &new_item)?;
        drop(actual);
        // Validate the full permutation before taking any original row.
        if order.len() != bindings.len()
            || order
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != bindings.len()
        {
            return Err(invalid("delayed normalization lost actual rows"));
        }
        self.alab_inputs.reorder_actual(&order)?;
        let mut positions = vec![0; order.len()];
        for (position, original) in order.into_iter().enumerate() {
            positions[original] = position;
        }
        let mut native = self
            .current_request
            .take()
            .into_iter()
            .chain(std::mem::take(&mut self.queue))
            .enumerate()
            .collect::<Vec<_>>();
        native.sort_unstable_by_key(|(index, _)| positions[*index]);
        let mut rows = native.into_iter().map(|(_, row)| row);
        let current = rows
            .next()
            .ok_or_else(|| invalid("original current row missing"))?;
        self.queue = rows.collect();
        if first != 0 {
            self.alab_switch_preserving_activation(
                current,
                new_item,
                previous_item,
                previous_request,
                features,
            )?;
        } else {
            self.current_request = Some(current);
        }
        Ok(())
    }
}

#[path = "retained_approval_activations.rs"]
pub(crate) mod activations;
