// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native hook presentation phases and deadlines; DATA does not invoke or complete a hook.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    runs: Vec<Run>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Run {
    id: String,
    status_message: Option<String>,
    state: State,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum State {
    PendingReveal {
        reveal_deadline: Snapshot,
    },
    VisibleRunning {
        visible_since: Snapshot,
    },
    QuietLinger {
        removal_deadline: Snapshot,
    },
    Completed {
        status: HookRunStatus,
        entries: Vec<HookOutputEntry>,
    },
}
impl Data {
    fn capture_hook(actual: &HookCell) -> io::Result<Self> {
        if actual.runs.len() > native::MAX_NODES {
            return Err(native::invalid("native hook run capacity"));
        }
        let runs = actual
            .runs
            .iter()
            .map(|run| {
                Ok(Run {
                    id: run.id.clone(),
                    status_message: run.status_message.clone(),
                    state: match &run.state {
                        HookRunState::PendingReveal { reveal_deadline } => State::PendingReveal {
                            reveal_deadline: Snapshot::capture(*reveal_deadline)
                                .map_err(native::invalid)?,
                        },
                        HookRunState::VisibleRunning { visible_since } => State::VisibleRunning {
                            visible_since: Snapshot::capture(*visible_since)
                                .map_err(native::invalid)?,
                        },
                        HookRunState::QuietLinger { removal_deadline } => State::QuietLinger {
                            removal_deadline: Snapshot::capture(*removal_deadline)
                                .map_err(native::invalid)?,
                        },
                        HookRunState::Completed { status, entries } => State::Completed {
                            status: *status,
                            entries: entries.clone(),
                        },
                    },
                })
            })
            .collect::<io::Result<_>>()?;
        Ok(Self { runs })
    }
    pub(in crate::history_cell) fn capture(actual: &dyn HistoryCell) -> io::Result<Option<Self>> {
        actual
            .as_any()
            .downcast_ref::<HookCell>()
            .map(Self::capture_hook)
            .transpose()
    }
    fn restore_hook(self) -> io::Result<HookCell> {
        if self.runs.len() > native::MAX_NODES {
            return Err(native::invalid("native hook run capacity"));
        }
        let runs = self
            .runs
            .into_iter()
            .map(|run| {
                Ok(HookRunCell {
                    id: run.id,
                    status_message: run.status_message,
                    state: match run.state {
                        State::PendingReveal { reveal_deadline } => HookRunState::PendingReveal {
                            reveal_deadline: reveal_deadline.restore().map_err(native::invalid)?,
                        },
                        State::VisibleRunning { visible_since } => HookRunState::VisibleRunning {
                            visible_since: visible_since.restore().map_err(native::invalid)?,
                        },
                        State::QuietLinger { removal_deadline } => HookRunState::QuietLinger {
                            removal_deadline: removal_deadline
                                .restore()
                                .map_err(native::invalid)?,
                        },
                        State::Completed { status, entries } => {
                            HookRunState::Completed { status, entries }
                        }
                    },
                })
            })
            .collect::<io::Result<_>>()?;
        Ok(HookCell { runs })
    }
    pub(in crate::history_cell) fn restore(self) -> io::Result<Box<dyn HistoryCell>> {
        self.restore_hook()
            .map(|actual| Box::new(actual) as Box<dyn HistoryCell>)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::PathBufExt;
    use crate::test_support::test_path_buf;
    fn run(id: &str) -> HookRunSummary {
        HookRunSummary {
            id: id.into(),
            event_name: codex_app_server_protocol::HookEventName::PostToolUse,
            handler_type: codex_app_server_protocol::HookHandlerType::Command,
            execution_mode: codex_app_server_protocol::HookExecutionMode::Sync,
            scope: codex_app_server_protocol::HookScope::Turn,
            source_path: test_path_buf("/actual/hooks.json").abs().into(),
            source: codex_app_server_protocol::HookSource::User,
            display_order: 0,
            status: HookRunStatus::Running,
            status_message: Some("actual hook status".into()),
            started_at: 1,
            completed_at: None,
            duration_ms: None,
            entries: vec![],
        }
    }
    fn saved(actual: &HookCell) -> io::Result<HookCell> {
        let data = Data::capture_hook(actual)?;
        serde_json::from_slice::<Data>(&serde_json::to_vec(&data).map_err(native::invalid)?)
            .map_err(native::invalid)?
            .restore_hook()
    }
    fn close_clock(a: Instant, b: Instant) -> bool {
        (if a >= b {
            a.duration_since(b)
        } else {
            b.duration_since(a)
        }) <= Duration::from_millis(1)
    }
    #[test]
    fn real_hook_reveal_visible_quiet_linger_and_removal_keep_original_deadlines() -> io::Result<()>
    {
        let mut original = new_active_hook_cell(run("actual"));
        let mut restored = saved(&original)?;
        let reveal = original
            .next_timer_deadline()
            .ok_or_else(|| native::invalid("actual pending hook timer missing"))?;
        assert!(close_clock(
            reveal,
            restored
                .next_timer_deadline()
                .ok_or_else(|| native::invalid("restored pending timer missing"))?
        ));
        let visible = reveal + Duration::from_millis(5);
        original.advance_time(visible);
        restored.advance_time(visible);
        assert_eq!(
            original.running_status_summary(),
            restored.running_status_summary()
        );
        restored = saved(&original)?;
        let mut completed = run("actual");
        completed.status = HookRunStatus::Completed;
        assert!(original.complete_run(completed.clone()));
        assert!(restored.complete_run(completed));
        let removal = original
            .next_timer_deadline()
            .ok_or_else(|| native::invalid("actual quiet linger missing"))?;
        assert!(close_clock(
            removal,
            restored
                .next_timer_deadline()
                .ok_or_else(|| native::invalid("restored quiet linger missing"))?
        ));
        restored = saved(&original)?;
        original.advance_time(removal + Duration::from_millis(5));
        restored.advance_time(removal + Duration::from_millis(5));
        assert!(original.is_empty() && restored.is_empty());
        Ok(())
    }
    #[test]
    fn real_hook_completed_output_context_privacy_and_native_type_survive_resize() -> io::Result<()>
    {
        let mut completed = run("actual complete");
        completed.status = HookRunStatus::Stopped;
        completed.entries = vec![
            HookOutputEntry {
                kind: HookOutputEntryKind::Context,
                text: "actual model-only context".into(),
            },
            HookOutputEntry {
                kind: HookOutputEntryKind::Warning,
                text: "exact visible text\nsecond line".into(),
            },
        ];
        let original = new_completed_hook_cell(completed);
        let restored = saved(&original)?;
        assert_eq!(restored.runs[0].id, original.runs[0].id);
        assert_eq!(
            restored.runs[0].status_message,
            original.runs[0].status_message
        );
        for width in [0, 5, 24, 97] {
            assert_eq!(original.display_lines(width), restored.display_lines(width));
            assert_eq!(
                original.transcript_lines(width),
                restored.transcript_lines(width)
            );
        }
        assert_eq!(original.raw_lines(), restored.raw_lines());
        assert!(
            !restored
                .raw_lines()
                .iter()
                .any(|line| line.to_string().contains("actual model-only context"))
        );
        assert!((&restored as &dyn HistoryCell).as_any().is::<HookCell>());
        Ok(())
    }
}
