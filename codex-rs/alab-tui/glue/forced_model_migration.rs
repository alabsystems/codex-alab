// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(execution) glue, compiled inside `tui::app::startup_prompts`.

use super::*;

pub(super) fn apply(
    config: &mut Config,
    launch_choices: &mut crate::app_server_session::StartupLaunchChoices,
    app_event_tx: &AppEventSender,
    from_model: String,
    target_model: String,
    target_default_effort: ReasoningEffortConfig,
) {
    app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
        history_cell::new_info_event(
            format!("Model {from_model} is no longer available; switched to {target_model}."),
            None,
        ),
    )));
    apply_accepted_model_migration(
        config,
        launch_choices,
        app_event_tx,
        from_model,
        target_model,
        target_default_effort,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;
    use tempfile::tempdir;
    use tokio::sync::mpsc::unbounded_channel;

    fn render_line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[tokio::test]
    async fn automatic_forced_migration_persists_choice_and_notice() {
        let home = tempdir().unwrap_or_else(|err| panic!("scratch CODEX_HOME: {err}"));
        let mut config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .fallback_cwd(Some(home.path().to_path_buf()))
            .build()
            .await
            .unwrap_or_else(|err| panic!("config: {err}"));
        let mut launch_choices = crate::app_server_session::StartupLaunchChoices::default();
        let (tx, mut rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(tx);

        apply(
            &mut config,
            &mut launch_choices,
            &app_event_tx,
            "retired-model".to_string(),
            "current-model".to_string(),
            codex_protocol::openai_models::ReasoningEffort::High,
        );

        assert_eq!(config.model.as_deref(), Some("current-model"));
        assert_eq!(launch_choices.model.as_deref(), Some("current-model"));
        assert_eq!(
            config.model_reasoning_effort,
            Some(codex_protocol::openai_models::ReasoningEffort::High)
        );
        let mut notice = false;
        let mut acknowledged = false;
        let mut persisted = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::InsertHistoryCell(cell) => {
                    let text = cell
                        .transcript_lines(/*width*/ 120)
                        .iter()
                        .map(render_line_text)
                        .collect::<Vec<_>>()
                        .join("\n");
                    notice = text.contains(
                        "Model retired-model is no longer available; switched to current-model.",
                    );
                }
                AppEvent::PersistModelMigrationPromptAcknowledged {
                    from_model,
                    to_model,
                } => {
                    acknowledged = from_model == "retired-model" && to_model == "current-model";
                }
                AppEvent::PersistModelSelection { model, .. } => {
                    persisted = model == "current-model";
                }
                _ => {}
            }
        }
        assert!(notice, "automatic migration records a visible notice");
        assert!(
            acknowledged,
            "automatic migration records its acknowledgement"
        );
        assert!(persisted, "automatic migration persists the replacement");
    }
}
