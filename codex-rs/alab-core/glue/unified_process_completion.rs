// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

/// Publish the terminal event from process observations. Operational failures retain the
/// measured duration and any real backend code, which can be zero even after a late denial.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn emit_exec_end_for_unified_exec(
    sandbox_type: Option<codex_protocol::sandbox::SandboxType>,
    session: Arc<Session>,
    turn: Arc<TurnContext>,
    model: Arc<CatalogModel>,
    call_id: String,
    command: Vec<String>,
    cwd: PathUri,
    process_id: Option<String>,
    plugin_attribution: Option<PluginCommandAttribution>,
    output_buffer: Arc<Mutex<OutputBuffers>>,
    fallback_output: String,
    exit_code: Option<i32>,
    duration: Duration,
    timed_out: bool,
    failure: Option<String>,
) {
    let failed = failure.is_some() || timed_out;
    // The initial failed attempt may have a more specific captured transcript.
    let mut output = if failure.is_some() && !fallback_output.is_empty() {
        fallback_output
    } else {
        resolve_aggregated_output(&output_buffer, fallback_output).await
    };
    if let Some(message) = failure {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&message);
    }
    let mut ctx = ToolEventCtx::new(&session, &turn, &model, &call_id, None);
    ctx.sandbox_type = sandbox_type;
    ExecCommandResult::measured(output, exit_code, duration, failed)
        .emit_for_process(
            ctx,
            &command,
            &cwd,
            process_id.as_deref(),
            plugin_attribution.as_ref(),
        )
        .await;
}
