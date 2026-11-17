// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native line/source graph. This is UI DATA, never callback or terminal authority.
use super::*;
use crate::markdown_copy::CopyLine;
use crate::markdown_copy::alab_retained_copy::CopyData;
use crate::transcript_image::TranscriptImage;
use crate::transcript_image::alab_retained_image::ImageData;
use codex_alab_tui::retained_arc_graph::Capture as Identities;
use codex_alab_tui::retained_arc_graph::Id;
use ratatui::layout::Alignment;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::Arc;

pub(crate) const MAX_NODES: usize = 1_000_000;
const MAX_DEPTH: usize = 256;
pub(crate) fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    T::deserialize(d)
}
pub(crate) fn depth(value: usize) -> io::Result<usize> {
    if value >= MAX_DEPTH {
        Err(invalid("native source graph nesting/cycle"))
    } else {
        Ok(value + 1)
    }
}
pub(crate) fn range(text: &str, value: &Range<usize>) -> io::Result<()> {
    if value.start <= value.end && text.get(value.clone()).is_some() {
        Ok(())
    } else {
        Err(invalid(
            "native source range is not valid UTF-8 coordinates",
        ))
    }
}
fn count(value: usize) -> io::Result<()> {
    if value > MAX_NODES {
        Err(invalid("native line graph capacity"))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ColorData([u8; 4]);
impl ColorData {
    fn capture(value: Color) -> Self {
        let tag = match value {
            Color::Reset => 0,
            Color::Black => 1,
            Color::Red => 2,
            Color::Green => 3,
            Color::Yellow => 4,
            Color::Blue => 5,
            Color::Magenta => 6,
            Color::Cyan => 7,
            Color::Gray => 8,
            Color::DarkGray => 9,
            Color::LightRed => 10,
            Color::LightGreen => 11,
            Color::LightYellow => 12,
            Color::LightBlue => 13,
            Color::LightMagenta => 14,
            Color::LightCyan => 15,
            Color::White => 16,
            Color::Rgb(r, g, b) => return Self([17, r, g, b]),
            Color::Indexed(i) => return Self([18, i, 0, 0]),
        };
        Self([tag, 0, 0, 0])
    }
    fn restore(&self) -> io::Result<Color> {
        let [tag, r, g, b] = self.0;
        if tag != 17 && (g != 0 || b != 0 || tag != 18 && r != 0) {
            return Err(invalid("noncanonical native color"));
        }
        Ok(match tag {
            0 => Color::Reset,
            1 => Color::Black,
            2 => Color::Red,
            3 => Color::Green,
            4 => Color::Yellow,
            5 => Color::Blue,
            6 => Color::Magenta,
            7 => Color::Cyan,
            8 => Color::Gray,
            9 => Color::DarkGray,
            10 => Color::LightRed,
            11 => Color::LightGreen,
            12 => Color::LightYellow,
            13 => Color::LightBlue,
            14 => Color::LightMagenta,
            15 => Color::LightCyan,
            16 => Color::White,
            17 => crate::terminal_palette::rgb_color((r, g, b)),
            18 => crate::terminal_palette::indexed_color(r),
            _ => return Err(invalid("unknown native color")),
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StyleData {
    #[serde(deserialize_with = "required")]
    fg: Option<ColorData>,
    #[serde(deserialize_with = "required")]
    bg: Option<ColorData>,
    #[serde(deserialize_with = "required")]
    underline: Option<ColorData>,
    add: u16,
    remove: u16,
}
impl StyleData {
    pub(crate) fn capture(value: Style) -> Self {
        Self {
            fg: value.fg.map(ColorData::capture),
            bg: value.bg.map(ColorData::capture),
            underline: value.underline_color.map(ColorData::capture),
            add: value.add_modifier.bits(),
            remove: value.sub_modifier.bits(),
        }
    }
    pub(crate) fn restore(&self) -> io::Result<Style> {
        Ok(Style {
            fg: self.fg.as_ref().map(ColorData::restore).transpose()?,
            bg: self.bg.as_ref().map(ColorData::restore).transpose()?,
            underline_color: self
                .underline
                .as_ref()
                .map(ColorData::restore)
                .transpose()?,
            add_modifier: Modifier::from_bits(self.add)
                .ok_or_else(|| invalid("unknown native add modifier"))?,
            sub_modifier: Modifier::from_bits(self.remove)
                .ok_or_else(|| invalid("unknown native remove modifier"))?,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpanData {
    text: String,
    style: StyleData,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LineData {
    spans: Vec<SpanData>,
    style: StyleData,
    #[serde(deserialize_with = "required")]
    alignment: Option<u8>,
}
impl LineData {
    pub(crate) fn capture(value: &Line<'_>) -> Self {
        Self {
            spans: value
                .spans
                .iter()
                .map(|span| SpanData {
                    text: span.content.to_string(),
                    style: StyleData::capture(span.style),
                })
                .collect(),
            style: StyleData::capture(value.style),
            alignment: value.alignment.map(|value| match value {
                Alignment::Left => 0,
                Alignment::Center => 1,
                Alignment::Right => 2,
            }),
        }
    }
    pub(crate) fn restore(&self) -> io::Result<Line<'static>> {
        count(self.spans.len())?;
        let mut line = Line::from(
            self.spans
                .iter()
                .map(|span| Ok(Span::styled(span.text.clone(), span.style.restore()?)))
                .collect::<io::Result<Vec<_>>>()?,
        )
        .style(self.style.restore()?);
        line.alignment = self
            .alignment
            .map(|value| match value {
                0 => Ok(Alignment::Left),
                1 => Ok(Alignment::Center),
                2 => Ok(Alignment::Right),
                _ => Err(invalid("unknown native line alignment")),
            })
            .transpose()?;
        Ok(line)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LinkData {
    columns: Range<usize>,
    destination: String,
    trusted_file: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HyperlinkData {
    line: LineData,
    links: Vec<LinkData>,
    #[serde(deserialize_with = "required")]
    source: Option<SourceData>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceData {
    text: Id,
    #[serde(deserialize_with = "required")]
    copy: Option<Id>,
    #[serde(deserialize_with = "required")]
    labelled: Option<Id>,
    prose: bool,
    styles: Id,
    line_style: StyleData,
    span_style: StyleData,
    range: Range<usize>,
    prefix: usize,
    wrap: u8,
    continuation: LineData,
    right_reserve: u16,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LabelData {
    length: usize,
    body: SourceData,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StylesData {
    ranges: Vec<(Range<usize>, StyleData)>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GraphData {
    strings: Vec<String>,
    styles: Vec<StylesData>,
    copies: Vec<CopyData>,
    labelled: Vec<LabelData>,
    tables: Vec<Vec<String>>,
    images: Vec<ImageData>,
}

struct Table<T: ?Sized, D> {
    ids: Identities<T>,
    data: Vec<Option<D>>,
}
impl<T: ?Sized, D> Default for Table<T, D> {
    fn default() -> Self {
        Self {
            ids: Identities::new(MAX_NODES),
            data: Vec::new(),
        }
    }
}
impl<T: ?Sized, D: Clone> Table<T, D> {
    fn start(&mut self, value: &Arc<T>) -> io::Result<(Id, bool)> {
        let (id, fresh) = self.ids.intern(value)?;
        if fresh {
            self.data.push(None);
        } else if self.data[id.0].is_none() {
            return Err(invalid("cyclic native source alias"));
        }
        Ok((id, fresh))
    }
    fn finish(&mut self, id: Id, value: D) {
        self.data[id.0] = Some(value);
    }
    fn export(&self) -> io::Result<Vec<D>> {
        self.data
            .iter()
            .map(|value| {
                value
                    .clone()
                    .ok_or_else(|| invalid("unfinished native source capture"))
            })
            .collect()
    }
}
#[derive(Default)]
pub(crate) struct Capture {
    strings: Table<str, String>,
    styles: Table<[(Range<usize>, Style)], StylesData>,
    copies: Table<CopyLine, CopyData>,
    labelled: Table<(usize, LogicalLineSource), LabelData>,
    tables: Table<[&'static str], Vec<String>>,
    images: Table<TranscriptImage, ImageData>,
}
impl Capture {
    pub(crate) fn string(&mut self, value: &Arc<str>) -> io::Result<Id> {
        let (id, fresh) = self.strings.start(value)?;
        if fresh {
            self.strings.finish(id, value.to_string());
        }
        Ok(id)
    }
    pub(crate) fn source(
        &mut self,
        value: &LogicalLineSource,
        nesting: usize,
    ) -> io::Result<SourceData> {
        let nesting = depth(nesting)?;
        range(&value.text, &value.range)?;
        let text = self.string(&value.text)?;
        let (styles, fresh) = self.styles.start(&value.styles)?;
        if fresh {
            self.styles.finish(
                styles,
                StylesData {
                    ranges: value
                        .styles
                        .iter()
                        .map(|(range, style)| (range.clone(), StyleData::capture(*style)))
                        .collect(),
                },
            );
        }
        let copy = value
            .copy
            .as_ref()
            .map(|copy| self.copy(copy, nesting))
            .transpose()?;
        let labelled = if let Some(actual) = &value.labelled_body {
            let (id, fresh) = self.labelled.start(actual)?;
            if fresh {
                let body = self.source(&actual.1, nesting)?;
                self.labelled.finish(
                    id,
                    LabelData {
                        length: actual.0,
                        body,
                    },
                );
            }
            Some(id)
        } else {
            None
        };
        Ok(SourceData {
            text,
            copy,
            labelled,
            prose: value.copy_as_prose,
            styles,
            line_style: StyleData::capture(value.line_style),
            span_style: StyleData::capture(value.span_style),
            range: value.range.clone(),
            prefix: value.prefix_bytes,
            wrap: match value.wrap_policy {
                LineWrapPolicy::Word => 0,
                LineWrapPolicy::UrlAware => 1,
                LineWrapPolicy::Hard => 2,
            },
            continuation: LineData::capture(&value.continuation_indent),
            right_reserve: value.right_reserve,
        })
    }
    pub(crate) fn copy(&mut self, value: &Arc<CopyLine>, nesting: usize) -> io::Result<Id> {
        let (id, fresh) = self.copies.start(value)?;
        if fresh {
            let data = CopyData::capture(value, self, depth(nesting)?)?;
            self.copies.finish(id, data);
        }
        Ok(id)
    }
    pub(crate) fn table(&mut self, value: &Arc<[&'static str]>) -> io::Result<Id> {
        let (id, fresh) = self.tables.start(value)?;
        if fresh {
            self.tables
                .finish(id, value.iter().map(ToString::to_string).collect());
        }
        Ok(id)
    }
    pub(crate) fn image(&mut self, value: &Arc<TranscriptImage>) -> io::Result<Id> {
        let (id, fresh) = self.images.start(value)?;
        if fresh {
            self.images.finish(id, ImageData::capture(value)?);
        }
        Ok(id)
    }
    pub(crate) fn line(&mut self, value: &HyperlinkLine) -> io::Result<HyperlinkData> {
        Ok(HyperlinkData {
            line: LineData::capture(&value.line),
            links: value
                .hyperlinks
                .iter()
                .map(|link| LinkData {
                    columns: link.columns.clone(),
                    destination: link.destination.clone(),
                    trusted_file: link.destination_kind == DestinationKind::TrustedFile,
                })
                .collect(),
            source: value
                .source
                .as_ref()
                .map(|source| self.source(source, 0))
                .transpose()?,
        })
    }
    pub(crate) fn export(&self) -> io::Result<GraphData> {
        Ok(GraphData {
            strings: self.strings.export()?,
            styles: self.styles.export()?,
            copies: self.copies.export()?,
            labelled: self.labelled.export()?,
            tables: self.tables.export()?,
            images: self.images.export()?,
        })
    }
}

type StyleRanges = Arc<[(Range<usize>, Style)]>;
pub(crate) struct Restored {
    data: GraphData,
    strings: Vec<Arc<str>>,
    styles: Vec<StyleRanges>,
    copies: Vec<Option<Arc<CopyLine>>>,
    labelled: Vec<Option<Arc<(usize, LogicalLineSource)>>>,
    tables: Vec<Arc<[&'static str]>>,
    images: Vec<Arc<TranscriptImage>>,
    visiting_copies: Vec<bool>,
    visiting_labels: Vec<bool>,
}
impl Restored {
    pub(crate) fn new(data: GraphData) -> io::Result<Self> {
        for size in [
            data.strings.len(),
            data.styles.len(),
            data.copies.len(),
            data.labelled.len(),
            data.tables.len(),
            data.images.len(),
        ] {
            count(size)?;
        }
        let strings = data
            .strings
            .iter()
            .map(|value| Arc::<str>::from(value.as_str()))
            .collect();
        let styles = data
            .styles
            .iter()
            .map(|value| {
                count(value.ranges.len())?;
                Ok(value
                    .ranges
                    .iter()
                    .map(|(range, style)| Ok((range.clone(), style.restore()?)))
                    .collect::<io::Result<Vec<_>>>()?
                    .into())
            })
            .collect::<io::Result<Vec<_>>>()?;
        let tables = data
            .tables
            .iter()
            .map(|table| {
                count(table.len())?;
                table
                    .iter()
                    .map(|value| match value.as_str() {
                        "---" => Ok("---"),
                        ":---" => Ok(":---"),
                        ":---:" => Ok(":---:"),
                        "---:" => Ok("---:"),
                        _ => Err(invalid("unknown original native table alignment")),
                    })
                    .collect::<io::Result<Vec<_>>>()
                    .map(Arc::from)
            })
            .collect::<io::Result<Vec<_>>>()?;
        let images = data
            .images
            .iter()
            .map(|image| image.restore().map(Arc::new))
            .collect::<io::Result<Vec<_>>>()?;
        let mut result = Self {
            strings,
            styles,
            copies: vec![None; data.copies.len()],
            labelled: vec![None; data.labelled.len()],
            visiting_copies: vec![false; data.copies.len()],
            visiting_labels: vec![false; data.labelled.len()],
            tables,
            images,
            data,
        };
        // Validate all payloads, including presently unreferenced entries, before exposing native DATA.
        for i in 0..result.copies.len() {
            result.copy(Id(i), 0)?;
        }
        for i in 0..result.labelled.len() {
            result.label(Id(i), 0)?;
        }
        Ok(result)
    }
    pub(crate) fn string(&self, id: Id) -> io::Result<Arc<str>> {
        self.strings
            .get(id.0)
            .cloned()
            .ok_or_else(|| invalid("unknown native string alias"))
    }
    pub(crate) fn table(&self, id: Id) -> io::Result<Arc<[&'static str]>> {
        self.tables
            .get(id.0)
            .cloned()
            .ok_or_else(|| invalid("unknown native table alias"))
    }
    pub(crate) fn image(&self, id: Id) -> io::Result<Arc<TranscriptImage>> {
        self.images
            .get(id.0)
            .cloned()
            .ok_or_else(|| invalid("unknown native image alias"))
    }
    fn label(&mut self, id: Id, nesting: usize) -> io::Result<Arc<(usize, LogicalLineSource)>> {
        let nesting = depth(nesting)?;
        let slot = self
            .labelled
            .get(id.0)
            .ok_or_else(|| invalid("unknown native labelled alias"))?;
        if let Some(value) = slot {
            return Ok(value.clone());
        }
        if std::mem::replace(&mut self.visiting_labels[id.0], true) {
            return Err(invalid("cyclic labelled source"));
        }
        let data = self.data.labelled[id.0].clone();
        let body = self.source(&data.body, nesting)?;
        let value = Arc::new((data.length, body));
        self.labelled[id.0] = Some(value.clone());
        self.visiting_labels[id.0] = false;
        Ok(value)
    }
    pub(crate) fn copy(&mut self, id: Id, nesting: usize) -> io::Result<Arc<CopyLine>> {
        let nesting = depth(nesting)?;
        let slot = self
            .copies
            .get(id.0)
            .ok_or_else(|| invalid("unknown native copy alias"))?;
        if let Some(value) = slot {
            return Ok(value.clone());
        }
        if std::mem::replace(&mut self.visiting_copies[id.0], true) {
            return Err(invalid("cyclic native copy source"));
        }
        let data = self.data.copies[id.0].clone();
        let value = Arc::new(data.restore(self, nesting)?);
        self.copies[id.0] = Some(value.clone());
        self.visiting_copies[id.0] = false;
        Ok(value)
    }
    pub(crate) fn source(
        &mut self,
        data: &SourceData,
        nesting: usize,
    ) -> io::Result<LogicalLineSource> {
        let nesting = depth(nesting)?;
        let text = self.string(data.text)?;
        range(&text, &data.range)?;
        let styles = self
            .styles
            .get(data.styles.0)
            .cloned()
            .ok_or_else(|| invalid("unknown native styles alias"))?;
        for (style_range, _) in styles.iter() {
            range(&text, style_range)?;
        }
        let copy = data.copy.map(|id| self.copy(id, nesting)).transpose()?;
        let labelled_body = data
            .labelled
            .map(|id| self.label(id, nesting))
            .transpose()?;
        if let Some(labelled) = &labelled_body
            && (!text.is_char_boundary(labelled.0)
                || labelled.1.text.len() > text.len()
                || labelled.0 > text.len().saturating_sub(labelled.1.text.len())
                || !text.ends_with(labelled.1.text.as_ref()))
        {
            return Err(invalid("invalid native labelled source body/coordinates"));
        }
        // A labelled parent still shares the BODY copy coordinates. The native
        // SelectedLine path splits it first; its body was recursively validated.
        if labelled_body.is_none()
            && let Some(copy) = &copy
        {
            CopyData::validate_source(copy, &text)?;
        }
        data.prefix
            .checked_add(data.range.len())
            .ok_or_else(|| invalid("native prefix overflow"))?;
        Ok(LogicalLineSource {
            text,
            copy,
            labelled_body,
            copy_as_prose: data.prose,
            styles,
            line_style: data.line_style.restore()?,
            span_style: data.span_style.restore()?,
            range: data.range.clone(),
            prefix_bytes: data.prefix,
            wrap_policy: match data.wrap {
                0 => LineWrapPolicy::Word,
                1 => LineWrapPolicy::UrlAware,
                2 => LineWrapPolicy::Hard,
                _ => return Err(invalid("unknown native wrapping policy")),
            },
            continuation_indent: data.continuation.restore()?,
            right_reserve: data.right_reserve,
        })
    }
    pub(crate) fn line(&mut self, data: &HyperlinkData) -> io::Result<HyperlinkLine> {
        count(data.links.len())?;
        let line = data.line.restore()?;
        let source = data
            .source
            .as_ref()
            .map(|value| self.source(value, 0))
            .transpose()?;
        if let Some(source) = &source {
            let displayed: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            if source
                .prefix_bytes
                .checked_add(source.range.len())
                .is_none_or(|end| displayed.get(source.prefix_bytes..end).is_none())
            {
                return Err(invalid("native line source exceeds displayed bytes"));
            }
        }
        let hyperlinks = data
            .links
            .iter()
            .map(|link| {
                if link.columns.start > link.columns.end {
                    return Err(invalid("native hyperlink reversed columns"));
                }
                Ok(TerminalHyperlink {
                    columns: link.columns.clone(),
                    destination: link.destination.clone(),
                    destination_kind: if link.trusted_file {
                        DestinationKind::TrustedFile
                    } else {
                        DestinationKind::Web
                    },
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(HyperlinkLine {
            line,
            source,
            hyperlinks,
        })
    }
}

// Standalone native-link DATA for original cells that store a link without a line.
// TrustedFile remains an existing UI annotation, never a file/resource permission.
impl LinkData {
    pub(crate) fn capture(value: &TerminalHyperlink) -> Self {
        Self {
            columns: value.columns.clone(),
            destination: value.destination.clone(),
            trusted_file: value.destination_kind == DestinationKind::TrustedFile,
        }
    }
    pub(crate) fn restore(&self) -> io::Result<TerminalHyperlink> {
        if self.columns.start > self.columns.end {
            return Err(invalid("native hyperlink reversed columns"));
        }
        Ok(TerminalHyperlink {
            columns: self.columns.clone(),
            destination: self.destination.clone(),
            destination_kind: if self.trusted_file {
                DestinationKind::TrustedFile
            } else {
                DestinationKind::Web
            },
        })
    }
}
#[cfg(test)]
#[path = "retained_text_links_tests.rs"]
mod standalone_link_tests;
