// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Observe completed native editor paints without creating a continuation grant.

use super::draw_restoring_editor;
use crate::app::App;
use crate::app::owned_transcript::tests::attach_thread;
use crate::app::test_support::make_test_app;
use crate::tui::test_support::make_test_tui;
use codex_protocol::ThreadId;
use color_eyre::Result;
use ratatui::layout::Rect;
use ratatui::layout::Size;

const SIZE: Size = Size::new(80, 24);

async fn primary_app() -> (Box<App>, ThreadId) {
    let mut app = make_test_app().await;
    let primary = ThreadId::new();
    attach_thread(&mut app, primary);
    app.primary_thread_id = Some(primary);
    app.active_thread_id = Some(primary);
    (app, primary)
}

#[tokio::test]
async fn inline_and_owned_frames_complete_the_displayed_primary_editor_layout() -> Result<()> {
    let (mut app, primary) = primary_app().await;
    let mut tui = make_test_tui()?;
    for owned in [false, true] {
        tui.set_owned_screen(owned)?;
        let mut completed = None;
        draw_restoring_editor(
            &mut app,
            &mut tui,
            |app, tui| app.paint_chat_widget_frame(tui, SIZE),
            |_, painted| completed = Some(painted),
        )?;
        assert_eq!(completed, Some(primary), "owned screen: {owned}");
    }
    Ok(())
}

#[tokio::test]
async fn skipped_failed_and_editorless_frames_preserve_input_deferral() -> Result<()> {
    let (mut app, _) = primary_app().await;
    let mut tui = make_test_tui()?;
    tui.set_owned_screen(true)?;
    app.paint_chat_widget_frame(&mut tui, SIZE)?;
    assert!(app.chat_widget.alab_runtime_editor_has_layout());
    draw_restoring_editor(
        &mut app,
        &mut tui,
        |_, _| Ok(Rect::default()),
        |_, _| panic!("a skipped write inherited an older frame"),
    )?;
    let failed = draw_restoring_editor(
        &mut app,
        &mut tui,
        |app, tui| {
            app.paint_chat_widget_frame(tui, SIZE)?;
            Err(std::io::Error::other("frame did not finish").into())
        },
        |_, _| panic!("an unsuccessful frame completed the editor layout"),
    );
    assert_eq!(failed.unwrap_err().to_string(), "frame did not finish");
    assert!(app.chat_widget.alab_runtime_editor_has_layout());
    draw_restoring_editor(
        &mut app,
        &mut tui,
        |_, tui| {
            tui.draw_with_shortcut_hud(SIZE.height, None, |_| {})?;
            Ok(Rect::default())
        },
        |_, _| panic!("an unrelated terminal write inherited the failed editor paint"),
    )?;
    Ok(())
}

#[tokio::test]
async fn display_switch_during_paint_keeps_the_original_primary_deferred() -> Result<()> {
    let (mut app, _) = primary_app().await;
    let mut tui = make_test_tui()?;
    draw_restoring_editor(
        &mut app,
        &mut tui,
        |app, tui| {
            let area = app.paint_chat_widget_frame(tui, SIZE)?;
            app.active_thread_id = Some(ThreadId::new());
            Ok(area)
        },
        |_, _| panic!("a different displayed thread acknowledged the restored editor"),
    )?;
    Ok(())
}

