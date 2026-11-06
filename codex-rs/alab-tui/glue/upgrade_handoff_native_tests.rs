// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A queued upgrade reads native input at dispatch, while a person's /exit retains
//! its existing semantics. No running owner session or process-global ALab switch.

use crate::app::AppRunControl;
use crate::app::tests::make_test_app_with_channels;
use crate::app_event::AppEvent;
use crate::app_event::ExitMode;
use codex_alab_tui::upgrade::UpgradeRequest;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;

#[tokio::test]
async fn queued_upgrade_rechecks_a_new_draft_or_turn_and_acks_the_actual_leave()
-> color_eyre::Result<()> {
    for changed in ["none", "draft", "turn"] {
        let (mut app, mut events, _) = make_test_app_with_channels().await;
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let started = server.start_thread(&app.config).await?;
        let thread_id = started.session.thread_id;
        app.active_thread_id = Some(thread_id);
        app.primary_thread_id = Some(thread_id);
        app.chat_widget.handle_thread_session_quiet(started.session);
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let (answer, accepted) = tokio::sync::oneshot::channel();
        let (request, _cancellation) = UpgradeRequest::new(move |ready| {
            let _ = answer.send(ready);
            ready
        });
        app.app_event_tx.send(AppEvent::AlabUpgrade(request));
        match changed {
            "draft" => app
                .chat_widget
                .apply_external_edit("Keep this unfinished draft".into()),
            "turn" => app.chat_widget.handle_server_notification(
                ServerNotification::TurnStarted(TurnStartedNotification {
                    thread_id: thread_id.to_string(),
                    turn: Turn {
                        id: "new-native-turn".into(),
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
            ),
            _ => {}
        }
        let event = std::iter::from_fn(|| events.try_recv().ok())
            .find(|event| matches!(event, AppEvent::AlabUpgrade(_)))
            .unwrap_or_else(|| panic!("the upgrade event was queued"));
        let control = app.handle_event(&mut tui, &mut server, event).await?;
        let committed = accepted.await.unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(committed, changed == "none");
        assert_eq!(matches!(control, AppRunControl::Exit(_)), committed);
        if changed == "draft" {
            assert_eq!(
                app.chat_widget.composer_text_with_pending(),
                "Keep this unfinished draft"
            );
            let explicit = app
                .handle_event(&mut tui, &mut server, AppEvent::Exit(ExitMode::Immediate))
                .await?;
            assert!(
                matches!(explicit, AppRunControl::Exit(_)),
                "a person's explicit exit stays unconditional"
            );
        }
        server.shutdown().await?;
    }
    Ok(())
}
