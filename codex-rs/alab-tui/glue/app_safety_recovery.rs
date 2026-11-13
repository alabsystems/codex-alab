// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app_command::AppCommand;
use crate::chatwidget::ThreadInputState;
use crate::chatwidget::ThreadInputStateRestoreMode;
use codex_app_server_protocol::UserInput;

const RECOVERY_INSTRUCTION: &str = "[codex-alab] The automatic safety-buffering recovery could not complete. Preserve all existing work and inspect the current state before acting. Do not repeat completed side effects or ask a person to unblock routine work. Continue only the remaining independently authorized work.";

fn recovery_turn(turn: &AppCommand) -> Option<AppCommand> {
    let mut recovery = turn.clone();
    let AppCommand::UserTurn {
        turn_trigger,
        client_user_message_id,
        items,
        alab_safety_retries,
        ..
    } = &mut recovery
    else {
        return None;
    };
    *turn_trigger = "alab_autopilot";
    *client_user_message_id = format!("alab-autopilot-{}", uuid::Uuid::new_v4());
    *alab_safety_retries = alab_safety_retries.saturating_add(1);
    items.insert(
        0,
        UserInput::Text {
            text: RECOVERY_INSTRUCTION.to_string(),
            text_elements: Vec::new(),
        },
    );
    let recovery_text = items
        .iter()
        .filter_map(|item| match item {
            UserInput::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    crate::startup_recovery::bind_submission(&recovery_text, client_user_message_id);
    Some(recovery)
}

pub(super) async fn continue_after_failure(
    app: &mut App,
    app_server: &mut AppServerSession,
    turn: AppCommand,
    input_state: Option<ThreadInputState>,
    prompt: crate::chatwidget::UserMessage,
    interrupted_turn_id: Option<&str>,
) {
    let Some(thread_id) = app.chat_widget.thread_id() else {
        return;
    };
    if !app.chat_widget.alab_autonomous_root_active() {
        app.chat_widget.restore_user_message_to_composer(prompt);
        return;
    }
    if app
        .active_turn_id_for_thread(thread_id)
        .await
        .is_some_and(|active| Some(active.as_str()) != interrupted_turn_id)
    {
        return;
    }
    if !app.chat_widget.alab_autonomous_root_active() {
        app.chat_widget.restore_user_message_to_composer(prompt);
        return;
    }
    if let Some(interrupted_turn_id) = interrupted_turn_id
        && let Some(channel) = app.thread_event_channels.get(&thread_id)
    {
        let mut store = channel.store.lock().await;
        if store.active_turn_id() == Some(interrupted_turn_id) {
            store.clear_active_turn_id();
        }
    }
    let Some(recovery) = recovery_turn(&turn) else {
        return;
    };
    let submitted = app.submit_thread_op(app_server, thread_id, recovery).await;
    if app.chat_widget.is_user_turn_pending_or_running() {
        return;
    }
    app.chat_widget.restore_thread_input_state(
        input_state,
        ThreadInputStateRestoreMode {
            preserve_in_flight_turn: false,
        },
    );
    if let Err(error) = submitted {
        app.chat_widget.add_error_message(format!(
            "Automatic continuation could not be submitted: {error}"
        ));
    }
}

#[cfg(test)]
#[path = "app_safety_validation_tests.rs"]
mod validation_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_command::CloudSubmissionIntent;
    use codex_app_server_protocol::AskForApproval;
    use std::path::PathBuf;

    #[test]
    fn recovery_has_native_attribution_a_fresh_id_and_the_original_context() {
        let original = AppCommand::user_turn(
            "user",
            CloudSubmissionIntent::StartIfIdle,
            "original-id".into(),
            vec![UserInput::Text {
                text: "original request".into(),
                text_elements: Vec::new(),
            }],
            PathBuf::from("/tmp/project"),
            AskForApproval::Never,
            None,
            "model".into(),
            None,
            None,
            None,
            None,
            None,
        );
        let recovery = recovery_turn(&original).expect("user turn recovery");
        let AppCommand::UserTurn {
            turn_trigger,
            client_user_message_id,
            items,
            alab_safety_retries,
            ..
        } = recovery
        else {
            panic!("recovery must remain a user turn");
        };
        assert_eq!(turn_trigger, "alab_autopilot");
        assert!(client_user_message_id.starts_with("alab-autopilot-"));
        assert_ne!(client_user_message_id, "original-id");
        assert_eq!(alab_safety_retries, 1);
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], UserInput::Text { text, .. } if text == RECOVERY_INSTRUCTION));
        assert!(matches!(&items[1], UserInput::Text { text, .. } if text == "original request"));
    }
}