#[tokio::test]
async fn sdk_completion_requires_a_new_focused_paint_on_the_attached_connection() -> Result<()> {
    use super::AfterEditorPaint;
    use super::draw_attached_editor;
    use crate::app::sidebar::PaneFocus;
    use crate::app::sidebar::SidebarContent;
    use crate::app_event_sender::AppEventSender;
    use crate::app_server_session::alab_upgrade_receiver::AttachedEditor;
    use crate::backend_id::BackendId;

    let (mut app, primary) = primary_app().await;
    let mut tui = make_test_tui()?;
    crate::app::test_support::enable_side_retention(&mut app)?;
    tui.set_owned_screen(true)?;
    let backend = BackendId::new();
    app.app_server_backend_id = Some(backend);
    let (original, original_rx) = tokio::sync::mpsc::unbounded_channel();
    app.app_event_tx = AppEventSender::new(original);
    let original_events = app.app_event_tx.clone();
    let owner = AttachedEditor::new(primary, backend, &original_events);
    let mut actions = Vec::new();
    let mut paint = |app: &mut App, sdk_was_active| {
        draw_attached_editor(app, &mut tui, SIZE, &owner, sdk_was_active, |_, action| {
            actions.push(action)
        })
    };
    paint(&mut app, false)?;

    // The SDK reply wakes the actor. A queued switch, chooser or replacement
    // connection may own input by the time that actor next paints.
    app.active_thread_id = Some(ThreadId::new());
    paint(&mut app, true)?;
    app.active_thread_id = Some(primary);
    app.sidebar_mut()
        .show(SidebarContent::Chooser, PaneFocus::Sidebar);
    paint(&mut app, true)?;
    app.sidebar_mut()
        .show(SidebarContent::Chooser, PaneFocus::Main);
    app.overlay = Some(crate::pager_overlay::Overlay::new_transcript(
        Vec::new(),
        app.keymap.pager.clone(),
        false,
        1.0,
    ));
    paint(&mut app, true)?;
    app.overlay = None;
    app.reconnect.offline = true;
    paint(&mut app, true)?;
    app.reconnect.offline = false;
    app.app_server_backend_id = Some(BackendId::new());
    paint(&mut app, true)?;
    app.app_server_backend_id = Some(backend);
    let (replacement, _rx) = tokio::sync::mpsc::unbounded_channel();
    app.app_event_tx = AppEventSender::new(replacement);
    paint(&mut app, true)?;
    app.app_event_tx = original_events;
    paint(&mut app, true)?;
    drop(original_rx);
    paint(&mut app, true)?;

    assert_eq!(
        actions,
        [AfterEditorPaint::Activate, AfterEditorPaint::ReleaseInput]
    );
    Ok(())
}

#[tokio::test]
async fn captured_upgrade_preserves_input_when_the_sidebar_owns_an_idle_primary() -> Result<()> {
    use crate::app::sidebar::PaneFocus;
    use crate::app::sidebar::SidebarContent;
    let (mut app, _) = primary_app().await;
    let mut tui = make_test_tui()?;
    crate::app::test_support::enable_side_retention(&mut app)?;
    tui.set_owned_screen(true)?;
    assert!(super::upgrade_ready(&app));
    assert!(super::composer_owns_input(&app));
    app.sidebar_mut()
        .show(SidebarContent::Chooser, PaneFocus::Sidebar);
    assert!(!super::composer_owns_input(&app));
    let (request, _cancellation) =
        codex_alab_tui::upgrade::UpgradeRequest::with_context::<
            codex_alab_tui::upgrade_terminal::Prepared,
        >(|_| panic!("addressed a composer that did not own the input"));
    assert!(!super::accept_upgrade_and_capture(&app, &tui, request).await?);
    assert!(app.sidebar_has_focus());
    Ok(())
}

#[tokio::test]
async fn source_exec_capture_requires_the_displayed_primary_on_its_backend() -> Result<()> {
    use crate::backend_id::BackendId;
    use std::io::ErrorKind;

    let (mut app, primary) = primary_app().await;
    let tui = make_test_tui()?;
    let backend = BackendId::new();
    app.app_server_backend_id = Some(backend);
    assert!(app.backend_registry.observe(backend, primary));

    async fn assert_refused(app: &App, tui: &crate::tui::Tui) -> Result<()> {
        assert!(super::upgrade_ready(app) && super::composer_owns_input(app));
        let error = super::prepare_primary_upgrade_capture(app, tui)
            .err()
            .expect("capture of a foreign editor");
        assert_eq!(error.kind(), ErrorKind::WouldBlock);
        let (request, _cancellation) =
            codex_alab_tui::upgrade::UpgradeRequest::with_context::<
                codex_alab_tui::upgrade_terminal::Prepared,
            >(|_| panic!("committed another conversation's editor"));
        assert!(!super::accept_upgrade_and_capture(app, tui, request).await?);
        Ok(())
    }

    let side = ThreadId::new();
    attach_thread(&mut app, side);
    app.active_thread_id = Some(side);
    assert_refused(&app, &tui).await?;
    // Bookkeeping may name the primary before its widget is attached.
    app.active_thread_id = Some(primary);
    assert_refused(&app, &tui).await?;
    attach_thread(&mut app, primary);
    // A matching thread UUID on a parked backend cannot supply the editor.
    app.app_server_backend_id = Some(BackendId::new());
    assert_refused(&app, &tui).await?;

    // With matching sources the actor reaches the terminal broker. This
    // fixture has no running reader, so it cannot commit a replacement.
    app.app_server_backend_id = Some(backend);
    assert!(super::upgrade_ready(&app) && super::composer_owns_input(&app));
    let error = super::prepare_primary_upgrade_capture(&app, &tui)
        .err()
        .expect("test terminal has no running reader");
    assert_eq!(error.kind(), ErrorKind::Other);
    Ok(())
}

