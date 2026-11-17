// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native transcript-only terminal interaction and original process summaries are UI DATA.
use super::*;
use serde::Deserialize;
use serde::Serialize;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) enum Data {
    Interaction {
        command_display: Option<String>,
        stdin: String,
    },
    Processes {
        processes: Vec<Process>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Process {
    command_display: String,
    recent_chunks: Vec<String>,
}
impl Data {
    pub(in crate::history_cell) fn capture(actual: &dyn HistoryCell) -> Option<Self> {
        if let Some(actual) = actual.as_any().downcast_ref::<UnifiedExecInteractionCell>() {
            return Some(Self::Interaction {
                command_display: actual.command_display.clone(),
                stdin: actual.stdin.clone(),
            });
        }
        actual
            .as_any()
            .downcast_ref::<UnifiedExecProcessesCell>()
            .map(|actual| Self::Processes {
                processes: actual
                    .processes
                    .iter()
                    .map(|actual| Process {
                        command_display: actual.command_display.clone(),
                        recent_chunks: actual.recent_chunks.clone(),
                    })
                    .collect(),
            })
    }
    pub(in crate::history_cell) fn restore(self) -> Box<dyn HistoryCell> {
        match self {
            Self::Interaction {
                command_display,
                stdin,
            } => Box::new(UnifiedExecInteractionCell {
                command_display,
                stdin,
            }),
            Self::Processes { processes } => Box::new(UnifiedExecProcessesCell {
                processes: processes
                    .into_iter()
                    .map(|actual| UnifiedExecProcessDetails {
                        command_display: actual.command_display,
                        recent_chunks: actual.recent_chunks,
                    })
                    .collect(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_original_process_summary_retains_every_command_and_chunk_as_native_type()
    -> std::io::Result<()> {
        let original = UnifiedExecProcessesCell::new(vec![UnifiedExecProcessDetails {
            command_display: "actual command\nwith args".into(),
            recent_chunks: vec!["old\nchunk".into(), "new chunk".into()],
        }]);
        let data = Data::capture(&original).ok_or_else(|| {
            crate::terminal_hyperlinks::alab_retained_text::invalid(
                "actual process variant missing",
            )
        })?;
        let data: Data = serde_json::from_slice(
            &serde_json::to_vec(&data)
                .map_err(crate::terminal_hyperlinks::alab_retained_text::invalid)?,
        )
        .map_err(crate::terminal_hyperlinks::alab_retained_text::invalid)?;
        let restored = data.restore();
        assert!(restored.as_any().is::<UnifiedExecProcessesCell>());
        for width in [0, 4, 24, 100] {
            assert_eq!(original.display_lines(width), restored.display_lines(width));
            assert_eq!(
                original.transcript_lines(width),
                restored.transcript_lines(width)
            );
        }
        assert_eq!(original.raw_lines(), restored.raw_lines());
        Ok(())
    }
}
