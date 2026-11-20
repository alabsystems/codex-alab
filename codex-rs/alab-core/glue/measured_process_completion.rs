// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

impl ExecCommandResult {
    pub(crate) fn measured(
        aggregated_output: String,
        exit_code: Option<i32>,
        duration: Duration,
        failed: bool,
    ) -> Self {
        Self {
            aggregated_output,
            exit_code,
            duration: Some(duration),
            status: if !failed && exit_code == Some(0) {
                ExecCommandStatus::Completed
            } else {
                ExecCommandStatus::Failed
            },
        }
    }

    pub(crate) async fn emit_for_process(
        self,
        ctx: ToolEventCtx<'_>,
        command: &[String],
        cwd: &PathUri,
        process_id: Option<&str>,
        plugin_attribution: Option<&PluginCommandAttribution>,
    ) {
        let parsed = parse_command(command);
        let input = ExecCommandInput::new(
            command,
            cwd,
            &parsed,
            ExecCommandSource::UnifiedExecStartup,
            None,
            process_id,
            plugin_attribution,
        );
        emit_exec_end(ctx, input, self).await;
    }
}
