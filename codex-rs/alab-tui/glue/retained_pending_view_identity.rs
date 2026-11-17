// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Stable identity DATA for the exact original callback(s) behind one view.
//! Decoding does not create a reply or prove a Source endpoint is still open.
use crate::bottom_pane::alab_retained_views::ViewData;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ToolRequestUserInputParams;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::io;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct CapturedView {
    schema: u32,
    request_ids: Vec<RequestId>,
    view: ViewData,
}
impl CapturedView {
    pub(super) fn capture(
        view: ViewData,
        request_ids: Vec<RequestId>,
        max_bytes: usize,
    ) -> io::Result<Self> {
        let value = Self {
            schema: 1,
            request_ids,
            view,
        };
        value.validate_data(max_bytes)?;
        Ok(value)
    }
    /// Feed these original IDs into the actual protected Source Capture before
    /// closing C1. They cannot select a callback without its authenticated owner.
    pub(crate) fn request_ids_data(&self) -> &[RequestId] {
        &self.request_ids
    }
    pub(in crate::app) fn original_thread_ids_data(
        &self,
        max_bytes: usize,
    ) -> io::Result<Vec<String>> {
        self.validate_data(max_bytes)?;
        Ok(self.view.requests(max_bytes)?.thread_ids())
    }
    pub(in crate::app) fn original_requests_data(
        &self,
        max_bytes: usize,
    ) -> io::Result<Vec<ToolRequestUserInputParams>> {
        self.validate_data(max_bytes)?;
        requests(&self.view, max_bytes)
    }
    /// Pure editing DATA pruning. The caller must obtain CLOSED identities
    /// from an actual Source observation; this helper supplies no such proof.
    /// If the current request closed, its editor must never move into a later
    /// request. Original replay already owns that later request's fresh editor.
    pub(in crate::app) fn editing_after_closed(
        &self,
        closed: &HashSet<RequestId>,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<Option<(ViewData, Vec<RequestId>)>> {
        self.validate_data(max_bytes)?;
        self.view.validate_native_data(max_bytes, max_generations)?;
        if matches!(&self.view, ViewData::UserInput { .. }) && closed.contains(&self.request_ids[0])
        {
            return Ok(None);
        }
        let keep: Vec<_> = self
            .request_ids
            .iter()
            .map(|id| !closed.contains(id))
            .collect();
        let Some(data) = self
            .view
            .editing_retaining(&keep, max_bytes, max_generations)?
        else {
            return Ok(None);
        };
        let ids = if let ViewData::Approval { original } = &self.view {
            crate::bottom_pane::alab_retained_approval::retained_positions(
                original, &keep, max_bytes,
            )?
            .into_iter()
            .map(|index| self.request_ids[index].clone())
            .collect()
        } else {
            self.request_ids
                .iter()
                .filter(|id| !closed.contains(*id))
                .cloned()
                .collect()
        };
        Ok(Some((data, ids)))
    }
    pub(crate) fn validate_data(&self, max_bytes: usize) -> io::Result<()> {
        let actual_requests = self.view.requests(max_bytes)?;
        if self.schema != 1
            || self.request_ids.len() != actual_requests.len()
            || self.request_ids.is_empty()
        {
            return Err(invalid(
                "retained view callback identity count differs from its request queue",
            ));
        }
        let mut unique = HashSet::new();
        if self.request_ids.iter().any(|id| !unique.insert(id)) {
            return Err(invalid("retained view repeats a callback identity"));
        }
        Ok(())
    }
    pub(super) fn into_parts(self, max_bytes: usize) -> io::Result<(ViewData, Vec<RequestId>)> {
        self.validate_data(max_bytes)?;
        Ok((self.view, self.request_ids))
    }
}

pub(in crate::app) fn requests(
    data: &ViewData,
    max_bytes: usize,
) -> io::Result<Vec<ToolRequestUserInputParams>> {
    match data {
        ViewData::Approval { .. } | ViewData::DelayedApproval { .. } => Err(invalid(
            "question-only parser cannot consume approval graph",
        )),
        ViewData::UserInput { original } => {
            if original.get().len() > max_bytes {
                return Err(invalid("retained pending view exceeds E1 budget"));
            }
            // Only extracts original identity DATA. The native variant decoder
            // still validates the complete closed editing graph before install.
            #[derive(Deserialize)]
            struct Requests {
                request: ToolRequestUserInputParams,
                queue: VecDeque<ToolRequestUserInputParams>,
            }
            let value: Requests = serde_json::from_str(original.get()).map_err(invalid)?;
            Ok(std::iter::once(value.request).chain(value.queue).collect())
        }
    }
}

pub(super) fn validate_bound_ids<'a>(
    captured: &[RequestId],
    actual: impl Iterator<Item = &'a RequestId>,
) -> io::Result<()> {
    let mut actual = actual;
    for captured in captured {
        if actual.next() != Some(captured) {
            return Err(invalid(
                "retained view no longer has the exact original callback identity",
            ));
        }
    }
    if actual.next().is_some() {
        return Err(invalid("replayed callback queue contains an extra owner"));
    }
    Ok(())
}
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
#[path = "retained_pending_view_identity_tests.rs"]
mod tests;
