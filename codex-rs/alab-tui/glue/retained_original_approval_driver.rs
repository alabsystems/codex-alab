// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native approval queue bindings. The existing handlers still create
//! their history, choices and commands. Their sender replaces only the exact
//! response with the same immutable Source publication owner before enqueue.
use super::*;
use crate::bottom_pane::ApprovalRequest;
use crate::bottom_pane::alab_retained_approval::RequestData;

impl Item {
    pub(super) fn bind_native(&self, native: &ApprovalRequest) -> io::Result<()> {
        let data = RequestData::capture(native);
        if self.native_faulted.load(Ordering::Acquire) {
            return Err(invalid(
                "original native approval conversion remains faulted",
            ));
        }
        if !self.request.allows_native(&data) {
            self.native_faulted.store(true, Ordering::Release);
            return Err(invalid(
                "approval conversion changed actual callback kind/thread",
            ));
        }
        let mut old = self
            .native_approval
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(old) = old.as_ref() {
            if !old.matches_native(native)? {
                self.native_faulted.store(true, Ordering::Release);
                return Err(invalid(
                    "approval conversion changed original native request",
                ));
            }
        } else {
            *old = Some(data);
        }
        Ok(())
    }
    pub(in crate::app) fn matches_approval(&self, data: &RequestData) -> bool {
        if self.native_faulted.load(Ordering::Acquire) {
            return false;
        }
        let old = self
            .native_approval
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match (
            old.as_ref()
                .and_then(|value| serde_json::to_value(value).ok()),
            serde_json::to_value(data),
        ) {
            (Some(left), Ok(right)) => without_label(left) == without_label(right),
            _ => false,
        }
    }
    pub(in crate::app) fn has_native_approval(&self) -> bool {
        !self.native_faulted.load(Ordering::Acquire)
            && self
                .native_approval
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
    }
    pub(in crate::app) fn source_binding_observed(&self) -> bool {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .bound
    }
    pub(in crate::app) fn has_original_value(&self) -> bool {
        self.reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending_publication()
            .is_some()
    }
}
fn without_label(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(object) = value.as_object_mut() {
        for key in ["thread_label", "threadLabel"] {
            if object.contains_key(key) {
                object.insert(key.into(), serde_json::Value::Null);
            }
        }
    }
    value
}

pub(crate) fn bind_current(native: &ApprovalRequest) -> Option<Arc<Item>> {
    let item = current()?;
    if let Err(error) = item.bind_native(native) {
        tracing::warn!(%error, "actual delayed approval owner remains held");
    }
    Some(item)
}