#[tokio::test]
async fn original_editor_installs_once_on_its_live_bootstrap_backend() -> Result<()> {
    use crate::app_event_sender::AppEventSender;
    use crate::app_server_session::alab_upgrade_receiver::AttachedEditor;
    use crate::backend_id::BackendId;
    use tokio::sync::OnceCell;
    use tokio::sync::mpsc;

    let (mut source, primary) = primary_app().await;
    source
        .chat_widget
        .apply_external_edit("original unsent text".into());
    let checkpoint = source.chat_widget.alab_capture_runtime_editor()?;
    let mut destination = make_test_app().await;
    attach_thread(&mut destination, primary);
    destination
        .chat_widget
        .apply_external_edit("existing destination".into());
    destination.chat_widget.initial_user_message = Some("pending startup input".into());

    let backend = BackendId::new();
    let (tx, _receiver) = mpsc::unbounded_channel();
    let actor = AppEventSender::new(tx);
    let (closed_tx, closed_rx) = mpsc::unbounded_channel();
    let closed = AppEventSender::new(closed_tx);
    drop(closed_rx);
    let attachment = OnceCell::new();

    for (selected, event_sender, bytes) in [
        (BackendId::new(), &actor, checkpoint.as_slice()),
        (backend, &closed, checkpoint.as_slice()),
        (backend, &actor, b"malformed checkpoint".as_slice()),
    ] {
        let refused = AttachedEditor::install(
            &attachment,
            primary,
            backend,
            Some(selected),
            event_sender,
            || {
                Ok(destination
                    .chat_widget
                    .alab_restore_runtime_editor(primary, bytes)?)
            },
        )
        .await;
        assert!(refused.is_err());
        assert!(attachment.get().is_none());
        assert_eq!(
            destination.chat_widget.composer_text_with_pending(),
            "existing destination"
        );
        assert_eq!(
            destination
                .chat_widget
                .initial_user_message
                .as_ref()
                .unwrap()
                .text,
            "pending startup input"
        );
    }

    AttachedEditor::install(&attachment, primary, backend, Some(backend), &actor, || {
        Ok(destination
            .chat_widget
            .alab_restore_runtime_editor(primary, &checkpoint)?)
    })
    .await?;
    assert_eq!(
        destination.chat_widget.composer_text_with_pending(),
        "original unsent text"
    );
    assert!(destination.chat_widget.initial_user_message.is_none());

    destination
        .chat_widget
        .apply_external_edit("typed after attachment".into());
    let (next_tx, _receiver) = mpsc::unbounded_channel();
    let replacement = AppEventSender::new(next_tx);
    for (event_sender, same_owner) in [(&actor, true), (&replacement, false)] {
        let result = AttachedEditor::install(
            &attachment,
            primary,
            backend,
            Some(backend),
            event_sender,
            || {
                Ok(destination
                    .chat_widget
                    .alab_restore_runtime_editor(primary, &checkpoint)?)
            },
        )
        .await;
        assert_eq!(result.is_ok(), same_owner);
        assert_eq!(
            destination.chat_widget.composer_text_with_pending(),
            "typed after attachment"
        );
    }
    Ok(())
}
