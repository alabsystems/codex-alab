// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Daybreak notices through the real widget and ALab's box observer. A reminder must not
//! take the composer; a refused request must leave access decisions to its account owner.

use std::sync::Arc;

use crate::app_command::AppCommand;
use crate::app_event::AppEvent;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::chatwidget::tests::render_bottom_popup;
use crate::chatwidget::tests::set_chatgpt_auth;
use crate::model_catalog::ModelCatalog;
use crate::security_setup::Identity;
use crate::security_setup::Notice;
use crate::terminal_program_status::ProgramStatus;
use codex_alab_tui::boxes;
use codex_alab_tui::boxes::Shared;
use codex_app_server_protocol::CodexErrorInfo;
use codex_app_server_protocol::ModelVerification;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ModelAccessPrograms;
use codex_protocol::turn_input::CyberAccessProgram;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::mpsc::UnboundedReceiver;

use super::tests::turn;

const DRAFT: &str = "Describe the repository's ordinary error handling.";

fn configure_daybreak_offer(chat: &mut crate::chatwidget::ChatWidget) -> ThreadId {
    chat.set_model("gpt-5.6-sol");
    set_chatgpt_auth(chat);
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    chat.set_feature_enabled(Feature::CliDaybreak, true);
    let mut model = crate::test_support::TEST_MODEL_PRESETS[0].clone();
    model.model = chat.current_model().into();
    model.available_access_programs = Some(ModelAccessPrograms {
        cyber: vec![
            CyberAccessProgram::Standard,
            CyberAccessProgram::DaybreakBlue,
        ],
    });
    chat.model_catalog = Arc::new(ModelCatalog::new(vec![model]));
    thread
}

fn shared() -> &'static Shared {
    Box::leak(Box::new(Shared::new()))
}

