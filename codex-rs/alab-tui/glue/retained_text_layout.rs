// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve the actual held width/revision, rows and native aliases. Restore does
//! not call TextLayout::new, from_logical, rewrap, wrap_line or any renderer.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::retained_arc_graph::Capture as Identities;
use codex_alab_tui::retained_arc_graph::Id;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::Arc;
use tabs::alab_retained_tabs::StopsData;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct LayoutRef(Id);
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LogicalData {
    line: native::HyperlinkData,
    initial: native::LineData,
    subsequent: native::LineData,
    wrap: u8,
    right_reserve: u16,
    origin: native::SourceData,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RowData {
    line: native::HyperlinkData,
    source: Range<usize>,
    #[serde(deserialize_with = "required")]
    line_end: Option<usize>,
    content_width: u16,
    first_column: usize,
    prefix_columns: usize,
    tabs: StopsData,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum AnchorData {
    Activity { offset: usize },
    Turn {},
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DisclosureData {
    anchor: AnchorData,
    label: String,
    row: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LayoutData {
    #[serde(deserialize_with = "required")]
    image: Option<Id>,
    logical: Vec<LogicalData>,
    text: String,
    rows: Vec<RowData>,
    width: u16,
    separated: bool,
    disclosure: bool,
    leading_blank_hidden: bool,
    #[serde(deserialize_with = "required")]
    control: Option<DisclosureData>,
}
/// Exact native by-value owner; no artificial Arc/layout identity is registered.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct UniqueLayout(LayoutData);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Data {
    schema: u8,
    native: native::GraphData,
    layouts: Vec<LayoutData>,
}

pub(crate) struct Capture {
    ids: Identities<TextLayout>,
    native: native::Capture,
    layouts: Vec<LayoutData>,
}
impl Default for Capture {
    fn default() -> Self {
        Self {
            ids: Identities::new(native::MAX_NODES),
            native: native::Capture::default(),
            layouts: Vec::new(),
        }
    }
}
impl Capture {
    /// All whole-App HyperlinkLine/copy/source fields must use this SAME table.
    pub(crate) fn lines(&mut self) -> &mut native::Capture {
        &mut self.native
    }
    pub(crate) fn unique_layout(&mut self, actual: &TextLayout) -> io::Result<UniqueLayout> {
        LayoutData::capture(actual, &mut self.native).map(UniqueLayout)
    }
    pub(crate) fn intern(&mut self, actual: &Arc<TextLayout>) -> io::Result<LayoutRef> {
        let (id, fresh) = self.ids.intern(actual)?;
        if !fresh && id.0 >= self.layouts.len() {
            return Err(native::invalid("unfinished native layout capture"));
        }
        if fresh {
            if id.0 != self.layouts.len() {
                return Err(native::invalid("unfinished native layout capture"));
            }
            let layout = LayoutData::capture(actual, &mut self.native)?;
            self.layouts.push(layout);
        }
        Ok(LayoutRef(id))
    }
    pub(crate) fn encode(&self, max_bytes: usize) -> io::Result<Vec<u8>> {
        if self.ids.len() != self.layouts.len() {
            return Err(native::invalid("unfinished native layout capture"));
        }
        let data = Data {
            schema: 1,
            native: self.native.export()?,
            layouts: self.layouts.clone(),
        };
        let bytes = serde_json::to_vec(&data).map_err(native::invalid)?;
        if max_bytes == 0 || bytes.len() > max_bytes {
            return Err(native::invalid("native layout byte budget"));
        }
        Ok(bytes)
    }
}
pub(crate) struct Restored {
    native: native::Restored,
    layouts: Vec<Arc<TextLayout>>,
}
impl Restored {
    pub(crate) fn decode(bytes: &[u8], max_bytes: usize) -> io::Result<Self> {
        if max_bytes == 0 || bytes.len() > max_bytes {
            return Err(native::invalid("native layout byte budget"));
        }
        let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(native::invalid)?;
        let data: Data = serde_json::from_slice(bytes).map_err(native::invalid)?;
        // Required explicit nulls and exact enum/field shape, including optional native fields.
        if raw != serde_json::to_value(&data).map_err(native::invalid)?
            || data.schema != 1
            || data.layouts.len() > native::MAX_NODES
        {
            return Err(native::invalid(
                "native layout schema/required field/capacity",
            ));
        }
        let mut native = native::Restored::new(data.native)?;
        let layouts = data
            .layouts
            .iter()
            .map(|value| value.restore(&mut native).map(Arc::new))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self { native, layouts })
    }
    pub(crate) fn restore_unique_layout(&mut self, data: UniqueLayout) -> io::Result<TextLayout> {
        data.0.restore(&mut self.native)
    }
    pub(crate) fn layout(&self, id: LayoutRef) -> io::Result<Arc<TextLayout>> {
        self.layouts
            .get(id.0.0)
            .cloned()
            .ok_or_else(|| native::invalid("unknown actual native layout identity"))
    }
    /// Read a text alias from the SAME native graph used by all retained layouts.
    pub(crate) fn text(&self, id: codex_alab_tui::retained_arc_graph::Id) -> io::Result<Arc<str>> {
        self.native.string(id)
    }
    /// Use for the other whole-App native fields encoded through Capture::lines.
    pub(crate) fn lines(&mut self) -> &mut native::Restored {
        &mut self.native
    }
}
impl LayoutData {
    fn capture(value: &TextLayout, graph: &mut native::Capture) -> io::Result<Self> {
        let logical = value
            .logical
            .iter()
            .map(|value| {
                Ok(LogicalData {
                    line: graph.line(&value.line)?,
                    initial: native::LineData::capture(&value.initial_indent),
                    subsequent: native::LineData::capture(&value.subsequent_indent),
                    wrap: match value.wrap_policy {
                        LineWrapPolicy::Word => 0,
                        LineWrapPolicy::UrlAware => 1,
                        LineWrapPolicy::Hard => 2,
                    },
                    right_reserve: value.right_reserve,
                    origin: graph.source(&value.origin, 0)?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let rows = value
            .rows
            .iter()
            .map(|value| {
                Ok(RowData {
                    line: graph.line(&value.line)?,
                    source: value.source.clone(),
                    line_end: value.line_end,
                    content_width: value.content_width,
                    first_column: value.first_column,
                    prefix_columns: value.prefix_columns,
                    tabs: StopsData::capture(&value.tabs),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            image: value
                .image
                .as_ref()
                .map(|value| graph.image(value))
                .transpose()?,
            logical,
            text: value.text.clone(),
            rows,
            width: value.width,
            separated: value.separated,
            disclosure: value.disclosure,
            leading_blank_hidden: value.leading_blank_hidden,
            control: value
                .disclosure_control
                .as_ref()
                .map(|value| DisclosureData {
                    anchor: match value.anchor {
                        DisclosureAnchor::Activity(offset) => AnchorData::Activity { offset },
                        DisclosureAnchor::Turn => AnchorData::Turn {},
                    },
                    label: value.label.clone(),
                    row: value.row,
                }),
        })
    }
    fn restore(&self, graph: &mut native::Restored) -> io::Result<TextLayout> {
        if self.width == 0
            || self.logical.len() > native::MAX_NODES
            || self.rows.len() > native::MAX_NODES
        {
            return Err(native::invalid("invalid native layout dimensions/capacity"));
        }
        let logical = self
            .logical
            .iter()
            .map(|value| {
                Ok(LogicalLine {
                    line: graph.line(&value.line)?,
                    initial_indent: value.initial.restore()?,
                    subsequent_indent: value.subsequent.restore()?,
                    wrap_policy: match value.wrap {
                        0 => LineWrapPolicy::Word,
                        1 => LineWrapPolicy::UrlAware,
                        2 => LineWrapPolicy::Hard,
                        _ => return Err(native::invalid("unknown logical native wrap policy")),
                    },
                    right_reserve: value.right_reserve,
                    origin: graph.source(&value.origin, 0)?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let rows = self
            .rows
            .iter()
            .map(|value| {
                native::range(&self.text, &value.source)?;
                if let Some(end) = value.line_end
                    && (end < value.source.end || !self.text.is_char_boundary(end))
                {
                    return Err(native::invalid("invalid native hard-line end"));
                }
                if value.content_width > self.width
                    || value.first_column > usize::from(self.width)
                    || value.prefix_columns > value.first_column
                {
                    return Err(native::invalid("invalid native row geometry"));
                }
                Ok(TextRow {
                    line: graph.line(&value.line)?,
                    source: value.source.clone(),
                    line_end: value.line_end,
                    content_width: value.content_width,
                    first_column: value.first_column,
                    prefix_columns: value.prefix_columns,
                    tabs: value.tabs.restore(&self.text[value.source.clone()])?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let disclosure_control = self
            .control
            .as_ref()
            .map(|value| {
                if value.row >= rows.len() {
                    return Err(native::invalid("missing native disclosure row"));
                }
                let anchor = match value.anchor {
                    AnchorData::Activity { offset } => {
                        if !self.text.is_char_boundary(offset) {
                            return Err(native::invalid("invalid native disclosure source offset"));
                        }
                        DisclosureAnchor::Activity(offset)
                    }
                    AnchorData::Turn {} => {
                        if value.row == 0 {
                            return Err(native::invalid("missing native turn disclosure spacer"));
                        }
                        DisclosureAnchor::Turn
                    }
                };
                Ok(DisclosureControl {
                    anchor,
                    label: value.label.clone(),
                    row: value.row,
                })
            })
            .transpose()?;
        Ok(TextLayout {
            image: self.image.map(|id| graph.image(id)).transpose()?,
            logical,
            text: self.text.clone(),
            rows,
            width: self.width,
            separated: self.separated,
            disclosure: self.disclosure,
            leading_blank_hidden: self.leading_blank_hidden,
            disclosure_control,
        })
    }
}
#[cfg(test)]
#[path = "retained_text_layout_tests.rs"]
mod tests;

fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(decoder)
}
