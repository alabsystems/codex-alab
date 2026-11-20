// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A provider finding ends model work. An already-started command reports its own result.

use super::*;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::new_active_exec_command;
use codex_app_server_protocol::CommandExecutionStatus;
use ratatui::text::Line;
use std::sync::{Arc, Weak};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AlabPendingCommand {
    thread_id: ThreadId,
    turn_id: String,
    call_id: String,
    command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    process_id: Option<String>,
    #[serde(default)]
    finished_without_code: bool,
    // The native retained history graph owns the command and its positioned detail cells.
    #[serde(skip)]
    original: Option<Arc<ExecCell>>,
}

impl AlabPendingCommand {
    pub(crate) fn new(
        thread_id: ThreadId,
        turn_id: String,
        call_id: String,
        command: String,
    ) -> Self {
        Self {
            thread_id,
            turn_id,
            call_id,
            command,
            process_id: None,
            finished_without_code: false,
            original: None,
        }
    }

    pub(crate) fn with_original(
        mut self,
        original: Arc<ExecCell>,
        process_id: Option<String>,
    ) -> Self {
        self.original = Some(original);
        self.process_id = process_id;
        self
    }

    pub(crate) fn original(&self) -> Option<&Arc<ExecCell>> {
        self.original.as_ref()
    }

    pub(crate) fn process_id(&self) -> Option<&str> {
        self.process_id.as_deref()
    }

    fn status_text(&self) -> &'static str {
        if self.finished_without_code {
            "Command ended without a reported exit code"
        } else {
            "Command result pending"
        }
    }

    pub(crate) fn matches(&self, thread_id: ThreadId, turn_id: &str, call_id: &str) -> bool {
        !self.finished_without_code
            && self.thread_id == thread_id
            && self.turn_id == turn_id
            && self.call_id == call_id
    }
}

impl HistoryCell for AlabPendingCommand {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        history_cell::new_info_event(format!("{}: {}", self.status_text(), self.command), None)
            .display_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let Some(original) = self.original() else {
            return self.display_lines(u16::MAX);
        };
        let mut lines = original.raw_lines();
        lines.push(self.status_text().into());
        lines
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(original) = self.original() else {
            return self.display_lines(width);
        };
        let mut lines = original.transcript_lines(width);
        lines.push(self.status_text().into());
        lines
    }
}

#[derive(Debug, Eq, Hash, PartialEq)]
struct Owner {
    thread: ThreadId,
    turn: String,
}

struct Start {
    owner: Owner,
    process_id: Option<String>,
}
struct Observation {
    cell: Weak<ExecCell>,
    source: ExecCommandSource,
    process_id: Option<String>,
    reused_call_id: bool,
    reused_handle: bool,
}

enum ResultState {
    Pending(Observation),
    // The UI/event owns the result. Only visible and in-flight rows need a duplicate guard.
    Displayed(Weak<dyn HistoryCell>),
    Evicted,
}

#[derive(Default)]
pub(super) struct Commands {
    starts: HashMap<String, Start>,
    observations: HashMap<String, HashMap<Owner, ResultState>>,
    admissions_until_sweep: usize,
}

impl Commands {
    pub(super) fn start(&mut self, thread: &str, turn: &str, item: &ThreadItemPayload) -> bool {
        let ThreadItemPayload::CommandExecution {
            id,
            process_id,
            source,
            ..
        } = item
        else {
            return true;
        };
        let Ok(thread) = ThreadId::from_string(thread) else {
            return true;
        };
        let owner = Owner {
            thread,
            turn: turn.to_owned(),
        };
        if let Some(states) = self.observations.get_mut(id) {
            if states.contains_key(&owner) {
                return false;
            }
            if *source == ExecCommandSource::UnifiedExecStartup {
                // The backend can reuse both protocol and process IDs across turns.
                for state in states.values_mut() {
                    if let ResultState::Pending(old) = state {
                        old.reused_call_id = true;
                        old.reused_handle |= old.process_id.as_deref().unwrap_or(id)
                            == process_id.as_deref().unwrap_or(id);
                    }
                }
            }
        }
        self.starts.insert(
            id.clone(),
            Start {
                owner,
                process_id: process_id.clone(),
            },
        );
        true
    }

    fn sweep(&mut self, terminals: &[UnifiedExecProcessSummary], restoring_terminals: bool) {
        let mut count = 0usize;
        let live: HashSet<(&str, &str)> = terminals
            .iter()
            .map(|p| (p.call_id.as_str(), p.key.as_str()))
            .collect();
        self.observations.retain(|id, states| {
            states.retain(|_, state| match state {
                ResultState::Displayed(row) => row.strong_count() > 0,
                ResultState::Pending(pending) => {
                    pending.cell.strong_count() > 0
                        || (is_unified_exec_source(pending.source)
                            && !pending.reused_handle
                            && (restoring_terminals
                                || live.contains(&(
                                    id.as_str(),
                                    pending.process_id.as_deref().unwrap_or(id),
                                ))))
                }
                ResultState::Evicted => false,
            });
            count = count.saturating_add(states.len());
            !states.is_empty()
        });
        self.admissions_until_sweep = count.max(16);
    }

