// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Historical incremental rendering and its original source boundaries.
use super::*;
use crate::streaming::alab_retained::boundary;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use crate::streaming::code_fence::alab_retained_code_fence as fence;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[cfg(test)]
pub(crate) fn has_open_fence(actual: &StreamingRender) -> bool {
    actual.open_code_fence.is_some()
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    list_spacing: u8,
    lines: Vec<native::HyperlinkData>,
    #[serde(deserialize_with = "required")]
    pending_math_start: Option<usize>,
    #[serde(deserialize_with = "required")]
    mutable_fence_start: Option<usize>,
    completed_source_len: usize,
    stable_source_len: usize,
    stable_rendered_len: usize,
    has_reference_link_definition: bool,
    has_inline_visualization_directive: bool,
    #[serde(deserialize_with = "required")]
    open_code_fence: Option<fence::Data>,
}
impl Data {
    pub(crate) fn capture(
        actual: &StreamingRender,
        source: &str,
        shared: &mut native::Capture,
    ) -> io::Result<Self> {
        let StreamingRender {
            list_spacing,
            lines,
            pending_math_start,
            mutable_fence_start,
            completed_source_len,
            stable_source_len,
            stable_rendered_len,
            has_reference_link_definition,
            has_inline_visualization_directive,
            open_code_fence,
        } = actual;
        Ok(Self {
            list_spacing: match list_spacing {
                ListSpacing::AfterMultiline => 0,
                ListSpacing::Compact => 1,
                ListSpacing::Uniform => 2,
            },
            lines: lines
                .iter()
                .map(|x| shared.line(x))
                .collect::<io::Result<_>>()?,
            pending_math_start: *pending_math_start,
            mutable_fence_start: *mutable_fence_start,
            completed_source_len: *completed_source_len,
            stable_source_len: *stable_source_len,
            stable_rendered_len: *stable_rendered_len,
            has_reference_link_definition: *has_reference_link_definition,
            has_inline_visualization_directive: *has_inline_visualization_directive,
            open_code_fence: open_code_fence
                .as_ref()
                .map(|x| fence::Data::capture(x, source))
                .transpose()?,
        })
    }
    pub(crate) fn fence_revision(&self) -> Option<u64> {
        self.open_code_fence.as_ref().map(fence::Data::revision)
    }
    pub(crate) fn restore(
        self,
        source: &str,
        shared: &mut native::Restored,
        equivalent_revision: Option<u64>,
    ) -> io::Result<StreamingRender> {
        for i in [
            Some(self.completed_source_len),
            Some(self.stable_source_len),
            self.pending_math_start,
            self.mutable_fence_start,
        ]
        .into_iter()
        .flatten()
        {
            boundary(source, i)?;
        }
        if self.stable_rendered_len > self.lines.len() {
            return Err(invalid(
                "streaming stable render boundary outside actual lines",
            ));
        }
        Ok(StreamingRender {
            list_spacing: match self.list_spacing {
                0 => ListSpacing::AfterMultiline,
                1 => ListSpacing::Compact,
                2 => ListSpacing::Uniform,
                _ => return Err(invalid("unknown native streaming list spacing")),
            },
            lines: self
                .lines
                .iter()
                .map(|x| shared.line(x))
                .collect::<io::Result<_>>()?,
            pending_math_start: self.pending_math_start,
            mutable_fence_start: self.mutable_fence_start,
            completed_source_len: self.completed_source_len,
            stable_source_len: self.stable_source_len,
            stable_rendered_len: self.stable_rendered_len,
            has_reference_link_definition: self.has_reference_link_definition,
            has_inline_visualization_directive: self.has_inline_visualization_directive,
            open_code_fence: self
                .open_code_fence
                .map(|x| x.restore(source, equivalent_revision))
                .transpose()?
                .flatten(),
        })
    }
}