fn assert_draft_submission(operations: &mut UnboundedReceiver<AppCommand>) {
    let submissions = std::iter::from_fn(|| operations.try_recv().ok())
        .filter_map(|op| match op {
            AppCommand::UserTurn { items, .. } => Some(items),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        submissions,
        vec![vec![UserInput::Text {
            text: DRAFT.into(),
            text_elements: Vec::new(),
        }]]
    );
}

#[tokio::test]
async fn daybreak_security_reminder_preserves_and_submits_the_draft() {
    let shared = shared();
    let _scope = boxes::scoped(shared);
    let (mut chat, _sender, mut events, mut operations) =
        make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    chat.apply_external_edit(DRAFT.into());
    let notice: Notice = serde_json::from_value(json!({
        "title": "Set up security for Daybreak mode",
        "description": "Set up Advanced Account Security. You can keep using Codex while you finish setup.",
        "action": {"label": "Set up security", "url": "https://chatgpt.com/cyber"}
    }))
    .expect("security reminder");
    chat.show_security_setup(
        Identity {
            account: "account".into(),
            user: "user".into(),
        },
        notice,
    );
    chat.pre_draw_tick();

    assert!(render_bottom_popup(&chat, 80).contains("Set up security"));
    assert!(chat.no_modal_or_popup_active());
    assert_eq!(shared.open_box(), None);
    assert_eq!(chat.desired_program_status(), ProgramStatus::Idle);
    assert_eq!(chat.composer_text_with_pending(), DRAFT);
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    assert_draft_submission(&mut operations);
    assert!(operations.try_recv().is_err());
    assert!(!chat.daybreak_enabled);
    assert!(
        std::iter::from_fn(|| events.try_recv().ok()).all(|event| !matches!(
            event,
            AppEvent::OpenUrlInBrowser { .. } | AppEvent::PersistDaybreakSelection { .. }
        ))
    );
}

#[tokio::test]
async fn daybreak_verification_warning_preserves_work_and_the_next_draft() {
    let shared = shared();
    let _scope = boxes::scoped(shared);
    let (mut chat, _sender, mut events, mut operations) =
        make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    turn(&mut chat, "turn-before-notice", TurnStatus::InProgress);
    chat.apply_external_edit(DRAFT.into());
    chat.on_app_server_model_verification(&[ModelVerification::TrustedAccessForCyber]);
    chat.pre_draw_tick();

    assert_eq!(chat.desired_program_status(), ProgramStatus::Working);
    assert_eq!(chat.composer_text_with_pending(), DRAFT);
    assert!(chat.no_modal_or_popup_active());
    assert_eq!(shared.open_box(), None);
    assert!(operations.try_recv().is_err());
    turn(&mut chat, "turn-before-notice", TurnStatus::Completed);
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    assert_draft_submission(&mut operations);
    assert!(!chat.daybreak_enabled);
    assert!(
        std::iter::from_fn(|| events.try_recv().ok())
            .all(|event| !matches!(event, AppEvent::PersistDaybreakSelection { .. }))
    );
}

#[tokio::test]
async fn daybreak_refusal_keeps_the_access_choice_and_corrected_draft_with_the_person() {
    let shared = shared();
    let _scope = boxes::scoped(shared);
    let (mut chat, _sender, mut events, mut operations) =
        make_chatwidget_manual_with_sender().await;
    configure_daybreak_offer(&mut chat);
    turn(&mut chat, "refused-turn", TurnStatus::InProgress);
    chat.apply_external_edit(DRAFT.into());
    chat.handle_non_retry_error("request refused".into(), Some(CodexErrorInfo::CyberPolicy));
    chat.pre_draw_tick();
    chat.pre_draw_tick();

    assert!(chat.has_active_view());
    assert!(!chat.bottom_pane.is_task_running());
    assert_eq!(
        shared.open_box(),
        None,
        "this access choice is no plan or tool question"
    );
    assert!(!chat.daybreak_enabled);
    assert_eq!(chat.current_model(), "gpt-5.6-sol");
    assert_eq!(chat.composer_text_with_pending(), DRAFT);
    assert!(operations.try_recv().is_err());
    assert!(
        std::iter::from_fn(|| events.try_recv().ok())
            .all(|event| !matches!(event, AppEvent::PersistDaybreakSelection { .. }))
    );

    // The person declines the offer. As the App does after SettingsSelectionSettled,
    // release the settings queue before submitting their corrected request.
    chat.handle_key_event(KeyEvent::from(KeyCode::Down));
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    assert!(chat.no_modal_or_popup_active());
    assert!(
        std::iter::from_fn(|| events.try_recv().ok())
            .all(|event| !matches!(event, AppEvent::PersistDaybreakSelection { .. }))
    );
    chat.set_queue_autosend_suppressed(false);
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    assert_draft_submission(&mut operations);
    assert!(!chat.daybreak_enabled);
    assert_eq!(chat.current_model(), "gpt-5.6-sol");
}

#[tokio::test]
async fn only_an_active_claimed_root_keeps_the_automatic_access_offer_non_modal() {
    use codex_alab_tui::session::Autopilot;

    for (mode, claimed, root, non_modal) in [
        (Autopilot::Active, true, true, true),
        (Autopilot::Passive, true, true, false),
        (Autopilot::Off, true, true, false),
        (Autopilot::Active, false, true, false),
        (Autopilot::Active, true, false, false),
    ] {
        let shared = shared();
        let _scope = boxes::scoped(shared);
        let (mut chat, _sender, mut events, mut operations) =
            make_chatwidget_manual_with_sender().await;
        let thread = configure_daybreak_offer(&mut chat);
        let key = thread.to_string();
        let config = chat.config_ref();
        shared.observe(
            &codex_alab_tui::session::Frame {
                primary_thread: Some(&key),
                displayed_thread: Some(&key),
                rollout_path: None,
                cwd: config.cwd.as_path(),
                model: chat.current_model(),
                codex_home: &config.codex_home,
                embedded: true,
                state: codex_alab_tui::session::ProgramState::Idle,
                draft_empty: true,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            },
            codex_alab_tui::boxes::now_ms(),
        );
        shared.set_autopilot(mode);
        if claimed {
            shared.set_claim(Some("codex-alab@test".into()));
        }
        if root {
            shared.note_root_thread(&thread.to_string());
        }
        turn(&mut chat, "refused-turn", TurnStatus::InProgress);
        chat.apply_external_edit(DRAFT.into());
        chat.handle_non_retry_error("request refused".into(), Some(CodexErrorInfo::CyberPolicy));
        chat.pre_draw_tick();
        assert_eq!(
            chat.no_modal_or_popup_active(),
            non_modal,
            "{mode:?} {claimed} {root}"
        );
        assert_eq!(chat.has_active_view(), !non_modal);
        assert_eq!(chat.composer_text_with_pending(), DRAFT);
        assert!(!chat.daybreak_enabled);
        assert_eq!(chat.current_model(), "gpt-5.6-sol");
        assert!(
            operations.try_recv().is_err(),
            "an unsent draft remains unsent"
        );
        assert!(
            std::iter::from_fn(|| events.try_recv().ok()).all(|event| !matches!(
                event,
                AppEvent::PersistDaybreakSelection { .. } | AppEvent::OpenUrlInBrowser { .. }
            ))
        );
    }
}