    fn observe(
        &mut self,
        row: &AlabPendingCommand,
        canonical_restore: bool,
        terminals: &[UnifiedExecProcessSummary],
        restoring_terminals: bool,
    ) -> bool {
        if row.finished_without_code {
            return false;
        }
        let Some(cell) = row.original() else {
            return false;
        };
        let [call] = cell.group.calls.as_slice() else {
            return false;
        };
        if call.call_id != row.call_id
            || !call.is_pending()
            || strip_bash_lc_and_escape(&call.command) != row.command
        {
            return false;
        }
        if self.admissions_until_sweep == 0 {
            self.sweep(terminals, restoring_terminals);
        } else {
            self.admissions_until_sweep -= 1;
        }
        let owner = Owner {
            thread: row.thread_id,
            turn: row.turn_id.clone(),
        };
        let states = self.observations.entry(row.call_id.clone()).or_default();
        use std::collections::hash_map::Entry;
        let entry = match states.entry(owner) {
            Entry::Vacant(entry) => entry,
            Entry::Occupied(mut entry) => {
                // Upgrade may install a new native graph over a surviving widget. Bind the
                // canonical Arc but preserve evidence of any newer process that reused its ID.
                if canonical_restore
                    && let ResultState::Pending(pending) = entry.get_mut()
                    && pending.process_id == row.process_id
                    && pending.source == call.source
                {
                    pending.cell = Arc::downgrade(cell);
                }
                return false;
            }
        };
        entry.insert(ResultState::Pending(Observation {
            cell: Arc::downgrade(cell),
            source: call.source,
            process_id: row.process_id.clone(),
            reused_call_id: false,
            reused_handle: false,
        }));
        true
    }

    pub(super) fn clear_starts(&mut self) {
        self.starts.clear();
    }
}

impl ChatWidget {
    pub(crate) fn alab_restore_finding_command(&mut self, pending: &AlabPendingCommand) {
        if self.thread_id == Some(pending.thread_id) {
            self.alab_finding_commands.observe(
                pending,
                true,
                &self.unified_exec_processes,
                self.pending_terminal_restore.is_some(),
            );
        }
    }

    pub(super) fn alab_replay_finding_command(
        &mut self,
        turn: &str,
        item: &ThreadItemPayload,
    ) -> bool {
        let ThreadItemPayload::CommandExecution {
            id,
            command,
            source,
            command_actions,
            process_id,
            status: CommandExecutionStatus::InProgress,
            aggregated_output,
            ..
        } = item
        else {
            return false;
        };
        let Some(thread) = self.thread_id else {
            return false;
        };
        let (argv, parsed) = command_execution_command_and_parsed(command, command_actions);
        if is_unified_exec_source(*source) && !is_standard_tool_call(&parsed) {
            return false;
        }
        let parsed = self.annotate_skill_reads_in_parsed_cmd(parsed);
        let display = strip_bash_lc_and_escape(&argv);
        let mut original = new_active_exec_command(id.clone(), argv, parsed, *source, None, false);
        if let Some(output) = aggregated_output {
            original.append_output(id, output);
        }
        let row = AlabPendingCommand::new(thread, turn.to_owned(), id.clone(), display)
            .with_original(Arc::new(original), process_id.clone());
        if self.alab_finding_commands.observe(
            &row,
            false,
            &self.unified_exec_processes,
            self.pending_terminal_restore.is_some(),
        ) {
            self.add_to_history(row);
        }
        true
    }

