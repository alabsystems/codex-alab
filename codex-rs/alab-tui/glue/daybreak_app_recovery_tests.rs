// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A queued correction preserves the draft whether the active harness keeps an access
//! offer informational or the person closes its picker through the real App handshake.

use crate::app::tests::make_test_app_with_channels;
use crate::app_command::AppCommand;
use crate::app_event::AppEvent;
use crate::bottom_pane::QueuedInputAction;
use crate::chatwidget::UserMessage;
use crate::chatwidget::tests::render_bottom_popup;
use crate::status::StatusAccountDisplay;
use codex_alab_tui::boxes;
use codex_app_server_protocol::CodexErrorInfo;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_protocol::openai_models::ModelAccessPrograms;
use codex_protocol::turn_input::CyberAccessProgram;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn declining_daybreak_releases_the_queued_correction_through_app_events_and_keeps_the_draft()
-> color_eyre::eyre::Result<()> {
    assert_daybreak_correction(false).await
}

#[tokio::test]
async fn an_active_harness_keeps_the_access_offer_non_modal_and_preserves_the_queued_correction()
-> color_eyre::eyre::Result<()> {
    assert_daybreak_correction(true).await
}

async fn assert_daybreak_correction(active_harness: bool) -> color_eyre::eyre::Result<()> {
    const CORRECTION: &str = "Explain ordinary error handling in this repository.";
    const DRAFT: &str = "The next question is still being edited.";

    let shared = Box::leak(Box::new(boxes::Shared::new()));
    let _scope = boxes::scoped(shared);
    let (mut app, mut events, mut operations) = make_test_app_with_channels().await;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let started = server.start_thread(&app.config).await?;
    let thread_id = started.session.thread_id;
    if active_harness {
        let key = thread_id.to_string();
        shared.observe(
            &codex_alab_tui::session::Frame {
                primary_thread: Some(&key),
                displayed_thread: Some(&key),
                rollout_path: None,
                cwd: app.config.cwd.as_path(),
                model: app.chat_widget.current_model(),
                codex_home: &app.config.codex_home,
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
        shared.note_root_thread(&key);
        shared.set_claim(Some("codex-alab@test".into()));
        shared.set_autopilot(codex_alab_tui::session::Autopilot::Active);
    }
    app.active_thread_id = Some(thread_id);
    app.primary_thread_id = Some(thread_id);
    app.chat_widget.handle_thread_session_quiet(started.session);
    app.chat_widget.set_model("gpt-5.6-sol");
    let account = StatusAccountDisplay::ChatGpt {
        email: Some("selected@example.test".into()),
        plan: Some("Pro".into()),
        workspace: None,
    };
    app.chat_widget
        .update_account_state(Some(account.clone()), None, true, true);
    app.chat_widget
        .set_feature_enabled(Feature::CliDaybreak, true);

    // Supply the selected account's catalog through the widget's real request boundary.
    app.chat_widget.open_model_popup();
    let request_id = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::FetchModels { request_id }
                if app.chat_widget.model_popup_request_is_current(request_id) =>
            {
                Some(request_id)
            }
            _ => None,
        })
        .expect("the model picker requests its account-scoped catalog");
    let mut model = crate::test_support::TEST_MODEL_PRESETS[0].clone();
    model.model = app.chat_widget.current_model().into();
    model.available_access_programs = Some(ModelAccessPrograms {
        cyber: vec![
            CyberAccessProgram::Standard,
            CyberAccessProgram::DaybreakBlue,
        ],
    });
    assert!(
        app.chat_widget
            .on_models_loaded(request_id, Ok(vec![model]))
    );
    app.chat_widget.handle_key_event(KeyCode::Esc.into());
    let mut tui = crate::tui::test_support::make_test_tui()?;
    while let Ok(event) = events.try_recv() {
        app.handle_event(&mut tui, &mut server, event).await?;
    }
    while operations.try_recv().is_ok() {}

    app.chat_widget.handle_server_notification(
        ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: thread_id.to_string(),
            turn: Turn {
                id: "refused-turn".into(),
                root_turn_id: None,
                items_view: TurnItemsView::Full,
                items: Vec::new(),
                status: TurnStatus::InProgress,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
            },
        }),
        None,
    );
    assert!(app.chat_widget.queue_user_message_with_options(
        UserMessage::from(CORRECTION),
        QueuedInputAction::Plain,
        Vec::new(),
    ));
    app.chat_widget.apply_external_edit(DRAFT.into());
    assert!(operations.try_recv().is_err());
    app.chat_widget.handle_server_notification(
        ServerNotification::Error(ErrorNotification {
            error: TurnError {
                message: "The cybersecurity access policy refused this turn.".into(),
                codex_error_info: Some(CodexErrorInfo::CyberPolicy),
                additional_details: None,
                misalignment: None,
            },
            will_retry: false,
            thread_id: thread_id.to_string(),
            turn_id: "refused-turn".into(),
        }),
        None,
    );
    app.chat_widget.pre_draw_tick();
    if active_harness {
        assert!(app.chat_widget.no_modal_or_popup_active());
        assert!(!app.chat_widget.has_active_view());
    } else {
        assert!(render_bottom_popup(&app.chat_widget, 80).contains("Turn on Daybreak"));
        assert!(app.chat_widget.has_queued_follow_up_messages());
    }
    assert_eq!(app.chat_widget.composer_text_with_pending(), DRAFT);
    assert_eq!(
        shared.open_box(),
        None,
        "the access choice stays with the person"
    );
    if !active_harness {
        assert!(operations.try_recv().is_err());
        app.chat_widget.handle_key_event(KeyCode::Down.into());
        app.chat_widget
            .handle_key_event(KeyEvent::from(KeyCode::Enter));
        assert!(app.chat_widget.no_modal_or_popup_active());
        assert!(app.chat_widget.has_queued_follow_up_messages());
        assert!(
            operations.try_recv().is_err(),
            "the App has not settled the close yet"
        );
    }

    let mut closed = 0;
    let mut settled = 0;
    while let Ok(event) = events.try_recv() {
        assert!(
            !matches!(
                event,
                AppEvent::PersistDaybreakSelection { .. } | AppEvent::OpenUrlInBrowser { .. }
            ),
            "declining access never enrolls, enables or opens a browser: {event:?}"
        );
        closed += usize::from(matches!(event, AppEvent::SettingsSelectionClosed));
        settled += usize::from(matches!(event, AppEvent::SettingsSelectionSettled));
        app.handle_event(&mut tui, &mut server, event).await?;
    }
    assert_eq!(
        (closed, settled),
        if active_harness { (0, 0) } else { (1, 1) }
    );
    let submissions = std::iter::from_fn(|| operations.try_recv().ok())
        .filter_map(|operation| match operation {
            AppCommand::UserTurn { items, model, .. } => Some((items, model)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        submissions,
        vec![(
            vec![UserInput::Text {
                text: CORRECTION.into(),
                text_elements: Vec::new(),
            }],
            "gpt-5.6-sol".into(),
        )]
    );
    assert!(!app.chat_widget.has_queued_follow_up_messages());
    assert!(app.chat_widget.is_user_turn_pending_or_running());
    assert!(
        app.chat_widget.alab_pending_user_turn(),
        "the native autopilot must see the accepted correction awaiting its turn"
    );
    assert_eq!(app.chat_widget.composer_text_with_pending(), DRAFT);
    assert_eq!(app.chat_widget.status_account_display(), Some(&account));
    assert_eq!(app.chat_widget.current_model(), "gpt-5.6-sol");
    server.shutdown().await?;
    Ok(())
}
