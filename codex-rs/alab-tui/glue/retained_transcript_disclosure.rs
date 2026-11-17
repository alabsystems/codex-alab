// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native disclosure state over the same retained cell graph. Activity IDs and
//! approval correlation remain presentation data, never callback ownership.
use super::identity::ActivityIdData;
use super::identity::CaptureRefs;
use super::identity::KeyData;
use super::*;
use crate::history_cell::alab_retained_cells::Restored;
use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;
use serde_json::value::RawValue;
use std::io;

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct Data {
    schema: u32,
    expanded: Vec<ActivityIdData>,
    focused: Option<(KeyData, Vec<ActivityIdData>)>,
    resume_after_approval: Option<(ThreadId, String)>,
    anchor_control: Option<KeyData>,
    live_ids: Vec<ActivityIdData>,
}
impl Data {
    pub(in crate::transcript_view) fn capture(
        view: &TranscriptView,
        refs: &CaptureRefs<'_>,
    ) -> io::Result<Self> {
        let actual = &view.disclosure;
        let mut expanded = actual
            .expanded
            .iter()
            .map(|value| {
                let value = refs.activity(value);
                serde_json::to_string(&value)
                    .map(|key| (key, value))
                    .map_err(invalid)
            })
            .collect::<io::Result<Vec<_>>>()?;
        expanded.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Self {
            schema: 2,
            expanded: expanded.into_iter().map(|(_, value)| value).collect(),
            focused: actual
                .focused
                .as_ref()
                .map(|(key, ids)| {
                    refs.key(*key)
                        .map(|key| (key, ids.iter().map(|value| refs.activity(value)).collect()))
                })
                .transpose()?,
            resume_after_approval: actual.resume_after_approval.clone(),
            anchor_control: actual.anchor_control.map(|key| refs.key(key)).transpose()?,
            live_ids: actual
                .live_ids
                .iter()
                .map(|value| refs.activity(value))
                .collect(),
        })
    }
    pub(in crate::transcript_view) fn decode(raw: &RawValue, max: usize) -> io::Result<Self> {
        if raw.get().len() > max {
            return Err(invalid("native disclosure exceeds whole-view bound"));
        }
        let data: Self = serde_json::from_str(raw.get()).map_err(invalid)?;
        let expanded = data
            .expanded
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid)?;
        if data.schema != 2
            || expanded.windows(2).any(|pair| pair[0] >= pair[1])
            || serde_json::to_value(&data).map_err(invalid)?
                != serde_json::from_str::<serde_json::Value>(raw.get()).map_err(invalid)?
        {
            return Err(invalid(
                "native disclosure has omitted, foreign or noncanonical fields",
            ));
        }
        Ok(data)
    }
    pub(in crate::transcript_view) fn restore(
        self,
        view: &mut TranscriptView,
        graph: &Restored,
    ) -> io::Result<()> {
        // Resolve every process-local key before changing any native state.
        let focused = self
            .focused
            .map(|(key, ids)| -> io::Result<_> {
                Ok((
                    key.restore(graph)?,
                    ids.into_iter()
                        .map(|id| id.restore(graph))
                        .collect::<io::Result<Vec<_>>>()?,
                ))
            })
            .transpose()?;
        let anchor_control = self
            .anchor_control
            .map(|key| key.restore(graph))
            .transpose()?;
        let expanded = self
            .expanded
            .into_iter()
            .map(|id| id.restore(graph))
            .collect::<io::Result<Vec<_>>>()?;
        let count = expanded.len();
        let expanded: std::collections::HashSet<_> = expanded.into_iter().collect();
        if expanded.len() != count {
            return Err(invalid("duplicate rebound native activity ID"));
        }
        let live_ids = self
            .live_ids
            .into_iter()
            .map(|id| id.restore(graph))
            .collect::<io::Result<Vec<_>>>()?;
        let actual = &mut view.disclosure;
        actual.expanded = expanded;
        actual.focused = focused;
        actual.resume_after_approval = self.resume_after_approval;
        actual.anchor_control = anchor_control;
        actual.live_ids = live_ids;
        // Actual constructor keymap remains owned by the destination config.
        Ok(())
    }
}

#[cfg(test)]
#[path = "retained_transcript_disclosure_tests.rs"]
mod tests;

impl Data {
    pub(in crate::transcript_view) fn prepare_native(
        self,
        graph: &Restored,
        keymap: &crate::keymap::RuntimeKeymap,
    ) -> io::Result<crate::transcript_view::disclosure::Disclosure> {
        // Resolve every process-local key before changing any native state.
        let focused = self
            .focused
            .map(|(key, ids)| -> io::Result<_> {
                Ok((
                    key.restore(graph)?,
                    ids.into_iter()
                        .map(|id| id.restore(graph))
                        .collect::<io::Result<Vec<_>>>()?,
                ))
            })
            .transpose()?;
        let anchor_control = self
            .anchor_control
            .map(|key| key.restore(graph))
            .transpose()?;
        let expanded = self
            .expanded
            .into_iter()
            .map(|id| id.restore(graph))
            .collect::<io::Result<Vec<_>>>()?;
        let count = expanded.len();
        let expanded: std::collections::HashSet<_> = expanded.into_iter().collect();
        if expanded.len() != count {
            return Err(invalid("duplicate rebound native activity ID"));
        }
        let live_ids = self
            .live_ids
            .into_iter()
            .map(|id| id.restore(graph))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(crate::transcript_view::disclosure::Disclosure {
            expanded,
            focused,
            resume_after_approval: self.resume_after_approval,
            anchor_control,
            live_ids,
            keymap: keymap.clone(),
        })
    }
}
