// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native copy targets share text and guard identities with the whole App.
//! Local guards remain dormant here; only the actual installer may arm them.
use super::*;
use crate::copy_input_guard::alab_retained_graph as guards;
use crate::transcript_view::alab_retained_view::identity::KeyData;
use crate::transcript_view::alab_retained_view::tables::Capture;
use crate::transcript_view::alab_retained_view::tables::Restore;
use crate::transcript_view::alab_retained_view::tables::TextRef;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct Data {
    schema: u8,
    #[serde(deserialize_with = "required")]
    guard: Option<guards::GuardRef>,
    #[serde(deserialize_with = "required")]
    response: Option<(KeyData, TextRef)>,
    whole: bool,
    #[serde(deserialize_with = "required")]
    source: Option<String>,
    label: String,
}
impl Data {
    pub(in crate::transcript_view) fn capture(
        actual: &CopyMode,
        graph: &mut Capture<'_>,
        guards: &mut guards::Capture,
    ) -> io::Result<Self> {
        Ok(Self {
            schema: 1,
            guard: actual
                ._guard
                .as_ref()
                .map(|value| guards.strong(value))
                .transpose()?,
            response: actual
                .response
                .as_ref()
                .map(|(key, text)| -> io::Result<_> { Ok((graph.key(*key)?, graph.text(text)?)) })
                .transpose()?,
            whole: actual.whole,
            source: actual.source.clone(),
            label: actual.label.to_string(),
        })
    }
    pub(in crate::transcript_view) fn restore(
        self,
        graph: &Restore<'_>,
        guards: &guards::Restored,
    ) -> io::Result<CopyMode> {
        if self.schema != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native copy-mode schema",
            ));
        }
        Ok(CopyMode {
            _guard: self
                .guard
                .map(|reference| guards.strong(reference))
                .transpose()?,
            response: self
                .response
                .map(|(key, text)| -> io::Result<_> {
                    Ok((key.restore(graph.cells)?, graph.text(text)?))
                })
                .transpose()?,
            whole: self.whole,
            source: self.source,
            label: self.label.into(),
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(decoder)
}

#[cfg(test)]
#[path = "retained_transcript_copy_mode_tests.rs"]
mod tests;
