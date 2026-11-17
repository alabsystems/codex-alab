// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Warning source identity is independent of transcript visibility/wrapping.
use super::*;
use crate::history_cell::base::alab_retained::PrefixedData;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    schema: u8,
    server_version_notice: bool,
    visible_in_transcript: bool,
    key: String,
    diagnostic: String,
    details: PrefixedData,
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        _graph: &mut native::Capture,
        depth: usize,
    ) -> io::Result<Option<Self>> {
        native::depth(depth)?;
        let Some(actual) = actual.as_any().downcast_ref::<WarningHistoryCell>() else {
            return Ok(None);
        };
        Ok(Some(Self {
            schema: 1,
            server_version_notice: actual.server_version_notice,
            visible_in_transcript: actual.visible_in_transcript,
            key: actual.key.clone(),
            diagnostic: actual.diagnostic.clone(),
            details: PrefixedData::capture(&actual.details),
        }))
    }
    pub(in crate::history_cell) fn restore(
        self,
        _graph: &mut native::Restored,
        depth: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        native::depth(depth)?;
        if self.schema != 1 {
            return Err(native::invalid("native warning version"));
        }
        Ok(Box::new(WarningHistoryCell {
            server_version_notice: self.server_version_notice,
            visible_in_transcript: self.visible_in_transcript,
            key: self.key,
            diagnostic: self.diagnostic,
            details: self.details.restore()?,
        }))
    }
}
