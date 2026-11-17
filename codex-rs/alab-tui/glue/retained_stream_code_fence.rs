// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Keep the exact fast-path cursor and distinguish uninitialized/plain/highlighted caches.
use super::*;
use crate::render::highlight::alab_retained_highlighter as highlighter;
use crate::streaming::alab_retained::boundary;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    marker: u8,
    marker_len: usize,
    language: String,
    content_start: usize,
    source_len: usize,
    theme_revision: u64,
    syntax_identity: [u8; 32],
    #[serde(deserialize_with = "required")]
    highlighter: Option<highlighter::Data>,
}
impl Data {
    pub(crate) fn capture(actual: &OpenCodeFence, source: &str) -> io::Result<Self> {
        let OpenCodeFence {
            marker,
            marker_len,
            language,
            content_start,
            source_len,
            theme_revision,
            highlighter,
        } = actual;
        let code = source
            .get(*content_start..*source_len)
            .ok_or_else(|| invalid("original fence code outside stream source"))?;
        Ok(Self {
            marker: *marker,
            marker_len: *marker_len,
            language: language.clone(),
            content_start: *content_start,
            source_len: *source_len,
            theme_revision: *theme_revision,
            syntax_identity: highlighter::syntax_identity()?,
            highlighter: highlighter
                .as_ref()
                .map(|x| highlighter::Data::capture(x, code, language))
                .transpose()?,
        })
    }
    pub(crate) fn restore(
        self,
        source: &str,
        equivalent_revision: Option<u64>,
    ) -> io::Result<Option<OpenCodeFence>> {
        boundary(source, self.content_start)?;
        boundary(source, self.source_len)?;
        if self.content_start > self.source_len
            || self.source_len != source.len()
            || !matches!(self.marker, b'`' | b'~')
            || self.marker_len < 3
            || self.language.is_empty()
        {
            return Err(invalid(
                "retained native fence has invalid source bounds or opener",
            ));
        }
        let Some(current_revision) = equivalent_revision else {
            return Ok(None);
        };
        if self.syntax_identity != highlighter::syntax_identity()? {
            return Ok(None);
        }
        let code = &source[self.content_start..self.source_len];
        let highlighter = match self.highlighter {
            Some(data) => {
                match data.restore(code, &self.language, self.theme_revision, current_revision)? {
                    Some(actual) => Some(actual),
                    None => return Ok(None),
                }
            }
            None => None,
        };
        Ok(Some(OpenCodeFence {
            marker: self.marker,
            marker_len: self.marker_len,
            language: self.language,
            content_start: self.content_start,
            source_len: self.source_len,
            theme_revision: current_revision,
            highlighter,
        }))
    }
    pub(crate) fn revision(&self) -> u64 {
        self.theme_revision
    }
}
