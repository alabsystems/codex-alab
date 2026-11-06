// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::tests::make_test_app_with_channels;
use crate::pets::PetImageRenderState;
use codex_alab_tui::interface;

#[tokio::test]
async fn disabling_pet_releases_images_while_a_hidden_pet_keeps_its_cache() -> Result<()> {
    let state = Box::leak(Box::new(interface::State::new()));
    let _scope = interface::scoped(state);
    interface::configure(true);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let mut app_server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.chat_widget
        .set_pet_image_support_for_tests(crate::pets::PetImageSupport::Supported(
            crate::pets::ImageProtocol::Kitty,
        ));
    app.chat_widget.install_test_ambient_pet_for_tests(false);
    assert!(app.chat_widget.ambient_pet_image_enabled());
    let releases = PetImageRenderState::alab_release_calls_for_test();

    // The 80-column fixture cannot fit the dock, but the configured pet remains
    // alive. Its cached frames must survive a temporary lack of room.
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert_eq!(PetImageRenderState::alab_release_calls_for_test(), releases);

    app.sync_tui_pet_disabled();
    assert!(!app.chat_widget.ambient_pet_image_enabled());
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert_eq!(
        PetImageRenderState::alab_release_calls_for_test(),
        releases + 1,
        "the real disabled-pet draw path must release the terminal image cache"
    );
    drop(_scope);
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert_eq!(
        PetImageRenderState::alab_release_calls_for_test(),
        releases + 1,
        "the inactive upstream interface keeps its native draw behavior"
    );
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn full_screen_overlays_hide_the_pet_without_releasing_its_frames() -> Result<()> {
    let state = Box::leak(Box::new(interface::State::new()));
    let scope = interface::scoped(state);
    interface::configure(true);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let mut app_server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.chat_widget
        .set_pet_image_support_for_tests(crate::pets::PetImageSupport::Supported(
            crate::pets::ImageProtocol::Kitty,
        ));
    app.chat_widget.install_test_ambient_pet_for_tests(false);

    for owned in [false, true] {
        tui.set_owned_screen(owned)?;
        for enabled in [true, false] {
            interface::set_enabled(enabled);
            app.overlay = Some(crate::pager_overlay::Overlay::new_static_with_lines(
                vec!["Full-screen content".into()],
                "Overlay".into(),
                app.keymap.pager.clone(),
            ));
            let clears = PetImageRenderState::alab_clear_calls_for_test();
            let releases = PetImageRenderState::alab_release_calls_for_test();
            app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
                .await?;
            assert_eq!(
                PetImageRenderState::alab_clear_calls_for_test(),
                clears + 1,
                "the overlay must hide the previous placement: owned={owned}, enabled={enabled}"
            );
            assert_eq!(PetImageRenderState::alab_release_calls_for_test(), releases);
            assert!(app.chat_widget.ambient_pet_image_enabled());
            app.close_transcript_overlay(&mut tui);
            let area = Rect::new(0, 0, 100, 24);
            assert!(
                app.chat_widget
                    .ambient_pet_draw(area, area.bottom())
                    .is_some()
            );
        }
    }
    tui.set_owned_screen(false)?;
    drop(scope);
    app.overlay = Some(crate::pager_overlay::Overlay::new_static_with_lines(
        vec!["Native overlay".into()],
        "Overlay".into(),
        app.keymap.pager.clone(),
    ));
    let clears = PetImageRenderState::alab_clear_calls_for_test();
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert_eq!(PetImageRenderState::alab_clear_calls_for_test(), clears);
    app.close_transcript_overlay(&mut tui);
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_diff_overlay_hides_the_picker_preview_and_restores_it_on_return() -> Result<()> {
    let state = Box::leak(Box::new(interface::State::new()));
    let _scope = interface::scoped(state);
    interface::configure(true);
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let mut app_server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.chat_widget
        .set_pet_image_support_for_tests(crate::pets::PetImageSupport::Supported(
            crate::pets::ImageProtocol::Kitty,
        ));
    app.chat_widget.install_test_ambient_pet_for_tests(false);
    app.chat_widget.open_pets_picker();
    let request_id = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if let Some(AppEvent::PetPreviewLoaded { request_id, .. }) = events.recv().await {
                break request_id;
            }
        }
    })
    .await?;
    let home = tempfile::tempdir()?;
    crate::pets::write_test_pack(home.path());
    let pet = crate::pets::load_pet_with_assets(
        crate::pets::DEFAULT_PET_ID.to_owned(),
        codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(home.path())?,
        tui.frame_requester(),
        false,
        &app.chat_widget.pet_http_client,
    )
    .await
    .expect("load the cached preview fixture");
    app.handle_pet_preview_loaded(&mut tui, request_id, Ok(pet));
    tui.set_owned_screen(true)?;
    let size = Size::new(120, 40);
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Resize(size))
        .await?;
    assert!(app.chat_widget.pet_picker_preview_draw().is_some());

    crate::test_support::boxed_future!(app.handle_event(
        &mut tui,
        &mut app_server,
        AppEvent::DiffResult(app.config.cwd.to_path_buf(), "A delayed diff".into()),
    ))
    .await?;
    assert!(app.overlay.is_some());
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert!(
        !app.chat_widget.should_clear_pet_picker_preview_image(),
        "the diff must have cleared the previous preview placement"
    );
    app.close_transcript_overlay(&mut tui);
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Resize(size))
        .await?;
    assert!(app.chat_widget.pet_picker_preview_draw().is_some());
    tui.draw_pet_picker_preview_image(None)?;
    tui.set_owned_screen(false)?;
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_queued_pet_disable_releases_images_while_an_overlay_stays_open() -> Result<()> {
    let state = Box::leak(Box::new(interface::State::new()));
    let _scope = interface::scoped(state);
    interface::configure(true);
    for owned in [false, true] {
        let (mut app, mut events, _ops) = make_test_app_with_channels().await;
        let mut app_server =
            Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(owned)?;
        app.chat_widget
            .set_pet_image_support_for_tests(crate::pets::PetImageSupport::Supported(
                crate::pets::ImageProtocol::Kitty,
            ));
        app.chat_widget.install_test_ambient_pet_for_tests(false);
        app.overlay = Some(crate::pager_overlay::Overlay::new_static_with_lines(
            vec!["Full-screen content".into()],
            "Overlay".into(),
            app.keymap.pager.clone(),
        ));
        let releases = PetImageRenderState::alab_release_calls_for_test();
        app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
            .await?;
        assert_eq!(PetImageRenderState::alab_release_calls_for_test(), releases);
        assert!(app.chat_widget.ambient_pet_image_enabled());

        app.app_event_tx.send(AppEvent::PetDisabled);
        let disabled = loop {
            let event = events.try_recv()?;
            if matches!(event, AppEvent::PetDisabled) {
                break event;
            }
        };
        crate::test_support::boxed_future!(app.handle_event(&mut tui, &mut app_server, disabled))
            .await?;
        assert!(
            app.overlay.is_some(),
            "the queued disable keeps the overlay open"
        );
        assert!(!app.chat_widget.ambient_pet_image_enabled());
        app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
            .await?;
        assert_eq!(
            PetImageRenderState::alab_release_calls_for_test(),
            releases + 1,
            "disabled-pet cleanup must run under an open overlay: owned={owned}"
        );
        assert!(app.overlay.is_some());
        app.close_transcript_overlay(&mut tui);
        tui.set_owned_screen(false)?;
        app_server.shutdown().await?;
    }
    Ok(())
}