#[derive(Default)]
pub(crate) struct Bindings {
    current: Option<Arc<Item>>,
    queued: Vec<Option<Arc<Item>>>,
}
impl Bindings {
    pub(crate) fn capture() -> Self {
        Self {
            current: current(),
            queued: Vec::new(),
        }
    }
    pub(crate) fn push(&mut self, native: &ApprovalRequest) {
        self.queued.push(bind_current(native));
    }
    pub(crate) fn advance(&mut self) {
        if let Some(next) = self.queued.pop() {
            self.current = next;
        }
    }
    pub(crate) fn clear_queue(&mut self) {
        self.queued.clear();
    }
    pub(crate) fn sender(
        &self,
        original: &crate::app_event_sender::AppEventSender,
        native: &ApprovalRequest,
    ) -> crate::app_event_sender::AppEventSender {
        let mut sender = original.clone();
        sender.alab_original_input = self.current.clone();
        if let Some(item) = &self.current
            && let Err(error) = item.bind_native(native)
        {
            tracing::warn!(%error, "actual approval remains held on its original callback");
        }
        sender
    }
    pub(crate) fn retain(
        &mut self,
        requests: &mut Vec<ApprovalRequest>,
        mut matches: impl FnMut(&ApprovalRequest) -> bool,
    ) {
        if self.queued.len() != requests.len() {
            tracing::warn!("approval queue/resource count remains held");
            return;
        }
        let old = std::mem::take(requests);
        let bindings = std::mem::take(&mut self.queued);
        for (request, binding) in old.into_iter().zip(bindings) {
            let remove = matches(&request) && binding.as_ref().is_none_or(matches_resolved);
            if !remove {
                requests.push(request);
                self.queued.push(binding);
            }
        }
    }
    pub(crate) fn current_matches_resolved(&self) -> bool {
        self.current.as_ref().is_none_or(matches_resolved)
    }
    pub(crate) fn validate_owners(
        &self,
        owners: &[crate::app::app_server_requests::alab_retained_owners::ApprovalOwner<'_>],
    ) -> io::Result<()> {
        let items = self.actual_items();
        if owners.is_empty() || owners.len() > items.len() {
            return Err(invalid("approval owner count changed"));
        }
        for (item, owner) in items.iter().zip(owners) {
            if !item
                .as_ref()
                .is_some_and(|item| owner.same_actual_item(item))
            {
                return Err(invalid("approval editing changed actual callback resource"));
            }
        }
        Ok(())
    }
    pub(crate) fn actual_items(&self) -> Vec<Option<Arc<Item>>> {
        std::iter::once(self.current.clone())
            .chain(self.queued.iter().cloned())
            .collect()
    }
}

/// Called by the actual common sender before its original channel/logging
/// path. Unsupported/auth commands keep the original route. A claimed actual
/// response never falls back to raw resolve or picks a later logical key.
pub(crate) fn rewrite(
    sender: &crate::app_event_sender::AppEventSender,
    event: crate::app_event::AppEvent,
) -> Option<crate::app_event::AppEvent> {
    let Some(item) = &sender.alab_original_input else {
        return Some(event);
    };
    if sender.voice_only.load(Ordering::Relaxed) {
        return Some(event);
    }
    let crate::app_event::AppEvent::SubmitThreadOp { thread_id, op } = &event else {
        return Some(event);
    };
    match response(item, *thread_id, op) {
        Ok(Some(value)) => match item.freeze_value(value) {
            Ok(publication) => Some(crate::app_event::AppEvent::AlabOriginalInput(publication)),
            Err(error) => {
                tracing::warn!(%error, "immutable original approval remains owned");
                None
            }
        },
        Ok(None) => Some(event),
        Err(error) => {
            tracing::warn!(%error, "approval response does not match its actual callback");
            None
        }
    }
}
fn response(
    item: &Item,
    thread: codex_protocol::ThreadId,
    op: &AppCommand,
) -> io::Result<Option<serde_json::Value>> {
    use codex_app_server_protocol::CommandExecutionRequestApprovalResponse;
    use codex_app_server_protocol::FileChangeRequestApprovalResponse;
    use codex_app_server_protocol::PermissionsRequestApprovalResponse;
    if item.native_faulted.load(Ordering::Acquire) {
        return Err(invalid("original approval conversion remains faulted"));
    }
    let _features = item.actual_features()?;
    let native = item
        .native_approval
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let Some(native) = native.as_ref() else {
        return Err(invalid("original approval native owner missing"));
    };
    if thread.to_string() != item.thread_id_data() {
        return Err(invalid("approval response changed actual thread"));
    }
    let value = match (native, op) {
        (RequestData::Exec { id: actual, .. }, AppCommand::ExecApproval { id, decision, .. })
            if actual == id =>
        {
            serde_json::to_value(CommandExecutionRequestApprovalResponse {
                decision: decision.clone(),
            })
        }
        (
            RequestData::ApplyPatch { id: actual, .. },
            AppCommand::PatchApproval { id, decision },
        ) if actual == id => serde_json::to_value(FileChangeRequestApprovalResponse {
            decision: decision.clone(),
        }),
        (
            RequestData::Permissions {
                call_id: actual, ..
            },
            AppCommand::RequestPermissionsResponse { id, response },
        ) if actual == id => serde_json::to_value(PermissionsRequestApprovalResponse {
            permissions:
                crate::app_server_approval_conversions::granted_permission_profile_from_request(
                    response.permissions.clone(),
                ),
            scope: response.scope.into(),
            strict_auto_review: response.strict_auto_review.then_some(true),
        }),
        (
            _,
            AppCommand::ExecApproval { .. }
            | AppCommand::PatchApproval { .. }
            | AppCommand::RequestPermissionsResponse { .. },
        ) => return Err(invalid("approval decision changed original native request")),
        _ => return Ok(None),
    };
    value.map(Some).map_err(invalid)
}

/// Runs only inside the actual incoming item scope at the original native
/// push, before either delayed or promoted queue. No DATA decoder calls it.
pub(crate) fn bind_features(features: &codex_features::Features) {
    let Some(item) = current() else {
        return;
    };
    let mut actual = item
        .native_features
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    match &*actual {
        Ok(Some(old)) if old != features => {
            *actual = Err(());
            tracing::warn!("same actual approval replay changed native feature policy");
        }
        Ok(None) => *actual = Ok(Some(features.clone())),
        _ => {}
    }
}
impl Item {
    pub(crate) fn actual_features(&self) -> io::Result<codex_features::Features> {
        match &*self
            .native_features
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            Ok(Some(actual)) => Ok(actual.clone()),
            _ => Err(invalid(
                "actual original native feature policy missing or changed",
            )),
        }
    }
}
impl Bindings {
    pub(crate) fn reorder_actual(&mut self, order: &[usize]) -> io::Result<()> {
        let mut all = self.actual_items();
        validate_permutation(order, all.len())?;
        let mut reordered = order.iter().map(|index| all[*index].take());
        self.current = reordered.next().flatten();
        self.queued = reordered.collect();
        Ok(())
    }
}
fn validate_permutation(order: &[usize], len: usize) -> io::Result<()> {
    if order.len() != len {
        return Err(invalid("actual approval permutation count changed"));
    }
    let mut seen = std::collections::HashSet::new();
    if order
        .iter()
        .any(|index| *index >= len || !seen.insert(*index))
    {
        return Err(invalid(
            "actual approval permutation repeats or loses a row",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "retained_original_approval_driver_tests.rs"]
pub(crate) mod tests;