    /// Separate completed runs and individual unfinished calls without changing any completed
    /// call's exit, parsed command, output, timing, or attached transcript details.
    pub(super) fn alab_record_finding_commands(&mut self) -> bool {
        let Some(exec) = self
            .transcript
            .active_cell
            .as_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<ExecCell>())
            .filter(|exec| exec.is_active())
        else {
            return false;
        };
        let mut parts = Vec::new();
        while let Some(last) = exec.group.calls.last() {
            let end = exec.group.calls.len();
            let start = if last.is_pending() {
                end - 1
            } else {
                exec.group
                    .calls
                    .iter()
                    .rposition(|call| call.is_pending())
                    .map_or(0, |index| index + 1)
            };
            let mut calls = exec.group.calls.split_off(start).into_iter();
            let mut part = ExecCell::new(calls.next().expect("nonempty command segment"), false);
            part.group.calls.extend(calls);
            part.group.details = if start == 0 {
                std::mem::take(&mut exec.group.details)
            } else {
                exec.group.details.split_after(start)
            };
            parts.push(part);
        }
        let _ = self.transcript.take_active_cell();
        for part in parts.into_iter().rev() {
            if !part.is_active() {
                self.add_to_history(part);
                continue;
            }
            let call = &part.group.calls[0];
            let command = strip_bash_lc_and_escape(&call.command);
            if let Some(Start { owner, process_id }) =
                self.alab_finding_commands.starts.remove(&call.call_id)
                && Some(owner.thread) == self.thread_id
            {
                let row = AlabPendingCommand::new(
                    owner.thread,
                    owner.turn.clone(),
                    call.call_id.clone(),
                    command,
                )
                .with_original(Arc::new(part), process_id);
                if self.alab_finding_commands.observe(
                    &row,
                    false,
                    &self.unified_exec_processes,
                    self.pending_terminal_restore.is_some(),
                ) {
                    self.add_to_history(row);
                }
            } else {
                // A historical begin without a live owner cannot authenticate a later result.
                self.add_to_history(history_cell::new_info_event(
                    format!("Command result unavailable: {command}"),
                    None,
                ));
            }
        }
        true
    }

    pub(super) fn alab_complete_finding_command(
        &mut self,
        thread: &str,
        turn: &str,
        item: &ThreadItemPayload,
    ) -> bool {
        let ThreadItemPayload::CommandExecution {
            id,
            source,
            status,
            process_id,
            aggregated_output,
            exit_code,
            duration_ms,
            ..
        } = item
        else {
            return false;
        };
        let Ok(thread) = ThreadId::from_string(thread) else {
            return false;
        };
        let commands = &mut self.alab_finding_commands;
        let start = commands.starts.get(id);
        let has_start = start.is_some();
        let current_start =
            start.is_some_and(|start| start.owner.thread == thread && start.owner.turn == turn);
        if current_start {
            commands.starts.remove(id);
        }
        let Some(observations) = commands.observations.get_mut(id) else {
            // Even after an evicted observation has been collected, a live command
            // cannot receive an outcome sent for a different turn under the same ID.
            return has_start && !current_start;
        };
        let owner = Owner {
            thread,
            turn: turn.to_owned(),
        };
        let Some(slot) = observations.get_mut(&owner) else {
            // A colliding ID on newly observed work has its own normal route. An unpaired
            // event from another turn cannot remove the retained command's process or row.
            return !current_start;
        };
        if *status == CommandExecutionStatus::InProgress {
            return true;
        }
        let ResultState::Pending(observed) = slot else {
            return true;
        };
        if observed.source != *source
            || matches!((&observed.process_id, process_id), (Some(expected), Some(actual)) if expected != actual)
        {
            return true;
        }
        let terminal = is_unified_exec_source(*source).then(|| {
            (
                observed
                    .process_id
                    .as_deref()
                    .or(process_id.as_deref())
                    .unwrap_or(id)
                    .to_owned(),
                observed.reused_call_id,
                observed.reused_handle,
            )
        });
        // Dropping the last retained or queued row immediately releases its bounded
        // output and details. Only an actual live terminal may still need reconciliation.
        let result = observed
            .cell
            .upgrade()
            .map(|original| -> Arc<dyn HistoryCell> {
                let call = &original.group.calls[0];
                let mut part = new_active_exec_command(
                    call.call_id.clone(),
                    call.command.clone(),
                    call.parsed.clone(),
                    call.source,
                    call.interaction_input.clone(),
                    false,
                );
                part.group.details = original.group.details.clone();
                // The final aggregate, even an empty one, supersedes the preview. Some native
                // completions report an exit code alone. Preserve the bounded native output then.
                let mut output = if *source == ExecCommandSource::UnifiedExecInteraction {
                    CommandOutput::default()
                } else if let Some(text) = aggregated_output {
                    CommandOutput::new(0, text.clone())
                } else {
                    call.output.clone().unwrap_or_default()
                };
                output.set_result(status.clone(), *exit_code);
                let duration = duration_ms
                    .map(|millis| std::time::Duration::from_millis(millis.max(0) as u64));
                let call = &mut part.group.calls[0];
                call.start_time = None;
                call.duration = duration;
                call.output = Some(output);
                Arc::new(part)
            });
        *slot = result.as_ref().map_or(ResultState::Evicted, |cell| {
            ResultState::Displayed(Arc::downgrade(cell))
        });
        if let Some((key, reused_call, reused_handle)) = terminal {
            let another_owner = self
                .unified_exec_processes
                .iter()
                .any(|process| process.key == key && process.call_id != *id);
            if !another_owner && !reused_handle {
                if reused_call {
                    // A terminal restore tracks exited item IDs without a turn. Retire only this
                    // exact old process when a newer turn has already reused the item ID.
                    self.unified_exec_processes
                        .retain(|process| !(process.key == key && process.call_id == *id));
                    self.sync_unified_exec_footer();
                } else {
                    self.track_unified_exec_process_end(id, Some(&key));
                }
            }
        }
        if let Some(cell) = result {
            self.app_event_tx
                .send(AppEvent::AlabFindingCommandFinished {
                    thread_id: thread,
                    turn_id: turn.to_owned(),
                    call_id: id.clone(),
                    cell,
                    #[cfg(unix)]
                    alab_source: self.alab_current_history_source(),
                });
        }
        true
    }
}

#[cfg(test)]
#[path = "finding_command_lifetime_tests.rs"]
mod lifetime_tests;
