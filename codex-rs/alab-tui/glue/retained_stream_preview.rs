// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve the incomplete prose/link scanner without reinterpreting its tail.
use super::*;
use crate::streaming::alab_retained::boundary;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Attribute {
    Key,
    ValueStart,
    Unquoted,
    Quoted(u8),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkData {
    brackets: usize,
    closed_label: bool,
    #[serde(deserialize_with = "required")]
    destination: Option<(usize, usize)>,
    #[serde(deserialize_with = "required")]
    delimiter: Option<u8>,
    whitespace: bool,
    escaped: bool,
    code_ticks: usize,
    pending_ticks: usize,
    #[serde(deserialize_with = "required")]
    code_followup_start: Option<usize>,
    #[serde(deserialize_with = "required")]
    followup_start: Option<usize>,
    followup_brackets: usize,
    followups: VecDeque<Range<usize>>,
    #[serde(deserialize_with = "required")]
    followup_attributes: Option<Attribute>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    lines: Vec<native::HyperlinkData>,
    scanned_len: usize,
    safe_len: usize,
    rich_safe_len: usize,
    has_pipe: bool,
    link: LinkData,
}
impl Data {
    pub(crate) fn capture(actual: &ProsePreview, shared: &mut native::Capture) -> io::Result<Self> {
        let ProsePreview {
            lines,
            scanned_len,
            safe_len,
            rich_safe_len,
            has_pipe,
            link,
        } = actual;
        let LinkPreview {
            brackets,
            closed_label,
            destination,
            delimiter,
            whitespace,
            escaped,
            code_ticks,
            pending_ticks,
            code_followup_start,
            followup_start,
            followup_brackets,
            followups,
            followup_attributes,
        } = link;
        Ok(Self {
            lines: lines
                .iter()
                .map(|x| shared.line(x))
                .collect::<io::Result<_>>()?,
            scanned_len: *scanned_len,
            safe_len: *safe_len,
            rich_safe_len: *rich_safe_len,
            has_pipe: *has_pipe,
            link: LinkData {
                brackets: *brackets,
                closed_label: *closed_label,
                destination: *destination,
                delimiter: *delimiter,
                whitespace: *whitespace,
                escaped: *escaped,
                code_ticks: *code_ticks,
                pending_ticks: *pending_ticks,
                code_followup_start: *code_followup_start,
                followup_start: *followup_start,
                followup_brackets: *followup_brackets,
                followups: followups.clone(),
                followup_attributes: followup_attributes.as_ref().map(|x| match x {
                    AttributeState::Key => Attribute::Key,
                    AttributeState::ValueStart => Attribute::ValueStart,
                    AttributeState::Unquoted => Attribute::Unquoted,
                    AttributeState::Quoted(x) => Attribute::Quoted(*x),
                }),
            },
        })
    }
    pub(crate) fn restore(
        self,
        source: &str,
        shared: &mut native::Restored,
    ) -> io::Result<ProsePreview> {
        // Link scanning counts bytes, so its starts must be bounded even when a
        // multibyte codepoint is currently incomplete at an ASCII delimiter.
        for n in [
            Some(self.scanned_len),
            Some(self.safe_len),
            Some(self.rich_safe_len),
            self.link.destination.map(|x| x.0),
            self.link.code_followup_start,
            self.link.followup_start,
        ]
        .into_iter()
        .flatten()
        {
            boundary(source, n)?;
        }
        if self.safe_len > self.scanned_len || self.rich_safe_len > self.scanned_len {
            return Err(invalid(
                "streaming preview advances beyond actual scanned source",
            ));
        }
        for range in &self.link.followups {
            native::range(source, range)?;
        }
        let link = self.link;
        Ok(ProsePreview {
            lines: self
                .lines
                .iter()
                .map(|x| shared.line(x))
                .collect::<io::Result<_>>()?,
            scanned_len: self.scanned_len,
            safe_len: self.safe_len,
            rich_safe_len: self.rich_safe_len,
            has_pipe: self.has_pipe,
            link: LinkPreview {
                brackets: link.brackets,
                closed_label: link.closed_label,
                destination: link.destination,
                delimiter: link.delimiter,
                whitespace: link.whitespace,
                escaped: link.escaped,
                code_ticks: link.code_ticks,
                pending_ticks: link.pending_ticks,
                code_followup_start: link.code_followup_start,
                followup_start: link.followup_start,
                followup_brackets: link.followup_brackets,
                followups: link.followups,
                followup_attributes: link.followup_attributes.map(|x| match x {
                    Attribute::Key => AttributeState::Key,
                    Attribute::ValueStart => AttributeState::ValueStart,
                    Attribute::Unquoted => AttributeState::Unquoted,
                    Attribute::Quoted(x) => AttributeState::Quoted(x),
                }),
            },
        })
    }
}
