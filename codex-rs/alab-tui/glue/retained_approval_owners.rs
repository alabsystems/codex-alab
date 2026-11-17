// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! A native view borrows the actual reserved queue item, never a lookup chosen
//! from equal decoded request params. The same real reply/Source operation
//! remains owned through synchronous editing installation.
use super::*;
use crate::app::retained_original_input::Item;
use crate::bottom_pane::alab_retained_approval::RequestData;
use std::sync::Arc;

pub(crate) struct ApprovalOwner<'a> {
    item: &'a Arc<Item>,
    id: AppServerRequestId,
}
impl<'a> ApprovalOwner<'a> {
    pub(crate) fn from_actual(
        item: &'a Arc<Item>,
        actual: &AppServerRequestHandle,
    ) -> io::Result<Self> {
        if !item.is_for_client(actual) || !item.has_native_approval() {
            return Err(unavailable());
        }
        Ok(Self {
            item,
            id: item.id(),
        })
    }
    pub(crate) fn request_id_data(&self) -> &AppServerRequestId {
        &self.id
    }
    pub(crate) fn matches(&self, native: &RequestData) -> bool {
        self.item.matches_approval(native)
    }
    pub(crate) fn permits_completed_view(&self) -> bool {
        self.item.has_original_value() && self.item.source_binding_observed()
    }
    pub(crate) fn same_actual_item(&self, item: &Arc<Item>) -> bool {
        Arc::ptr_eq(self.item, item)
    }
}
