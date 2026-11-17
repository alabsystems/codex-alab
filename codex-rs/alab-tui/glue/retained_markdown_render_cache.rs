// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! The exact native cache key and rich cached rows; decoding never re-renders.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    schema: u8,
    cached: Option<Cached>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cached {
    width: u16,
    list_spacing: u8,
    syntax_theme_revision: u64,
    terminal_fg: Option<(u8, u8, u8)>,
    terminal_bg: Option<(u8, u8, u8)>,
    color_level: u8,
    lines: Vec<native::HyperlinkData>,
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &MarkdownRenderCache,
        shared: &mut native::Capture,
    ) -> io::Result<Self> {
        let cached = actual
            .cached
            .lock()
            .map_err(|_| native::invalid("native markdown cache is poisoned"))?;
        let cached = cached
            .as_ref()
            .map(|(key, lines)| {
                Ok::<_, io::Error>(Cached {
                    width: key.width,
                    list_spacing: match key.list_spacing {
                        ListSpacing::AfterMultiline => 0,
                        ListSpacing::Compact => 1,
                        ListSpacing::Uniform => 2,
                    },
                    syntax_theme_revision: key.syntax_theme_revision,
                    terminal_fg: key.terminal_fg,
                    terminal_bg: key.terminal_bg,
                    color_level: match key.color_level {
                        crate::terminal_palette::StdoutColorLevel::TrueColor => 0,
                        crate::terminal_palette::StdoutColorLevel::Ansi256 => 1,
                        crate::terminal_palette::StdoutColorLevel::Ansi16 => 2,
                        crate::terminal_palette::StdoutColorLevel::Unknown => 3,
                    },
                    lines: lines
                        .iter()
                        .map(|line| shared.line(line))
                        .collect::<io::Result<_>>()?,
                })
            })
            .transpose()?;
        Ok(Self { schema: 1, cached })
    }

    pub(in crate::history_cell) fn restore(
        self,
        shared: &mut native::Restored,
    ) -> io::Result<MarkdownRenderCache> {
        if self.schema != 1 {
            return Err(native::invalid("native markdown cache version"));
        }
        let cached = self
            .cached
            .map(|cached| {
                if cached.lines.len() > native::MAX_NODES {
                    return Err(native::invalid("native markdown cached-line capacity"));
                }
                let key = MarkdownRenderCacheKey {
                    width: cached.width,
                    list_spacing: match cached.list_spacing {
                        0 => ListSpacing::AfterMultiline,
                        1 => ListSpacing::Compact,
                        2 => ListSpacing::Uniform,
                        _ => return Err(native::invalid("unknown native list-spacing policy")),
                    },
                    syntax_theme_revision: cached.syntax_theme_revision,
                    terminal_fg: cached.terminal_fg,
                    terminal_bg: cached.terminal_bg,
                    color_level: match cached.color_level {
                        0 => crate::terminal_palette::StdoutColorLevel::TrueColor,
                        1 => crate::terminal_palette::StdoutColorLevel::Ansi256,
                        2 => crate::terminal_palette::StdoutColorLevel::Ansi16,
                        3 => crate::terminal_palette::StdoutColorLevel::Unknown,
                        _ => return Err(native::invalid("unknown native stdout color level")),
                    },
                };
                let lines = cached
                    .lines
                    .iter()
                    .map(|line| shared.line(line))
                    .collect::<io::Result<_>>()?;
                Ok::<_, io::Error>((key, lines))
            })
            .transpose()?;
        Ok(MarkdownRenderCache {
            cached: Mutex::new(cached),
        })
    }
}
