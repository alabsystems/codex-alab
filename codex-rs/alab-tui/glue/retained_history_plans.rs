// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native plan stream/update fields. Original activity IDs and rich line aliases
//! survive without new UUIDs, Markdown parsing or source/render fallbacks.
use super::*;
use crate::history_cell::markdown_render_cache::alab_retained_cache::Data as CacheData;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::history_cell) struct PathData(Vec<u8>);
impl PathData {
    pub(in crate::history_cell) fn capture(path: &Path) -> Self {
        Self(path.as_os_str().as_bytes().to_vec())
    }
    pub(in crate::history_cell) fn bytes(&self) -> &[u8] {
        &self.0
    }
    pub(in crate::history_cell) fn restore(self) -> io::Result<PathBuf> {
        if self.0.contains(&0) {
            return Err(native::invalid("native history path contains NUL"));
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(self.0)))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum StatusData {
    Pending {},
    InProgress {},
    Completed {},
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItemData {
    step: String,
    status: StatusData,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Body {
    Proposed {
        markdown: String,
        cwd: PathData,
        rendered: CacheData,
    },
    StreamingTail {
        lines: Vec<native::HyperlinkData>,
        continuation: bool,
    },
    ProposedStream {
        lines: Vec<native::HyperlinkData>,
        continuation: bool,
    },
    Update {
        activity_id: String,
        explanation: Option<String>,
        plan: Vec<ItemData>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    schema: u8,
    body: Body,
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        graph: &mut native::Capture,
        depth: usize,
    ) -> io::Result<Option<Self>> {
        native::depth(depth)?;
        let body = if let Some(actual) = actual.as_any().downcast_ref::<ProposedPlanCell>() {
            Body::Proposed {
                markdown: actual.plan_markdown.clone(),
                cwd: PathData::capture(&actual.cwd),
                rendered: CacheData::capture(&actual.rendered_lines, graph)?,
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<StreamingPlanTailCell>() {
            Body::StreamingTail {
                lines: actual
                    .lines
                    .iter()
                    .map(|line| graph.line(line))
                    .collect::<io::Result<_>>()?,
                continuation: actual.is_stream_continuation,
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<ProposedPlanStreamCell>() {
            Body::ProposedStream {
                lines: actual
                    .lines
                    .iter()
                    .map(|line| graph.line(line))
                    .collect::<io::Result<_>>()?,
                continuation: actual.is_stream_continuation,
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<PlanUpdateCell>() {
            Body::Update {
                activity_id: actual.activity_id.clone(),
                explanation: actual.explanation.clone(),
                plan: actual
                    .plan
                    .iter()
                    .map(|item| ItemData {
                        step: item.step.clone(),
                        status: match item.status {
                            StepStatus::Pending => StatusData::Pending {},
                            StepStatus::InProgress => StatusData::InProgress {},
                            StepStatus::Completed => StatusData::Completed {},
                        },
                    })
                    .collect(),
            }
        } else {
            return Ok(None);
        };
        Ok(Some(Self { schema: 1, body }))
    }
    pub(in crate::history_cell) fn restore(
        self,
        graph: &mut native::Restored,
        depth: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        native::depth(depth)?;
        if self.schema != 1 {
            return Err(native::invalid("native plan cell version"));
        }
        match self.body {
            Body::Proposed {
                markdown,
                cwd,
                rendered,
            } => {
                // Historical cwd remains original render DATA. No filesystem read,
                // Config/InlineVisualizationContext or native role is constructed here.
                Ok(Box::new(ProposedPlanCell {
                    plan_markdown: markdown,
                    cwd: cwd.restore()?,
                    rendered_lines: rendered.restore(graph)?,
                }))
            }
            Body::StreamingTail {
                lines,
                continuation,
            } => {
                if lines.len() > native::MAX_NODES {
                    return Err(native::invalid("native plan line capacity"));
                }
                Ok(Box::new(StreamingPlanTailCell {
                    lines: lines
                        .iter()
                        .map(|line| graph.line(line))
                        .collect::<io::Result<_>>()?,
                    is_stream_continuation: continuation,
                }))
            }
            Body::ProposedStream {
                lines,
                continuation,
            } => {
                if lines.len() > native::MAX_NODES {
                    return Err(native::invalid("native plan line capacity"));
                }
                Ok(Box::new(ProposedPlanStreamCell {
                    lines: lines
                        .iter()
                        .map(|line| graph.line(line))
                        .collect::<io::Result<_>>()?,
                    is_stream_continuation: continuation,
                }))
            }
            Body::Update {
                activity_id,
                explanation,
                plan,
            } => {
                if plan.len() > native::MAX_NODES {
                    return Err(native::invalid("native plan item capacity"));
                }
                Ok(Box::new(PlanUpdateCell {
                    activity_id,
                    explanation,
                    plan: plan
                        .into_iter()
                        .map(|item| PlanItemArg {
                            step: item.step,
                            status: match item.status {
                                StatusData::Pending {} => StepStatus::Pending,
                                StatusData::InProgress {} => StepStatus::InProgress,
                                StatusData::Completed {} => StepStatus::Completed,
                            },
                        })
                        .collect(),
                }))
            }
        }
    }
}

#[cfg(test)]
#[path = "retained_history_aux_tests.rs"]
mod tests;
