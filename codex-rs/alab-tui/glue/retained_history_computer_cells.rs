// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve grouped native computer calls and shared chronological detail cells.
use super::*;
use crate::history_cell::activity_details::alab_retained_details;
use crate::history_cell::alab_retained_cells::CaptureContext;
use crate::history_cell::alab_retained_cells::RestoreContext;
use crate::history_cell::mcp::alab_retained_mcp::Call;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    calls: Vec<Call>,
    details: alab_retained_details::Data,
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        context: &mut CaptureContext<'_>,
    ) -> io::Result<Option<Self>> {
        let Some(actual) = actual.as_any().downcast_ref::<ComputerActivityCell>() else {
            return Ok(None);
        };
        if actual.group.calls.len() > native::MAX_NODES {
            return Err(native::invalid("computer call capacity"));
        }
        Ok(Some(Self {
            calls: actual
                .group
                .calls
                .iter()
                .map(Call::capture)
                .collect::<io::Result<_>>()?,
            details: alab_retained_details::Data::capture(
                &actual.group.details,
                context,
                actual.group.calls.len(),
            )?,
        }))
    }
    pub(in crate::history_cell) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
        nesting: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        if self.calls.len() > native::MAX_NODES {
            return Err(native::invalid("computer call capacity"));
        }
        let calls = self
            .calls
            .into_iter()
            .map(Call::restore)
            .collect::<io::Result<Vec<_>>>()?;
        let details = self
            .details
            .restore(shared, resources, calls.len(), nesting)?;
        Ok(Box::new(ComputerActivityCell {
            group: ActivityGroup { calls, details },
        }))
    }
}
