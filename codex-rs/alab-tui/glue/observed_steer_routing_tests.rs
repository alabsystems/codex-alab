// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::session_lifecycle_requests::recorded_params;
use super::session_lifecycle_requests::start_recording_remote_app_server;
use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn a_delayed_steer_stays_with_its_original_thread_after_selection() -> Result<()> {
    let (mut app, _events, mut ops) = make_test_app_with_channels().await;
    let (mut server, requests, proxy) = start_recording_remote_app_server(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let source = ThreadId::new();
    let destination = ThreadId::new();
    for thread in [source, destination] {
        app.thread_event_channels.insert(
            thread,
            ThreadEventChannel::new_with_session(
                THREAD_EVENT_CHANNEL_CAPACITY,
                test_thread_session(thread, app.config.cwd.to_path_buf()),
                Vec::new(),
            ),
        );
    }
    app.primary_thread_id = Some(source);
    app.activate_thread_channel(source).await;
    app.chat_widget
        .handle_thread_session_quiet(test_thread_session(source, app.config.cwd.to_path_buf()));
    let started = turn_started_notification(source, "observed");
    app.thread_event_channels[&source]
        .store
        .lock()
        .await
        .push_notification(started.clone());
    app.chat_widget.handle_server_notification(started, None);
    app.chat_widget.apply_external_edit("source steer".into());
    app.chat_widget
        .handle_key_event(KeyEvent::from(KeyCode::Enter));
    let op = next_user_turn_op(&mut ops);

    Box::pin(app.select_agent_thread(&mut tui, &mut server, destination)).await?;
    assert_eq!(app.chat_widget.thread_id(), Some(destination));
    app.chat_widget
        .apply_external_edit("destination draft".into());
    Box::pin(app.handle_event(&mut tui, &mut server, AppEvent::CodexOp(op))).await?;
    assert!(recorded_params(&requests, "turn/start").is_empty());
    assert!(recorded_params(&requests, "turn/steer").is_empty());
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "destination draft"
    );
    assert!(app.chat_widget.queued_user_message_texts().is_empty());

    Box::pin(app.select_agent_thread(&mut tui, &mut server, source)).await?;
    assert_eq!(app.chat_widget.thread_id(), Some(source));
    assert_eq!(
        app.chat_widget.queued_user_message_texts(),
        ["source steer"]
    );
    assert!(!app.chat_widget.maybe_send_next_queued_input());
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(app.chat_widget.composer_text_with_pending(), "source steer");
    assert!(app.chat_widget.queued_user_message_texts().is_empty());
    assert!(recorded_params(&requests, "turn/start").is_empty());
    assert!(recorded_params(&requests, "turn/steer").is_empty());
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}
