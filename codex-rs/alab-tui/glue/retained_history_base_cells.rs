// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native base fields, never a fallback for another HistoryCell implementation.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use ratatui::layout::Alignment;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::history_cell) struct PlainData {
    lines: Vec<native::LineData>,
}
impl PlainData {
    pub(in crate::history_cell) fn capture(actual: &PlainHistoryCell) -> Self {
        Self {
            lines: actual.lines.iter().map(native::LineData::capture).collect(),
        }
    }
    pub(in crate::history_cell) fn restore(self) -> io::Result<PlainHistoryCell> {
        Ok(PlainHistoryCell {
            lines: self
                .lines
                .iter()
                .map(native::LineData::restore)
                .collect::<io::Result<_>>()?,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextData {
    lines: Vec<native::LineData>,
    style: native::StyleData,
    alignment: Option<u8>,
}
impl TextData {
    fn capture(actual: &Text<'_>) -> Self {
        Self {
            lines: actual.lines.iter().map(native::LineData::capture).collect(),
            style: native::StyleData::capture(actual.style),
            alignment: actual.alignment.map(|value| match value {
                Alignment::Left => 0,
                Alignment::Center => 1,
                Alignment::Right => 2,
            }),
        }
    }
    fn restore(self) -> io::Result<Text<'static>> {
        let alignment = self
            .alignment
            .map(|value| match value {
                0 => Ok(Alignment::Left),
                1 => Ok(Alignment::Center),
                2 => Ok(Alignment::Right),
                _ => Err(native::invalid("unknown native Text alignment")),
            })
            .transpose()?;
        Ok(Text {
            lines: self
                .lines
                .iter()
                .map(native::LineData::restore)
                .collect::<io::Result<_>>()?,
            style: self.style.restore()?,
            alignment,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::history_cell) struct PrefixedData {
    text: TextData,
    initial: native::LineData,
    subsequent: native::LineData,
}
impl PrefixedData {
    pub(in crate::history_cell) fn capture(actual: &PrefixedWrappedHistoryCell) -> Self {
        Self {
            text: TextData::capture(&actual.text),
            initial: native::LineData::capture(&actual.initial_prefix),
            subsequent: native::LineData::capture(&actual.subsequent_prefix),
        }
    }
    pub(in crate::history_cell) fn restore(self) -> io::Result<PrefixedWrappedHistoryCell> {
        Ok(PrefixedWrappedHistoryCell {
            text: self.text.restore()?,
            initial_prefix: self.initial.restore()?,
            subsequent_prefix: self.subsequent.restore()?,
        })
    }
}

use crate::history_cell::alab_retained_cells as cells;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::history_cell) struct CompositeData {
    parts: Vec<cells::Body>,
}
impl CompositeData {
    pub(in crate::history_cell) fn capture(
        actual: &CompositeHistoryCell,
        context: &mut cells::CaptureContext<'_>,
        nesting: usize,
    ) -> io::Result<Self> {
        let next = cells::depth(nesting)?;
        Ok(Self {
            parts: actual
                .parts
                .iter()
                .map(|part| cells::Body::capture(part.as_ref(), context, next))
                .collect::<io::Result<_>>()?,
        })
    }
    pub(in crate::history_cell) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut cells::RestoreContext,
        nesting: usize,
    ) -> io::Result<CompositeHistoryCell> {
        let next = cells::depth(nesting)?;
        Ok(CompositeHistoryCell {
            parts: self
                .parts
                .into_iter()
                .map(|part| part.restore(shared, resources, next))
                .collect::<io::Result<_>>()?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) enum Data {
    Plain { original: PlainData },
    Labeled { original: PlainData, label: String },
    Web { lines: Vec<native::HyperlinkData> },
    Prefixed { original: PrefixedData },
    Composite { original: CompositeData },
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        context: &mut cells::CaptureContext<'_>,
        nesting: usize,
    ) -> io::Result<Option<Self>> {
        cells::depth(nesting)?;
        let original = if let Some(actual) = actual.as_any().downcast_ref::<PlainHistoryCell>() {
            Self::Plain {
                original: PlainData::capture(actual),
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<LabeledToolCell>() {
            Self::Labeled {
                original: PlainData::capture(&actual.cell),
                label: actual.label.clone(),
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<WebHyperlinkHistoryCell>() {
            Self::Web {
                lines: actual
                    .lines
                    .iter()
                    .map(|line| context.lines().line(line))
                    .collect::<io::Result<_>>()?,
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<PrefixedWrappedHistoryCell>() {
            Self::Prefixed {
                original: PrefixedData::capture(actual),
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<CompositeHistoryCell>() {
            Self::Composite {
                original: CompositeData::capture(actual, context, nesting)?,
            }
        } else {
            return Ok(None);
        };
        Ok(Some(original))
    }
    pub(in crate::history_cell) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut cells::RestoreContext,
        nesting: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        cells::depth(nesting)?;
        Ok(match self {
            Self::Plain { original } => Box::new(original.restore()?),
            Self::Labeled { original, label } => Box::new(LabeledToolCell {
                cell: original.restore()?,
                label,
            }),
            Self::Web { lines } => Box::new(WebHyperlinkHistoryCell {
                lines: lines
                    .iter()
                    .map(|line| shared.line(line))
                    .collect::<io::Result<_>>()?,
            }),
            Self::Prefixed { original } => Box::new(original.restore()?),
            Self::Composite { original } => Box::new(original.restore(shared, resources, nesting)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use ratatui::style::Stylize;
    #[test]
    fn original_prefixed_native_cell_preserves_text_prefixes_styles_and_alignment() -> io::Result<()>
    {
        let mut text = Text::from(vec![Line::from(vec![
            "α β".red().bold(),
            " gamma δ".on_blue(),
        ])]);
        text.style = text.style.fg(Color::Green);
        text.alignment = Some(Alignment::Right);
        let original = PrefixedWrappedHistoryCell::new(
            text,
            Line::from("first> ").blue(),
            Line::from("next> ").fg(ratatui::style::Color::Yellow),
        );
        let raw = serde_json::to_vec(&PrefixedData::capture(&original)).map_err(native::invalid)?;
        let data: PrefixedData = serde_json::from_slice(&raw).map_err(native::invalid)?;
        let restored = data.restore()?;
        assert_eq!(restored.text, original.text);
        assert_eq!(restored.initial_prefix, original.initial_prefix);
        assert_eq!(restored.subsequent_prefix, original.subsequent_prefix);
        for width in [0, 4, 20, 80] {
            assert_eq!(restored.display_lines(width), original.display_lines(width));
            assert_eq!(restored.raw_lines(), original.raw_lines());
        }
        Ok(())
    }
    #[test]
    fn original_plain_cell_retains_span_content_and_styles_as_its_actual_native_type()
    -> io::Result<()> {
        let original = PlainHistoryCell::new(vec![
            Line::from(vec!["same".bold(), " same".dim()]).alignment(Alignment::Center),
        ]);
        let restored = PlainData::capture(&original).restore()?;
        assert_eq!(restored.lines, original.lines);
        assert!(
            (&restored as &dyn HistoryCell)
                .as_any()
                .is::<PlainHistoryCell>()
        );
        Ok(())
    }
}

#[cfg(test)]
mod graph_tests {
    use super::*;
    use std::sync::Arc;
    const LIMIT: usize = 1_048_576;
    #[test]
    fn composite_restores_original_unique_box_types_and_tool_labels() -> io::Result<()> {
        let actual: Arc<dyn HistoryCell> = Arc::new(CompositeHistoryCell::new(vec![
            Box::new(PlainHistoryCell::new(vec![Line::from("first")])),
            Box::new(
                PlainHistoryCell::new(vec![Line::from("second")])
                    .with_tool_label("original tool".into()),
            ),
        ]));
        let mut graph = cells::Capture::default();
        let key = graph.intern(&actual)?;
        let mut text = native::Capture::default();
        let raw = graph.encode(&mut text, LIMIT)?;
        let mut text = native::Restored::new(text.export()?)?;
        let restored = cells::Restored::decode(
            &raw,
            LIMIT,
            &mut text,
            &mut cells::RestoreContext::default(),
        )?;
        let root = restored.cell(key)?;
        let original = root
            .as_any()
            .downcast_ref::<CompositeHistoryCell>()
            .unwrap();
        assert_eq!(original.parts.len(), 2);
        assert!(original.parts[0].as_any().is::<PlainHistoryCell>());
        assert!(original.parts[1].as_any().is::<LabeledToolCell>());
        assert_eq!(
            original.parts[1].tool_call_labels(),
            vec![String::from("original tool")]
        );
        assert_eq!(root.raw_lines(), actual.raw_lines());
        assert!(Arc::ptr_eq(&root, &restored.cell(key)?));
        Ok(())
    }
    #[test]
    fn excessive_original_box_nesting_refuses_without_render_fallback() -> io::Result<()> {
        let mut actual: Box<dyn HistoryCell> =
            Box::new(PlainHistoryCell::new(vec![Line::from("leaf")]));
        for _ in 0..300 {
            actual = Box::new(CompositeHistoryCell::new(vec![actual]));
        }
        let actual: Arc<dyn HistoryCell> = Arc::from(actual);
        let mut graph = cells::Capture::default();
        graph.intern(&actual)?;
        assert!(
            graph
                .encode(&mut native::Capture::default(), LIMIT)
                .is_err()
        );
        Ok(())
    }
}

#[cfg(test)]
mod shared_line_tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn actual_web_cells_share_the_same_original_source_generation_after_global_restore()
    -> io::Result<()> {
        let source = crate::terminal_hyperlinks::LogicalLineSource::new("same source".into());
        let line = crate::terminal_hyperlinks::HyperlinkLine {
            line: Line::from("same source"),
            hyperlinks: Vec::new(),
            source: Some(source),
        };
        let first: Arc<dyn HistoryCell> =
            Arc::new(WebHyperlinkHistoryCell::new_hyperlink_lines(vec![
                line.clone(),
            ]));
        let second: Arc<dyn HistoryCell> =
            Arc::new(WebHyperlinkHistoryCell::new_hyperlink_lines(vec![line]));
        let mut graph = cells::Capture::default();
        let a = graph.intern(&first)?;
        let b = graph.intern(&second)?;
        let mut text = native::Capture::default();
        let raw = graph.encode(&mut text, 1_048_576)?;
        let mut text = native::Restored::new(text.export()?)?;
        let restored = cells::Restored::decode(
            &raw,
            1_048_576,
            &mut text,
            &mut cells::RestoreContext::default(),
        )?;
        let a = restored.cell(a)?;
        let b = restored.cell(b)?;
        assert!(!Arc::ptr_eq(&a, &b));
        let a = a
            .as_any()
            .downcast_ref::<WebHyperlinkHistoryCell>()
            .unwrap();
        let b = b
            .as_any()
            .downcast_ref::<WebHyperlinkHistoryCell>()
            .unwrap();
        assert!(Arc::ptr_eq(
            &a.lines[0].source.as_ref().unwrap().text,
            &b.lines[0].source.as_ref().unwrap().text
        ));
        Ok(())
    }
}
