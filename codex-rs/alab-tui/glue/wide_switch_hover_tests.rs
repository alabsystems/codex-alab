// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::sidebar::PaneFocus;
use crate::app::sidebar::SidebarContent;
use crate::app::test_support::enable_side_retention;
use crate::app::test_support::side_frame;
use crate::custom_terminal::test_support::last_rendered_buffer;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_tui::session::Autopilot;
use codex_alab_tui::session::HarnessWord;
use codex_alab_tui::session::Shared;
use crossterm::event::MouseButton::Right;
use crossterm::event::MouseEventKind::Down;
use crossterm::event::MouseEventKind::Moved;
use crossterm::event::MouseEventKind::Up;
use ratatui::layout::Size;
use ratatui::style::Modifier;

#[tokio::test]
async fn hovering_named_modes_underlines_only_their_letters_without_selecting_them()
-> color_eyre::eyre::Result<()> {
    let root = codex_alab_tui::test_paths::short_tempdir()?;
    let fake = FakeAterm::start(root.path(), Mode::Current).await?;
    let _interface = scope();
    interface::route_aterm_for_tests(fake.endpoint());
    let shared = Box::leak(Box::new(Shared::new()));
    let _host = codex_alab_tui::boxes::scoped(shared);
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    attach_thread(&mut app, ThreadId::new());
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    let size = tui.terminal.size()?;
    assert_eq!(size, Size::new(80, 24));
    app.chat_widget
        .apply_external_edit("unfinished draft".into());
    app.chat_widget.handle_key_event(KeyCode::Left.into());
    let draft = app.chat_widget.capture_thread_input_state();
    app.render_chat_widget_frame(&mut tui, size)?;
    let before = last_rendered_buffer(&tui.terminal).clone();
    find_text(&before, "[ Cruise ] Land │ Off ▕");
    let land = find_text(&before, " Land ");
    let off = find_text(&before, " Off ");
    let y = land.y;
    let land_ink = Some(land.x + 1..land.x + 5);
    let off_ink = Some(off.x + 1..off.x + 4);
    assert!((land.x..=off.x + 4).all(|x| !before[(x, y)].modifier.contains(Modifier::UNDERLINED)));
    assert_eq!(
        interface::mode_mouse_hit(land.x, y),
        Some(HarnessWord::Land)
    );
    assert_eq!(
        interface::mode_mouse_hit(off.x + 4, y),
        Some(HarnessWord::Off)
    );
    let modified = TuiEvent::Mouse(MouseEvent {
        kind: Moved,
        column: land.x + 1,
        row: y,
        modifiers: KeyModifiers::SHIFT,
    });
    for (event, ink) in [
        (mouse(Moved, land.x, y), land_ink.clone()),
        (mouse(Moved, off.x + 4, y), off_ink.clone()),
        (mouse(Moved, off.x - 1, y), None), // Divider.
        (mouse(Moved, land.x + 1, y), land_ink.clone()),
        (modified, None),
        (mouse(Moved, off.x + 1, y), off_ink),
        (TuiEvent::FocusLost, None),
        (TuiEvent::FocusGained, None),
        (mouse(Moved, land.x + 2, y), land_ink),
        (mouse(Moved, 0, y), None),
    ] {
        app.handle_tui_event(&mut tui, &mut server, event).await?;
        app.render_chat_widget_frame(&mut tui, size)?;
        let after = last_rendered_buffer(&tui.terminal);
        for x in before.area.left()..before.area.right() {
            let mut expected = before[(x, y)].clone();
            if ink.as_ref().is_some_and(|letters| letters.contains(&x)) {
                expected.modifier.insert(Modifier::UNDERLINED);
            }
            assert_eq!(after[(x, y)], expected, "hover {ink:?}, column {x}");
        }
    }
    for kind in [Down(Right), Up(Right)] {
        app.handle_tui_event(&mut tui, &mut server, mouse(kind, land.x + 1, y))
            .await?;
    }
    assert!(
        app.chat_widget
            .active_view_is(interface::AUTOPILOT_PICKER_ID)
    );
    app.render_chat_widget_frame(&mut tui, size)?;
    app.handle_tui_event(&mut tui, &mut server, mouse(Moved, off.x + 1, y))
        .await?;
    // Dismiss the covering view without routing another pointer or focus event.
    app.chat_widget.handle_key_event(KeyCode::Esc.into());
    assert!(!app.chat_widget.has_active_view());
    app.render_chat_widget_frame(&mut tui, size)?;
    assert_eq!(
        last_rendered_buffer(&tui.terminal)[(off.x + 1, y)],
        before[(off.x + 1, y)]
    );

    // A width change collapses the switch. Moving over the new disclosure leaves its ink intact.
    app.handle_tui_event(&mut tui, &mut server, mouse(Moved, land.x, y))
        .await?;
    let narrow = Size::new(28, 24);
    tui.terminal = crate::tui::Terminal::with_screen_size_and_cursor_position_for_test(
        ratatui::backend::CrosstermBackend::new(std::io::stdout()),
        narrow,
        Position::default(),
    );
    app.handle_tui_event(&mut tui, &mut server, TuiEvent::Resize(narrow))
        .await?;
    app.render_chat_widget_frame(&mut tui, narrow)?;
    let arrow = find_text(last_rendered_buffer(&tui.terminal), "▾");
    app.handle_tui_event(&mut tui, &mut server, mouse(Moved, arrow.x, arrow.y))
        .await?;
    app.render_chat_widget_frame(&mut tui, narrow)?;
    assert!(
        !last_rendered_buffer(&tui.terminal)[arrow]
            .modifier
            .contains(Modifier::UNDERLINED)
    );

    // A focused main composer stays clickable beside a chooser. Below 100 columns
    // a focused chooser takes the entire screen, including the old segment.
    enable_side_retention(&mut app)?;
    app.local_settings.tui.animations = false;
    let wide = Size::new(120, 24);
    app.sidebar_mut()
        .show(SidebarContent::Chooser, PaneFocus::Main);
    side_frame(&mut app, &mut tui, wide)?;
    let split = last_rendered_buffer(&tui.terminal);
    let land = split
        .area
        .positions()
        .find(|&pos| {
            split[pos].symbol() == "L"
                && interface::mode_mouse_hit(pos.x, pos.y) == Some(HarnessWord::Land)
        })
        .expect("painted Land segment");
    let chooser = app
        .conversation_frame
        .chooser_area()
        .expect("split chooser");
    assert!(land.x < chooser.left());
    let motion = mouse(Moved, land.x, land.y);
    app.handle_tui_event(&mut tui, &mut server, motion.clone())
        .await?;
    side_frame(&mut app, &mut tui, wide)?;
    assert!(
        last_rendered_buffer(&tui.terminal)[land]
            .modifier
            .contains(Modifier::UNDERLINED)
    );

    app.sidebar_mut()
        .show(SidebarContent::Chooser, PaneFocus::Sidebar);
    side_frame(&mut app, &mut tui, Size::new(80, 24))?;
    assert!(app.conversation_frame.chooser_covers_transcript());
    app.handle_tui_event(&mut tui, &mut server, motion).await?;
    app.sidebar_mut()
        .show(SidebarContent::Chooser, PaneFocus::Main);
    side_frame(&mut app, &mut tui, wide)?;
    assert!(
        !last_rendered_buffer(&tui.terminal)[land]
            .modifier
            .contains(Modifier::UNDERLINED)
    );
    assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
    assert_eq!(shared.harness_word(), HarnessWord::Cruise);
    assert!(
        fake.requests()
            .iter()
            .all(|r| !r.contains(" meta set autopilot "))
    );
    tui.set_owned_screen(false)?;
    server.shutdown().await?;
    Ok(())
}
