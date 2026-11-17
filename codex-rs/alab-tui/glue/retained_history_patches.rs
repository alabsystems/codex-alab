// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native patch source and failures. Raw paths remain original render DATA;
//! decoding does not open/apply patches, create config or manufacture approval.
use super::*;
use crate::history_cell::plans::alab_retained_plans::PathData;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Change {
    Add {
        content: String,
    },
    Delete {
        content: String,
    },
    Update {
        unified_diff: String,
        move_path: Option<PathData>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    path: PathData,
    change: Change,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Body {
    Patch {
        activity_id: String,
        cwd: PathData,
        changes: Vec<Entry>,
    },
    Failure {
        activity_id: String,
        stderr: String,
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
        _graph: &mut native::Capture,
        depth: usize,
    ) -> io::Result<Option<Self>> {
        native::depth(depth)?;
        let body = if let Some(actual) = actual.as_any().downcast_ref::<PatchHistoryCell>() {
            let mut changes = actual
                .changes
                .iter()
                .map(|(path, change)| Entry {
                    path: PathData::capture(path),
                    change: match change {
                        FileChange::Add { content } => Change::Add {
                            content: content.clone(),
                        },
                        FileChange::Delete { content } => Change::Delete {
                            content: content.clone(),
                        },
                        FileChange::Update {
                            unified_diff,
                            move_path,
                        } => Change::Update {
                            unified_diff: unified_diff.clone(),
                            move_path: move_path.as_ref().map(|path| PathData::capture(path)),
                        },
                    },
                })
                .collect::<Vec<_>>();
            // The native diff renderer sorts paths too. Preserve every actual map entry.
            changes.sort_by(|a, b| a.path.bytes().cmp(b.path.bytes()));
            Body::Patch {
                activity_id: actual.activity_id.clone(),
                cwd: PathData::capture(&actual.cwd),
                changes,
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<PatchFailureCell>() {
            Body::Failure {
                activity_id: actual.activity_id.clone(),
                stderr: actual.stderr.clone(),
            }
        } else {
            return Ok(None);
        };
        Ok(Some(Self { schema: 1, body }))
    }
    pub(in crate::history_cell) fn restore(
        self,
        _graph: &mut native::Restored,
        depth: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        native::depth(depth)?;
        if self.schema != 1 {
            return Err(native::invalid("native patch cell version"));
        }
        match self.body {
            Body::Failure {
                activity_id,
                stderr,
            } => Ok(Box::new(PatchFailureCell {
                activity_id,
                stderr,
            })),
            Body::Patch {
                activity_id,
                cwd,
                changes,
            } => {
                if changes.len() > native::MAX_NODES {
                    return Err(native::invalid("native patch change capacity"));
                }
                let mut restored = HashMap::new();
                for entry in changes {
                    let path = entry.path.restore()?;
                    let change = match entry.change {
                        Change::Add { content } => FileChange::Add { content },
                        Change::Delete { content } => FileChange::Delete { content },
                        Change::Update {
                            unified_diff,
                            move_path,
                        } => FileChange::Update {
                            unified_diff,
                            move_path: move_path.map(PathData::restore).transpose()?,
                        },
                    };
                    if restored.insert(path, change).is_some() {
                        return Err(native::invalid("duplicate native patch path"));
                    }
                }
                Ok(Box::new(PatchHistoryCell {
                    activity_id,
                    cwd: cwd.restore()?,
                    changes: restored,
                }))
            }
        }
    }
}
