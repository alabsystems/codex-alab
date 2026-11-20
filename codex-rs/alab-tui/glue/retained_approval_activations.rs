// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual per-item native state. Queued DATA borrows exact replay-owned Items;
//! it never constructs a callback, native request or Source/Native permission.
use super::*;
use crate::app::retained_original_input::Item;
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Editing {
    decisions: Vec<DecisionData>,
    list: ListData,
    current_complete: bool,
    done: bool,
    generation: u64,
}
impl Editing {
    pub(super) fn capture(view: &ApprovalOverlay) -> io::Result<Self> {
        Ok(Self {
            decisions: decisions(view),
            list: ListData::capture(&view.list)?,
            current_complete: view.current_complete,
            done: view.done,
            generation: view.alab_editing.original()?,
        })
    }
    pub(super) fn validate(&self) -> io::Result<()> {
        if self.decisions.len() > 256 {
            return Err(invalid("queued approval choices exceed native bound"));
        }
        self.list.validate_data()
    }
    pub(super) fn completed(&self) -> bool {
        self.current_complete || self.done || self.list.completed()
    }
    pub(super) fn validate_destination(&self, view: &ApprovalOverlay) -> io::Result<()> {
        if serde_json::to_value(&self.decisions).map_err(invalid)?
            != serde_json::to_value(decisions(view)).map_err(invalid)?
        {
            return Err(invalid("original approval choice meanings changed"));
        }
        self.list.validate_destination(&view.list)
    }
    pub(super) fn install(self, view: &mut ApprovalOverlay) -> io::Result<()> {
        self.validate_destination(view)?;
        if self.done && !view.queue.is_empty() {
            view.current_complete = true;
            view.done = false;
            view.advance_queue();
        } else {
            self.list.install(&mut view.list)?;
            view.current_complete = self.current_complete;
            view.done = self.done;
            view.alab_editing = EditingGeneration(Some(self.generation));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Data {
    pub(super) position: usize,
    pub(super) request: RequestData,
    pub(super) editing: Editing,
}
pub(super) fn validate_data(values: &[Data], queue: &[RequestData]) -> io::Result<()> {
    let mut seen = std::collections::HashSet::new();
    for value in values {
        if value.position == 0 || value.position > queue.len() || !seen.insert(value.position) {
            return Err(invalid("queued editing position is missing or repeated"));
        }
        if serde_json::to_value(value.request.clone().without_label()).map_err(invalid)?
            != serde_json::to_value(queue[value.position - 1].clone().without_label())
                .map_err(invalid)?
        {
            return Err(invalid("queued editing changed its original whole request"));
        }
        value.editing.validate()?;
    }
    Ok(())
}
struct Actual {
    item: Arc<Item>,
    request: RequestData,
    list: ListSelectionView,
    options: Vec<ApprovalOption>,
    sender: AppEventSender,
    features: Features,
    editing: EditingGeneration,
    complete: bool,
    done: bool,
}
impl Actual {
    fn from_native(item: Arc<Item>, view: ApprovalOverlay) -> io::Result<Self> {
        let request = RequestData::capture(
            view.current_request
                .as_ref()
                .ok_or_else(|| invalid("original constructor lacks actual current row"))?,
        );
        Ok(Self {
            item,
            request,
            list: view.list,
            options: view.options,
            sender: view.app_event_tx,
            features: view.features,
            editing: view.alab_editing,
            complete: view.current_complete,
            done: view.done,
        })
    }
    fn swap_into(&mut self, view: &mut ApprovalOverlay) {
        std::mem::swap(&mut self.list, &mut view.list);
        std::mem::swap(&mut self.options, &mut view.options);
        std::mem::swap(&mut self.sender, &mut view.app_event_tx);
        std::mem::swap(&mut self.features, &mut view.features);
        std::mem::swap(&mut self.editing, &mut view.alab_editing);
        std::mem::swap(&mut self.complete, &mut view.current_complete);
        std::mem::swap(&mut self.done, &mut view.done);
    }
    fn data(&self, position: usize) -> io::Result<Data> {
        Ok(Data {
            position,
            request: self.request.clone(),
            editing: Editing {
                decisions: choices(&self.options),
                list: ListData::capture(&self.list)?,
                current_complete: self.complete,
                done: self.done,
                generation: self.editing.original()?,
            },
        })
    }
}
pub(super) fn choices(options: &[ApprovalOption]) -> Vec<DecisionData> {
    options
        .iter()
        .map(|option| decision_data(&option.decision))
        .collect()
}
fn decision_data(decision: &ApprovalDecision) -> DecisionData {
    match decision {
        ApprovalDecision::Command(v) => DecisionData::Command { value: v.clone() },
        ApprovalDecision::FileChange(v) => DecisionData::FileChange { value: v.clone() },
        ApprovalDecision::McpElicitation(v) => DecisionData::McpElicitation { value: *v },
        ApprovalDecision::Permissions(v) => DecisionData::Permissions {
            value: match v {
                PermissionsDecision::GrantForTurn => PermissionChoice::Turn,
                PermissionsDecision::GrantForTurnWithStrictAutoReview => {
                    PermissionChoice::TurnStrict
                }
                PermissionsDecision::GrantForSession => PermissionChoice::Session,
                PermissionsDecision::Deny => PermissionChoice::Deny,
            },
        },
    }
}
struct Pending {
    item: Arc<Item>,
    data: Data,
}
#[derive(Default)]
pub(crate) struct Book {
    actual: Vec<Actual>,
    pending: Vec<Pending>,
    fault: Option<String>,
}
impl Book {
    pub(super) fn check(&self) -> io::Result<()> {
        match &self.fault {
            Some(value) => Err(invalid(value)),
            None => Ok(()),
        }
    }
    fn prune(&mut self, items: &[Option<Arc<Item>>]) {
        self.actual.retain(|entry| {
            items
                .iter()
                .flatten()
                .any(|item| Arc::ptr_eq(item, &entry.item))
        });
        self.pending.retain(|entry| {
            items
                .iter()
                .flatten()
                .any(|item| Arc::ptr_eq(item, &entry.item))
        });
    }
}
impl ApprovalOverlay {
    pub(crate) fn alab_prune_activations(&mut self) {
        self.alab_activations
            .prune(&self.alab_inputs.actual_items());
    }
    pub(super) fn alab_capture_queued_activations(&self) -> io::Result<Vec<Data>> {
        self.alab_activations.check()?;
        let items = self.alab_inputs.actual_items();
        let mut result = Vec::new();
        for (position, item) in items.iter().enumerate().skip(1) {
            let Some(item) = item else {
                continue;
            };
            if let Some(entry) = self
                .alab_activations
                .actual
                .iter()
                .find(|entry| Arc::ptr_eq(&entry.item, item))
            {
                result.push(entry.data(position)?);
            } else if let Some(entry) = self
                .alab_activations
                .pending
                .iter()
                .find(|entry| Arc::ptr_eq(&entry.item, item))
            {
                let mut data = entry.data.clone();
                data.position = position;
                result.push(data);
            }
        }
        Ok(result)
    }
    pub(super) fn alab_install_queued_activations(
        &mut self,
        data: &[Data],
        owners: &[ApprovalOwner<'_>],
    ) -> io::Result<()> {
        self.alab_activations.check()?;
        let items = self.alab_inputs.actual_items();
        let mut additions = Vec::new();
        // Validate every resource before any native state changes.
        for value in data {
            let item = items
                .get(value.position)
                .and_then(Option::as_ref)
                .ok_or_else(|| invalid("queued activation has no actual replayed item"))?;
            let owner = owners
                .get(value.position)
                .ok_or_else(|| invalid("queued activation lost its actual Source owner"))?;
            if !owner.same_actual_item(item)
                || !owner.matches(&value.request)
                || value.editing.completed() && !owner.permits_completed_view()
            {
                return Err(invalid(
                    "queued editing lacks exact actual owner/completed input",
                ));
            }
            value.editing.validate()?;
            if !self
                .alab_activations
                .actual
                .iter()
                .any(|old| Arc::ptr_eq(&old.item, item))
                && !self
                    .alab_activations
                    .pending
                    .iter()
                    .any(|old| Arc::ptr_eq(&old.item, item))
            {
                additions.push(Pending {
                    item: item.clone(),
                    data: value.clone(),
                });
            }
        }
        self.alab_activations.pending.extend(additions);
        Ok(())
    }
    pub(crate) fn alab_restore_actual_activation(
        &mut self,
        request: ApprovalRequest,
    ) -> Option<ApprovalRequest> {
        self.alab_prune_activations();
        let Some(item) = self.alab_inputs.actual_items().first().cloned().flatten() else {
            return Some(request);
        };
        let Some(index) = self
            .alab_activations
            .actual
            .iter()
            .position(|entry| Arc::ptr_eq(&entry.item, &item))
        else {
            return Some(request);
        };
        match self.alab_activations.actual[index]
            .request
            .matches_native(&request)
        {
            Ok(true) => {}
            _ => {
                self.alab_activations.fault =
                    Some("actual cached activation changed original native request".into());
                return Some(request);
            }
        }
        let mut entry = self.alab_activations.actual.remove(index);
        entry.swap_into(self);
        self.current_request = Some(request);
        if self.done && !self.queue.is_empty() {
            self.done = false;
            self.current_complete = true;
            self.advance_queue();
        }
        None
    }
    pub(crate) fn alab_apply_queued_activation(&mut self) {
        self.alab_prune_activations();
        let Some(item) = self.alab_inputs.actual_items().first().cloned().flatten() else {
            return;
        };
        let Some(index) = self
            .alab_activations
            .pending
            .iter()
            .position(|entry| Arc::ptr_eq(&entry.item, &item))
        else {
            return;
        };
        let data = self.alab_activations.pending[index].data.clone();
        let valid = self
            .current_request
            .as_ref()
            .is_some_and(|actual| data.request.matches_native(actual).unwrap_or(false));
        if !valid {
            self.alab_activations.fault =
                Some("bound queued activation changed original request".into());
            return;
        }
        let pending = self.alab_activations.pending.remove(index);
        if let Err(error) = data.editing.install(self) {
            self.alab_activations.pending.push(pending);
            self.alab_activations.fault = Some(error.to_string());
        }
    }
    pub(crate) fn alab_validate_activation_switch(
        &self,
        request: &ApprovalRequest,
        new_item: &Arc<Item>,
    ) -> io::Result<()> {
        self.alab_activations.check()?;
        if let Some(entry) = self
            .alab_activations
            .actual
            .iter()
            .find(|entry| Arc::ptr_eq(&entry.item, new_item))
            && !entry.request.matches_native(request)?
        {
            return Err(invalid(
                "actual queued activation changed request before switch",
            ));
        }
        Ok(())
    }
    pub(crate) fn alab_switch_preserving_activation(
        &mut self,
        request: ApprovalRequest,
        new_item: Arc<Item>,
        old_item: Arc<Item>,
        old_request: RequestData,
        features: Features,
    ) -> io::Result<()> {
        self.alab_activations.check()?;
        let existing = self
            .alab_activations
            .actual
            .iter()
            .position(|entry| Arc::ptr_eq(&entry.item, &new_item));
        if let Some(index) = existing
            && !self.alab_activations.actual[index]
                .request
                .matches_native(&request)?
        {
            return Err(invalid(
                "actual queued activation changed request before switch",
            ));
        }
        let mut next = match existing {
            Some(index) => self.alab_activations.actual.remove(index),
            None => {
                // Original activation consumes the SAME native request and
                // actual feature policy; this temporary view is never pushed.
                let native = new_item.with_scope(|| {
                    ApprovalOverlay::new(
                        request.clone(),
                        self.app_event_tx.clone(),
                        features,
                        self.approval_keymap.clone(),
                        self.list_keymap.clone(),
                    )
                });
                Actual::from_native(new_item, native)?
            }
        };
        next.swap_into(self);
        next.item = old_item;
        next.request = old_request;
        self.alab_activations.actual.push(next);
        self.current_request = Some(request);
        if self.done && !self.queue.is_empty() {
            self.done = false;
            self.current_complete = true;
            self.advance_queue();
        } else {
            self.alab_apply_queued_activation();
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "retained_approval_activations_tests.rs"]
mod tests;
