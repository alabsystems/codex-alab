//! Data model for grouped exec-call history cells in the TUI transcript.
//!
//! An `ExecCell` can represent either a single command or an "exploring" group of related read/
//! list/search commands. The chat widget relies on stable `call_id` matching to route progress and
//! end events into the right cell, and it treats "call id not found" as a real signal (for
//! example, an orphan end that should render as a separate history entry).
//! Transcript-only reasoning stays inside exploration groups so it does not split their
//! compact display, while the expanded transcript retains its position between commands.

use std::borrow::Cow;
use std::time::Duration;
use std::time::Instant;

use super::live_output::LiveCommandOutput;
use crate::history_cell::ActivityGroup;
use codex_app_server_protocol::CommandExecutionSource as ExecCommandSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_protocol::parse_command::ParsedCommand;
use itertools::Either;

#[derive(Clone, Debug, Default)] // ALAB(misalignment-review): preserve bounded observed output when the exit omits its aggregate.
pub(crate) struct CommandOutput {
    pub(crate) exit_code: Option<i32>,
    // ALAB(shell-output): keep a terminal status independently of an observed exit.
    reported_status: Option<CommandExecutionStatus>,
    /// The finalized, interleaved stderr and stdout that replaces any streamed preview.
    aggregated_output: String,
    /// The live preview while command-output deltas are still arriving.
    live_output: Option<LiveCommandOutput>,
}

impl CommandOutput {
    pub(crate) fn new(exit_code: i32, aggregated_output: String) -> Self {
        Self {
            exit_code: Some(exit_code),
            reported_status: None,
            aggregated_output,
            live_output: None,
        }
    }

    /// Returns the total number of logical lines and the number retained for rendering.
    pub(super) fn line_counts(&self) -> (usize, usize) {
        match self.live_output.as_ref() {
            Some(output) => (output.total_lines(), output.retained_lines()),
            None => {
                let total = self.aggregated_output.lines().count();
                (total, total)
            }
        }
    }

