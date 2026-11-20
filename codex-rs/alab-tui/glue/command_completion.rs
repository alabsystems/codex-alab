// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// Included by exec_cell::model so status, streamed output and call state share one representation.
use ratatui::style::Stylize as _;
use ratatui::text::Line as OutcomeLine;
use ratatui::text::Span as OutcomeSpan;

impl CommandOutput {
    pub(crate) fn from_execution(
        status: CommandExecutionStatus,
        exit_code: Option<i32>,
        source: ExecCommandSource,
        output: Option<String>,
    ) -> Self {
        let output = if source == ExecCommandSource::UnifiedExecInteraction {
            String::new()
        } else {
            output.unwrap_or_default()
        };
        let mut result = Self {
            aggregated_output: output,
            ..Self::default()
        };
        result.set_result(status, exit_code);
        result
    }

    pub(crate) fn set_result(&mut self, status: CommandExecutionStatus, code: Option<i32>) {
        // Older command-rejection events supplied -1 even though no process had run.
        let code = if status == CommandExecutionStatus::Declined {
            None
        } else {
            code
        };
        self.exit_code = code;
        self.reported_status = Some(status);
    }

    pub(crate) fn succeeded(&self) -> bool {
        self.reported_status == Some(CommandExecutionStatus::Completed)
            || (self.reported_status.is_none() && self.exit_code == Some(0))
    }

    pub(super) fn failed(&self) -> bool {
        self.reported_status == Some(CommandExecutionStatus::Failed)
            || self.nonzero_exit().is_some()
    }

    pub(super) fn nonzero_exit(&self) -> Option<i32> {
        self.exit_code.filter(|n| {
            *n != 0
                && matches!(
                    self.reported_status,
                    None | Some(CommandExecutionStatus::Failed)
                )
        })
    }

    fn has_reported_terminal(&self) -> bool {
        matches!(
            self.reported_status,
            Some(
                CommandExecutionStatus::Completed
                    | CommandExecutionStatus::Failed
                    | CommandExecutionStatus::Declined
            )
        )
    }

    pub(super) fn status_label(&self) -> Option<Cow<'static, str>> {
        match (&self.reported_status, self.exit_code) {
            (Some(CommandExecutionStatus::Completed), Some(0)) => return None,
            (Some(CommandExecutionStatus::Failed), Some(code)) if code != 0 => return None,
            _ => {}
        }
        let label = match self.reported_status.as_ref()? {
            CommandExecutionStatus::InProgress => return None,
            CommandExecutionStatus::Completed => "Completed",
            CommandExecutionStatus::Failed => "Failed",
            CommandExecutionStatus::Declined => "Declined",
        };
        Some(match self.exit_code {
            Some(code) => Cow::Owned(format!("{label} (exit {code})")),
            None => Cow::Borrowed(label),
        })
    }

    pub(super) fn transcript_outcome(&self) -> Option<OutcomeLine<'static>> {
        if let Some(label) = self.status_label() {
            let label = label.into_owned();
            let span = if self.failed() {
                label.red()
            } else if self.succeeded() {
                label.green()
            } else {
                label.dim()
            };
            Some(OutcomeLine::from(span))
        } else if let Some(code) = self.nonzero_exit() {
            Some(OutcomeLine::from(vec![
                "✗".red().bold(),
                format!(" ({code})").into(),
            ]))
        } else {
            self.succeeded()
                .then(|| OutcomeLine::from("✓".green().bold()))
        }
    }
}

impl ExecCall {
    pub(super) fn annotated_result(&self) -> Option<&CommandOutput> {
        let output = self.output.as_ref()?;
        (output.nonzero_exit().is_some() || output.status_label().is_some()).then_some(output)
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.duration.is_none()
            && !self.output.as_ref().is_some_and(|output| {
                output.has_reported_terminal() || output.nonzero_exit().is_some()
            })
    }

    pub(super) fn append_outcome_label(&self, label: &mut String) {
        if let Some(output) = self.annotated_result() {
            if let Some(status) = output.status_label() {
                label.push_str(&format!(" ({status})"));
            } else if let Some(code) = output.nonzero_exit() {
                label.push_str(&format!(" (exit {code})"));
            }
        }
    }

    pub(super) fn exploration_outcome(&self) -> Option<OutcomeSpan<'static>> {
        let output = self.annotated_result()?;
        if let Some(label) = output.status_label() {
            let suffix = format!(" ({label})");
            return Some(if output.failed() {
                suffix.red()
            } else {
                suffix.dim()
            });
        }
        let code = output.nonzero_exit()?;
        let suffix = if self.parsed.len() > 1 {
            format!(" (command exit {code})")
        } else {
            format!(" (exit {code})")
        };
        // A search with exit 1 can mean that no lines matched.
        let no_match = code == 1
            && self
                .parsed
                .iter()
                .any(|p| matches!(p, ParsedCommand::Search { .. }));
        Some(if no_match { suffix.dim() } else { suffix.red() })
    }

    pub(super) fn compact_result(
        &self,
        active: bool,
        running: impl FnOnce() -> OutcomeSpan<'static>,
    ) -> (OutcomeSpan<'static>, String) {
        if let Some(output) = self.annotated_result() {
            if let Some(label) = output.status_label() {
                let marker = if output.failed() {
                    "•".red().bold()
                } else if output.succeeded() {
                    "•".green().bold()
                } else {
                    "•".dim()
                };
                return (marker, label.into_owned());
            }
            if let Some(code) = output.nonzero_exit() {
                return ("•".bold(), format!("Exited (status {code})"));
            }
        }
        if active {
            (running(), "Running".to_owned())
        } else {
            let title = if self.is_unified_exec_interaction() {
                "Background output"
            } else {
                "Ran"
            };
            ("•".green().bold(), title.to_owned())
        }
    }

    pub(super) fn command_header(
        &self,
        active: bool,
        running: impl FnOnce() -> OutcomeSpan<'static>,
    ) -> OutcomeLine<'static> {
        let finished = self.output.as_ref().filter(|_| !self.is_pending());
        let bullet = match finished {
            Some(output) if output.succeeded() => "•".green().bold(),
            Some(output) if output.failed() => "•".red().bold(),
            Some(_) => "•".dim(),
            None => running(),
        };
        let label = finished.and_then(CommandOutput::status_label);
        if self.is_unified_exec_interaction() && label.is_none() {
            OutcomeLine::from(vec![bullet, " ".into()])
        } else {
            let title = label.unwrap_or(Cow::Borrowed(if active {
                "Running"
            } else if self.is_user_shell_command() {
                "You ran"
            } else {
                "Ran"
            }));
            OutcomeLine::from(vec![
                bullet,
                " ".into(),
                title.into_owned().bold(),
                " ".into(),
            ])
        }
    }
}
