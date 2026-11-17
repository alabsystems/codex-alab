// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native message variants, including original cache/tail/source state. No text fallback.
use super::super::markdown_render_cache::alab_retained_cache;
use super::*;
use crate::history_cell::alab_retained_cells::CaptureContext;
use crate::history_cell::alab_retained_cells::RestoreContext;
use crate::inline_visualization::alab_retained_context::ContextData;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) enum Data {
    Reasoning { original: Box<Reasoning> },
    Agent { original: Box<Agent> },
    Markdown { original: Box<Markdown> },
    Tail { original: Box<Agent> },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Reasoning {
    header: String,
    content: String,
    cwd: PathData,
    transcript_only: bool,
    source_item_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Agent {
    lines: Vec<native::HyperlinkData>,
    is_first_line: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Markdown {
    interrupted_item: Option<(String, String)>,
    phase: Option<u8>,
    markdown_source: String,
    copy_source: Option<String>,
    cwd: PathData,
    inline_context: Option<ContextData>,
    rendered_lines: Option<alab_retained_cache::Data>,
    spoken_artifacts: bool,
    question_titles: Vec<std::ops::Range<usize>>,
}
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct PathData(Vec<u8>);
impl PathData {
    fn capture(actual: &Path) -> Self {
        Self(actual.as_os_str().as_bytes().to_vec())
    }
    fn restore(self) -> io::Result<PathBuf> {
        if self.0.contains(&0) {
            return Err(native::invalid("message cwd contains NUL"));
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(self.0)))
    }
}
fn title_ranges(source: &str, titles: &[std::ops::Range<usize>]) -> io::Result<()> {
    if titles.len() > native::MAX_NODES {
        return Err(native::invalid("message title range capacity"));
    }
    let mut previous = 0;
    for title in titles {
        native::range(source, title)?;
        if title.start < previous {
            return Err(native::invalid(
                "message title ranges overlap or go backwards",
            ));
        }
        previous = title.end;
    }
    Ok(())
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        context: &mut CaptureContext<'_>,
    ) -> io::Result<Option<Self>> {
        if let Some(actual) = actual.as_any().downcast_ref::<ReasoningSummaryCell>() {
            return Ok(Some(Self::Reasoning {
                original: Box::new(Reasoning {
                    header: actual._header.clone(),
                    content: actual.content.clone(),
                    cwd: PathData::capture(&actual.cwd),
                    transcript_only: actual.transcript_only,
                    source_item_id: actual.source_item_id.clone(),
                }),
            }));
        }
        if let Some(actual) = actual.as_any().downcast_ref::<AgentMessageCell>() {
            return Ok(Some(Self::Agent {
                original: Box::new(Agent {
                    lines: actual
                        .lines
                        .iter()
                        .map(|line| context.lines().line(line))
                        .collect::<io::Result<_>>()?,
                    is_first_line: actual.is_first_line,
                }),
            }));
        }
        if let Some(actual) = actual.as_any().downcast_ref::<StreamingAgentTailCell>() {
            return Ok(Some(Self::Tail {
                original: Box::new(Agent {
                    lines: actual
                        .lines
                        .iter()
                        .map(|line| context.lines().line(line))
                        .collect::<io::Result<_>>()?,
                    is_first_line: actual.is_first_line,
                }),
            }));
        }
        if let Some(actual) = actual.as_any().downcast_ref::<AgentMarkdownCell>() {
            title_ranges(&actual.markdown_source, &actual.question_titles)?;
            let inline_context = actual
                .inline_visualization_context
                .as_ref()
                .map(|actual| context.inline_context(actual))
                .transpose()?;
            let rendered_lines = actual
                .rendered_lines
                .as_ref()
                .map(|actual| alab_retained_cache::Data::capture(actual, context.lines()))
                .transpose()?;
            return Ok(Some(Self::Markdown {
                original: Box::new(Markdown {
                    interrupted_item: actual.interrupted_item.clone(),
                    phase: actual.phase.as_ref().map(|phase| match phase {
                        codex_protocol::models::MessagePhase::Commentary => 0,
                        codex_protocol::models::MessagePhase::PartialAnswer => 1,
                        codex_protocol::models::MessagePhase::FinalAnswer => 2,
                    }),
                    markdown_source: actual.markdown_source.clone(),
                    copy_source: actual.copy_source.clone(),
                    cwd: PathData::capture(&actual.cwd),
                    inline_context,
                    rendered_lines,
                    spoken_artifacts: actual.spoken_artifacts,
                    question_titles: actual.question_titles.clone(),
                }),
            }));
        }
        Ok(None)
    }
    pub(in crate::history_cell) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
    ) -> io::Result<Box<dyn HistoryCell>> {
        Ok(match self {
            Self::Reasoning { original } => Box::new(ReasoningSummaryCell {
                _header: original.header,
                content: original.content,
                cwd: original.cwd.restore()?,
                transcript_only: original.transcript_only,
                source_item_id: original.source_item_id,
            }),
            Self::Agent { original } => {
                if original.lines.len() > native::MAX_NODES {
                    return Err(native::invalid("agent line capacity"));
                }
                Box::new(AgentMessageCell {
                    lines: original
                        .lines
                        .iter()
                        .map(|line| shared.line(line))
                        .collect::<io::Result<_>>()?,
                    is_first_line: original.is_first_line,
                })
            }
            Self::Tail { original } => {
                if original.lines.len() > native::MAX_NODES {
                    return Err(native::invalid("streaming tail line capacity"));
                }
                Box::new(StreamingAgentTailCell {
                    lines: original
                        .lines
                        .iter()
                        .map(|line| shared.line(line))
                        .collect::<io::Result<_>>()?,
                    is_first_line: original.is_first_line,
                })
            }
            Self::Markdown { original } => {
                title_ranges(&original.markdown_source, &original.question_titles)?;
                if original.rendered_lines.is_some()
                    == crate::inline_visualization::contains_inline_visualization(
                        &original.markdown_source,
                    )
                {
                    return Err(native::invalid(
                        "markdown directive/cache native policy differs",
                    ));
                }
                let phase = original
                    .phase
                    .map(|phase| match phase {
                        0 => Ok(codex_protocol::models::MessagePhase::Commentary),
                        1 => Ok(codex_protocol::models::MessagePhase::PartialAnswer),
                        2 => Ok(codex_protocol::models::MessagePhase::FinalAnswer),
                        _ => Err(native::invalid("unknown message phase")),
                    })
                    .transpose()?;
                let inline_visualization_context = original
                    .inline_context
                    .as_ref()
                    .map(|data| resources.inline_context(data))
                    .transpose()?;
                Box::new(AgentMarkdownCell {
                    interrupted_item: original.interrupted_item,
                    phase,
                    markdown_source: original.markdown_source,
                    copy_source: original.copy_source,
                    cwd: original.cwd.restore()?,
                    inline_visualization_context,
                    rendered_lines: original
                        .rendered_lines
                        .map(|data| data.restore(shared))
                        .transpose()?,
                    spoken_artifacts: original.spoken_artifacts,
                    question_titles: original.question_titles,
                })
            }
        })
    }
}

#[cfg(test)]
#[path = "retained_history_message_tests.rs"]
mod tests;
