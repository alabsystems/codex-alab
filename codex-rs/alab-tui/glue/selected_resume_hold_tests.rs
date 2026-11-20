// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::chatwidget::ThreadInputState;
use crate::chatwidget::UserMessage;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc::UnboundedReceiver;

async fn saved_input(text: &str, confirmed: bool, followup: Option<&str>) -> ThreadInputState {
    let (mut widget, _sender, _events, _ops) =
        crate::boxed_future!(crate::chatwidget::tests::make_chatwidget_manual_with_sender()).await;
    if confirmed {
        widget.apply_external_edit(text.into());
        let mut draft = Some(widget.composer_draft_snapshot());
        widget.apply_external_edit(String::new());
        let mut confirmed = true;
        widget.restore_startup_input_when_ready(&mut draft, &mut confirmed);
        assert!(draft.is_none() && !confirmed);
    } else {
        widget.initial_user_message = Some(UserMessage::from(text));
    }
    if let Some(followup) = followup {
        widget.set_queue_autosend_suppressed(true);
        assert!(widget.queue_user_message_with_options(
            UserMessage::from(followup),
            crate::bottom_pane::QueuedInputAction::Literal,
            Vec::new(),
        ));
        widget.set_queue_autosend_suppressed(false);
    }
    widget.capture_thread_input_state().expect("saved input")
}

async fn resume_by_id(
    app: &mut App,
    tui: &mut tui::Tui,
    server: &mut AppServerSession,
    id: ThreadId,
) -> Result<()> {
    Box::pin(app.handle_event(
        tui,
        server,
        AppEvent::ResumeSessionByIdOrName(id.to_string()),
    ))
    .await?;
    assert_eq!(app.chat_widget.thread_id(), Some(id));
    assert_eq!(app.current_displayed_thread_id(), Some(id));
    Ok(())
}

fn submitted(events: &mut UnboundedReceiver<AppEvent>, displayed: ThreadId) -> Vec<Vec<UserInput>> {
    std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::SubmitThreadOp {
                thread_id,
                op: Op::UserTurn { items, .. },
            } => {
                assert_eq!(thread_id, displayed);
                Some(items)
            }
            AppEvent::CodexOp(Op::UserTurn { items, .. }) => Some(items),
            _ => None,
        })
        .collect()
}

fn expected_turn(text: &str) -> Vec<Vec<UserInput>> {
    vec![vec![UserInput::Text {
        text: text.into(),
        text_elements: Vec::new(),
    }]]
}

#[tokio::test]
async fn named_resume_keeps_confirmed_draft_and_cli_until_their_writable_attachment() -> Result<()>
{
    for confirmed in [true, false] {
        let (mut app, _home) = Box::pin(make_history_test_app()).await?;
        std::fs::write(
            app.config.codex_home.join("config.toml"),
            "[tui]\nresume_cwd = 'current'\n",
        )?;
        for cwd in [test_path_buf("/"), app.config.cwd.to_path_buf()] {
            crate::legacy_core::config::set_project_trust_level(
                app.config.codex_home.as_path(),
                &cwd,
                codex_protocol::config_types::TrustLevel::Trusted,
            )
            .map_err(std::io::Error::other)?;
        }
        let locked = create_history_rollout(&app.config, ThreadHistoryMode::Legacy, "Locked")?;
        let mut owner = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        Box::pin(owner.resume_thread(
            &app.local_settings,
            app.config.clone(),
            locked,
            crate::app_server_session::ResumeModelSettings::PreserveExistingThread,
        ))
        .await?;
        let (mut server, requests, proxy) =
            Box::pin(start_recording_app_server(&app.config, None, None)).await?;
        app.app_server_target = AppServerTarget::LocalDaemon {
            allow_embedded_fallback: true,
            endpoint: crate::resolve_remote_addr("ws://127.0.0.1:4500")?,
        };
        let current = Box::pin(server.start_thread(&app.config)).await?;
        app.enqueue_primary_thread_session(current.session, current.turns)
            .await?;
        let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
        app.app_event_tx = AppEventSender::new(tx);
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let text = if confirmed {
            "Confirmed on locked"
        } else {
            "CLI on locked"
        };
        let followup = (!confirmed).then_some("Recovered follow-up");
        app.agents_overview
            .input_states
            .insert(locked, saved_input(text, confirmed, followup).await);

        for _ in 0..2 {
            resume_by_id(&mut app, &mut tui, &mut server, locked).await?;
            assert!(app.chat_widget.is_external_writer_view());
            assert_eq!(
                app.thread_event_channels[&locked].attachment(),
                ThreadEventAttachment::ExternalWriter
            );
            app.chat_widget
                .restore_startup_input_when_ready(&mut None, &mut false);
            app.chat_widget.submit_initial_user_message_if_pending();
            assert!(submitted(&mut events, locked).is_empty());
            if confirmed {
                assert_eq!(app.chat_widget.composer_text_with_pending(), text);
            } else {
                assert_eq!(
                    app.chat_widget.initial_user_message,
                    Some(UserMessage::from(text))
                );
            }
        }
        assert_eq!(recorded_params(&requests, "thread/resume").len(), 2);
        if confirmed {
            let other = create_history_rollout(&app.config, ThreadHistoryMode::Legacy, "Other")?;
            app.agents_overview
                .input_states
                .insert(other, saved_input("CLI on other", false, None).await);
            resume_by_id(&mut app, &mut tui, &mut server, other).await?;
            assert!(!app.chat_widget.is_external_writer_view());
            assert_eq!(submitted(&mut events, other), expected_turn("CLI on other"));
            resume_by_id(&mut app, &mut tui, &mut server, locked).await?;
            assert!(app.chat_widget.is_external_writer_view());
            app.chat_widget
                .restore_startup_input_when_ready(&mut None, &mut false);
            assert!(submitted(&mut events, locked).is_empty());
        }

        owner.shutdown().await?;
        resume_by_id(&mut app, &mut tui, &mut server, locked).await?;
        assert!(!app.chat_widget.is_external_writer_view());
        assert_eq!(
            app.thread_event_channels[&locked].attachment(),
            ThreadEventAttachment::Live
        );
        app.chat_widget
            .restore_startup_input_when_ready(&mut None, &mut false);
        assert_eq!(submitted(&mut events, locked), expected_turn(text));
        resume_by_id(&mut app, &mut tui, &mut server, locked).await?;
        app.chat_widget
            .restore_startup_input_when_ready(&mut None, &mut false);
        app.chat_widget.submit_initial_user_message_if_pending();
        assert!(submitted(&mut events, locked).is_empty());
        if let Some(followup) = followup {
            assert!(!app.chat_widget.maybe_send_next_queued_input());
            assert_eq!(app.chat_widget.queued_user_message_texts(), [followup]);
            assert!(
                app.chat_widget
                    .capture_thread_input_state()
                    .expect("live input")
                    .recovered_queue
            );
        }
        server.shutdown().await?;
        proxy.await??;
    }
    Ok(())
}