    /// Returns retained preview lines with reverse traversal for efficient tail rendering.
    pub(super) fn lines(&self) -> impl DoubleEndedIterator<Item = Cow<'_, str>> {
        match self.live_output.as_ref() {
            Some(output) => Either::Left(output.lines()),
            None => Either::Right(self.aggregated_output.lines().map(Cow::Borrowed)),
        }
    }

    /// Returns lines for the expanded transcript, including any storage-level omission marker.
    pub(super) fn transcript_lines(&self) -> impl Iterator<Item = Cow<'_, str>> {
        match self.live_output.as_ref() {
            Some(output) => Either::Left(output.transcript_lines()),
            None => Either::Right(self.aggregated_output.lines().map(Cow::Borrowed)),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ExecCall {
    pub(crate) call_id: String,
    pub(crate) command: Vec<String>,
    pub(crate) parsed: Vec<ParsedCommand>,
    pub(crate) output: Option<CommandOutput>,
    pub(crate) source: ExecCommandSource,
    pub(crate) start_time: Option<Instant>,
    pub(crate) duration: Option<Duration>,
    pub(crate) interaction_input: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ExecCell {
    pub(crate) group: ActivityGroup<ExecCall>,
    animations_enabled: bool,
}

impl ExecCell {
    pub(crate) fn new(call: ExecCall, animations_enabled: bool) -> Self {
        Self {
            group: ActivityGroup::new(vec![call]),
            animations_enabled,
        }
    }

    pub(crate) fn add_call(
        &mut self,
        call_id: String,
        command: Vec<String>,
        parsed: Vec<ParsedCommand>,
        source: ExecCommandSource,
        interaction_input: Option<String>,
    ) -> bool {
        let call = ExecCall {
            call_id,
            command,
            parsed,
            output: None,
            source,
            start_time: Some(Instant::now()),
            duration: None,
            interaction_input,
        };
        if self.is_exploring_cell() && Self::is_exploring_call(&call) {
            self.group.calls.push(call);
            true
        } else {
            false
        }
    }

    /// Historical grouping uses the same compatibility and failure boundary as live calls.
    pub(crate) fn append_completed(&mut self, mut newer: Self) -> Result<(), Self> {
        if !self.is_exploring_cell() || self.should_flush() || !newer.is_exploring_cell() {
            return Err(newer);
        }
        newer.group.details.prepend(
            std::mem::take(&mut self.group.details),
            self.group.calls.len(),
        );
        self.group.details = newer.group.details;
        self.group.calls.append(&mut newer.group.calls);
        Ok(())
    }

    /// Preserve live clocks and pending output when a validated older page extends exploration.
    pub(crate) fn prepend(&mut self, older: Self) {
        self.group.prepend(older.group);
    }

    /// Marks the most recently matching call as finished and returns whether a call was found.
    ///
    /// Callers should treat `false` as a routing mismatch rather than silently ignoring it. The
    /// chat widget uses that signal to avoid attaching an orphan `exec_end` event to an unrelated
    /// active exploring cell, which would incorrectly collapse two transcript entries together.
    pub(crate) fn complete_call(
        &mut self,
        call_id: &str,
        output: CommandOutput,
        duration: impl Into<Option<Duration>>,
    ) -> bool {
        let Some(call) = self
            .group
            .calls
            .iter_mut()
            .rev()
            .find(|c| c.call_id == call_id)
        else {
            return false;
        };
        call.output = Some(output);
        call.duration = duration.into(); // ALAB(shell-output): an unreported duration is distinct from zero.
        call.start_time = None;
        true
    }

    pub(crate) fn should_flush(&self) -> bool {
        // Exploration stays open for adjacent calls, including after a failed read/list/search.
        !self.is_exploring_cell() && self.group.calls.iter().all(|c| !c.is_pending())
    }

    pub(crate) fn mark_failed(&mut self) {
        for call in self.group.calls.iter_mut() {
            if call.is_pending() {
                let elapsed = call.start_time.map(|st| st.elapsed());
                call.start_time = None;
                call.duration = elapsed;
                call.output
                    .get_or_insert_with(CommandOutput::default)
                    .set_result(CommandExecutionStatus::Failed, None);
            }
        }
    }

    pub(crate) fn is_exploring_cell(&self) -> bool {
        self.group.calls.iter().all(Self::is_exploring_call)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.group.calls.iter().any(ExecCall::is_pending)
    }

    pub(crate) fn active_start_time(&self) -> Option<Instant> {
        self.group
            .calls
            .iter()
            .find(|c| c.is_pending())
            .and_then(|c| c.start_time)
    }

    pub(crate) fn animations_enabled(&self) -> bool {
        self.animations_enabled
    }

    pub(crate) fn freeze_snapshot(&mut self) {
        self.animations_enabled = false;
    }

    pub(crate) fn iter_calls(&self) -> impl Iterator<Item = &ExecCall> {
        self.group.calls.iter()
    }

    pub(crate) fn append_output(&mut self, call_id: &str, chunk: &str) -> bool {
        if chunk.is_empty() {
            return false;
        }
        let Some(call) = self
            .group
            .calls
            .iter_mut()
            .rev()
            .find(|c| c.call_id == call_id)
        else {
            return false;
        };
        let output = call.output.get_or_insert_with(CommandOutput::default);
        output
            .live_output
            .get_or_insert_with(LiveCommandOutput::default)
            .push_str(chunk);
        true
    }

    pub(super) fn is_exploring_call(call: &ExecCall) -> bool {
        !matches!(call.source, ExecCommandSource::UserShell)
            && !call.parsed.is_empty()
            && call.parsed.iter().all(|p| {
                matches!(
                    p,
                    ParsedCommand::Read { .. }
                        | ParsedCommand::ListFiles { .. }
                        | ParsedCommand::Search { .. }
                )
            })
    }
}

impl ExecCall {
    pub(crate) fn is_user_shell_command(&self) -> bool {
        matches!(self.source, ExecCommandSource::UserShell)
    }

    pub(crate) fn is_unified_exec_interaction(&self) -> bool {
        matches!(self.source, ExecCommandSource::UnifiedExecInteraction)
    }
}

#[cfg(unix)]
#[path = "../../../alab-tui/glue/retained_history_command_output.rs"]
pub(super) mod alab_retained_output; // ALAB(retained-ui): preserve exact native activity state and shared graph identity.

// ALAB(shell-output): keep observed command outcomes in live and restored cells.
include!("../../../alab-tui/glue/command_completion.rs");
