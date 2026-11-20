// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde::Deserialize;
use serde::Serialize;

pub(super) fn retry_message(error: codex_app_server_protocol::TurnError) -> String {
    match error.additional_details {
        Some(details) if !details.is_empty() => format!("{} ({details})", error.message),
        _ => error.message,
    }
}

pub(super) fn failed_command_line(exit_code: Option<i32>, duration_suffix: &str) -> String {
    match exit_code {
        Some(0) => format!(" failed (exit 0){duration_suffix}:"),
        Some(code) => format!(" exited {code}{duration_suffix}:"),
        None => format!(" failed{duration_suffix}:"),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    kind: String,
    version: u8,
    final_message: Option<String>,
    final_message_rendered: bool,
    emit_final_message_on_shutdown: bool,
    last_total_token_usage: Option<ThreadTokenUsage>,
    last_turn_diff: String,
}

pub(super) fn snapshot(
    renderer: &EventProcessorWithHumanOutput,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(Snapshot {
        kind: "human".into(),
        version: 1,
        final_message: renderer.final_message.clone(),
        final_message_rendered: renderer.final_message_rendered,
        emit_final_message_on_shutdown: renderer.emit_final_message_on_shutdown,
        last_total_token_usage: renderer.last_total_token_usage.clone(),
        last_turn_diff: renderer.last_turn_diff.clone(),
    })
    .map_err(|_| "human renderer checkpoint could not be encoded".into())
}

pub(super) fn restore(
    renderer: &mut EventProcessorWithHumanOutput,
    value: serde_json::Value,
) -> Result<(), String> {
    let state: Snapshot = serde_json::from_value(value)
        .map_err(|_| "invalid human renderer checkpoint".to_owned())?;
    if state.kind != "human" || state.version != 1 {
        return Err("inconsistent human renderer checkpoint".into());
    }
    renderer.final_message = state.final_message;
    renderer.final_message_rendered = state.final_message_rendered;
    renderer.emit_final_message_on_shutdown = state.emit_final_message_on_shutdown;
    renderer.last_total_token_usage = state.last_total_token_usage;
    renderer.last_turn_diff = state.last_turn_diff;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::ErrorNotification;
    use codex_app_server_protocol::TurnError;
    use codex_core::config::ConfigBuilder;

    #[tokio::test]
    async fn stderr_distinguishes_scheduled_retries_from_terminal_errors() -> anyhow::Result<()> {
        let test = concat!(
            module_path!(),
            "::stderr_distinguishes_scheduled_retries_from_terminal_errors"
        );
        if !core_test_support::alab_test_process::child_with_stderr(test, |stderr| {
            assert_eq!(
                stderr.lines().collect::<Vec<_>>(),
                [
                    "ERROR: capacity unavailable",
                    "warning: capacity unavailable (Retry scheduled in 324s.)",
                    "ERROR: capacity unavailable",
                ]
            );
        }) {
            return Ok(());
        }
        let home = tempfile::tempdir()?;
        let config = ConfigBuilder::default()
            .codex_home(home.path().to_owned())
            .fallback_cwd(Some(home.path().to_owned()))
            .build()
            .await?;
        let mut renderer = EventProcessorWithHumanOutput::create_with_ansi(false, &config, None);
        let mut render = |will_retry| {
            let status = renderer.process_server_notification(ServerNotification::Error(
                ErrorNotification {
                    thread_id: "thread".into(),
                    turn_id: "turn".into(),
                    will_retry,
                    error: TurnError {
                        message: "capacity unavailable".into(),
                        additional_details: Some("Retry scheduled in 324s.".into()),
                        codex_error_info: None,
                        misalignment: None,
                    },
                },
            ));
            assert_eq!(status, CodexStatus::Running);
        };
        render(true);
        codex_alab_core::activate();
        render(true);
        render(false);
        Ok(())
    }
}
