// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ExecCommandToolOutput;
use super::ToolCallSource;
use super::TruncationPolicy;
use super::with_serialization_allowance;
use crate::unified_exec::shell_output::DisclosedOutput;
use crate::unified_exec::shell_output::LogNotice;
use uuid::Uuid;

impl ExecCommandToolOutput {
    pub(super) fn shell_invocation_id(&self) -> Option<Uuid> {
        self.process_id?;
        Some(self.raw_output.shell_log.as_ref()?.invocation_id())
    }

    pub(super) fn add_invocation_header(&self, sections: &mut Vec<String>) {
        if let Some(id) = self.shell_invocation_id() {
            sections.push(format!("Invocation ID: {id}"));
        }
    }

    pub(super) fn direct_shell_notice(&self, header_len: usize) -> Option<LogNotice> {
        let budget = with_serialization_allowance(self.truncation_policy).byte_budget();
        let mut notice = self.shell_output_notice(
            Some(self.model_output_policy()),
            budget.saturating_sub(header_len.saturating_add(1)),
        )?;
        if notice.tail.is_some() {
            let minimum_preview = self
                .truncated_output_with_policy(TruncationPolicy::Bytes(0))
                .len();
            if header_len
                .saturating_add(2)
                .saturating_add(notice.text().len())
                .saturating_add(minimum_preview)
                > budget
            {
                notice.withhold_tail();
            }
        }
        Some(notice)
    }

    pub(super) fn code_mode_shell_notice(&self) -> Option<LogNotice> {
        self.shell_output_notice(
            self.max_output_tokens.map(TruncationPolicy::Tokens),
            usize::MAX,
        )
    }

    pub(crate) fn frozen_shell_tail(&self) -> Option<&codex_alab_core::ShellOutputTail> {
        self.raw_output.shell_log.as_ref()?.frozen_tail()
    }

    pub(crate) fn undelivered_shell_tail(&self) -> Option<&codex_alab_core::ShellOutputTail> {
        self.raw_output.shell_log.as_ref()?.undelivered_tail()
    }

    pub(crate) fn prepare_shell_observation(
        &mut self,
        source: &ToolCallSource,
        terminal_failure: bool,
    ) -> Option<DisclosedOutput> {
        let notice = match source {
            ToolCallSource::CodeMode { .. } => self.code_mode_shell_notice(),
            _ => self.direct_shell_notice(self.response_header().len()),
        };
        let disclosed = notice.as_ref().and_then(|notice| notice.disclosed);
        if terminal_failure && let Some(log) = self.raw_output.shell_log.as_mut() {
            log.freeze_result_notice(notice);
        }
        disclosed
    }
}
