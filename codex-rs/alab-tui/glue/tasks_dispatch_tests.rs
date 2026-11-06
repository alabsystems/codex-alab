// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[tokio::test]
async fn offline_app_dispatch_keeps_task_inspection_refresh_and_back_local_but_blocks_agents()
-> color_eyre::eyre::Result<()> {
    let _scope = scope();
    let (mut app, mut events, mut operations) = make_test_app_with_channels().await;
    let thread = ThreadId::from_u128(712);
    attach_thread(&mut app, thread);
    let active_thread = app.active_thread_id;
    app.chat_widget
        .apply_external_edit("unfinished offline draft".into());
    let draft = app.chat_widget.capture_thread_input_state();
    plan(&mut app, &[("Inspect offline step", StepStatus::Pending)]);
    let key = app.alab_tasks()[0].key.clone();
    let agent = ThreadId::from_u128(713);
    app.agent_navigation
        .upsert(agent, Some("Worker".into()), None, false);
    let mut session = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.reconnect.offline = true;
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}

    app.handle_event(&mut tui, &mut session, AppEvent::OpenAlabTasks)
        .await?;
    assert!(app.chat_widget.alab_task_view_state().is_some());
    let (selected_index, query) = app.chat_widget.alab_task_view_state().unwrap();
    plan(&mut app, &[("Inspect offline step", StepStatus::Completed)]);
    let lifetime = Arc::new(());
    app.handle_event(
        &mut tui,
        &mut session,
        AppEvent::AlabTasksRefresh {
            view: Arc::downgrade(&lifetime),
            thread_id: Some(thread),
            selected_key: Some(key.clone()),
            selected_index,
            query,
        },
    )
    .await?;
    let rendered = crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 100);
    assert!(rendered.contains("[x] Inspect offline step"), "{rendered}");
    let selection = app.chat_widget.alab_task_view_state();
    app.handle_event(&mut tui, &mut session, AppEvent::SelectAgentThread(agent))
        .await?;
    assert_eq!(
        app.active_thread_id, active_thread,
        "agent navigation remains blocked offline"
    );
    assert_eq!(app.chat_widget.alab_task_view_state(), selection);
    assert!(
        events.try_recv().is_err(),
        "blocked navigation produces no server action or history"
    );

    app.chat_widget
        .handle_disconnected_view_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), None);
    let details = events.try_recv()?;
    assert!(
        matches!(&details, AppEvent::AlabTaskDetails { thread_id: Some(id), key: selected } if *id == thread && selected == &key)
    );
    app.handle_event(&mut tui, &mut session, details).await?;
    assert!(app.chat_widget.active_view_is("alab-task-details"));
    let rendered = crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 100);
    assert!(rendered.contains("Inspect offline step"));
    assert!(rendered.contains("Completed"));
    app.chat_widget
        .handle_disconnected_view_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), None);
    let back = events.try_recv()?;
    assert!(matches!(&back, AppEvent::AlabTasksBack { thread_id: Some(id) } if *id == thread));
    app.handle_event(&mut tui, &mut session, back).await?;
    assert!(app.chat_widget.alab_task_view_state().is_some());
    app.chat_widget
        .handle_disconnected_view_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), None);
    assert!(!app.chat_widget.has_active_view());
    assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
    assert!(
        operations.try_recv().is_err(),
        "local inspection never submits model work"
    );
    assert!(
        events.try_recv().is_err(),
        "cancel does not reopen the picker"
    );
    session.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn an_online_picker_disables_agents_immediately_on_disconnect_and_refresh_reenables_them()
-> color_eyre::eyre::Result<()> {
    let _scope = scope();
    let (mut app, mut events, mut operations) = make_test_app_with_channels().await;
    let thread = ThreadId::from_u128(714);
    let agent = ThreadId::from_u128(715);
    attach_thread(&mut app, thread);
    app.agent_navigation
        .upsert(agent, Some("Worker".into()), None, false);
    app.chat_widget
        .apply_external_edit("draft while agents are offline".into());
    let draft = app.chat_widget.capture_thread_input_state();
    let mut config = TuiKeymap::default();
    config.list.accept = Some(KeybindingsSpec::One(KeybindingSpec("ctrl-x enter".into())));
    config.list.cancel = Some(KeybindingsSpec::One(KeybindingSpec("f12".into())));
    let runtime = RuntimeKeymap::from_config(&config).unwrap();
    app.chat_widget.apply_keymap_update(config, &runtime);
    app.keymap = runtime;
    app.app_server_target = crate::AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr("ws://127.0.0.1:8765")?,
    };
    let mut session = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;

    app.alab_open_tasks();
    app.chat_widget.handle_paste("Worker".into());
    assert!(
        !crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 46)
            .contains("Reconnect to open")
    );
    assert!(app.begin_reconnect());
    let shown = crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 46);
    assert!(shown.contains("Reconnect to open"), "{shown}");
    let narrow = tasks_text_at(&app, 46, Some(8));
    assert!(narrow.contains("Reconnect to open"), "{narrow}");
    let selection = app.chat_widget.alab_task_view_state();
    assert_eq!(selection.as_ref().unwrap().0, Some(0));
    assert_eq!(selection.as_ref().unwrap().1.as_deref(), Some("Worker"));
    let active_thread = app.active_thread_id;
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}

    for key in [
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ] {
        app.handle_tui_event(&mut tui, &mut session, crate::tui::TuiEvent::Key(key))
            .await?;
    }
    assert_eq!(app.chat_widget.alab_task_view_state(), selection);
    assert_eq!(app.active_thread_id, active_thread);
    assert!(
        events.try_recv().is_err(),
        "a disabled row emits no agent action"
    );
    assert!(operations.try_recv().is_err());
    assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
    app.handle_event(&mut tui, &mut session, AppEvent::SelectAgentThread(agent))
        .await?;
    assert_eq!(app.chat_widget.alab_task_view_state(), selection);
    assert_eq!(app.active_thread_id, active_thread);

    app.reconnect.offline = false;
    let (selected_index, query) = app.chat_widget.alab_task_view_state().unwrap();
    let lifetime = Arc::new(());
    app.handle_event(
        &mut tui,
        &mut session,
        AppEvent::AlabTasksRefresh {
            view: Arc::downgrade(&lifetime),
            thread_id: Some(thread),
            selected_key: Some(format!("agent:{agent}")),
            selected_index,
            query,
        },
    )
    .await?;
    let shown = crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 46);
    assert!(shown.contains("Worker"), "{shown}");
    assert!(!shown.contains("Reconnect to open"), "{shown}");
    for key in [
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ] {
        app.handle_tui_event(&mut tui, &mut session, crate::tui::TuiEvent::Key(key))
            .await?;
    }
    assert!(matches!(events.try_recv(), Ok(AppEvent::SelectAgentThread(id)) if id == agent));
    assert!(!app.chat_widget.has_active_view());
    assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
    assert!(operations.try_recv().is_err());
    session.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn disconnect_refresh_keeps_the_search_and_selected_enabled_source() {
    let _scope = scope();
    let (mut app, _events, _operations) = make_test_app_with_channels().await;
    attach_thread(&mut app, ThreadId::from_u128(718));
    plan(
        &mut app,
        &[
            ("Build step", StepStatus::Pending),
            ("Review step", StepStatus::InProgress),
        ],
    );
    app.agent_navigation
        .upsert(ThreadId::from_u128(719), Some("Worker".into()), None, false);
    app.app_server_target = crate::AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr("ws://127.0.0.1:8765").unwrap(),
    };
    app.alab_open_tasks();
    app.chat_widget.handle_paste("step".into());
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let selected = app.chat_widget.alab_task_view_state();
    assert_eq!(selected, Some((Some(1), Some("step".into()))));

    assert!(app.begin_reconnect());
    assert_eq!(app.chat_widget.alab_task_view_state(), selected);
    interface::set_enabled(false);
    app.alab_tasks_connection_changed();
    assert_eq!(app.chat_widget.alab_task_view_state(), selected);
}

#[tokio::test]
async fn queued_detail_back_rejects_a_replacement_thread_or_off_and_direct_open_stays_available()
-> color_eyre::eyre::Result<()> {
    let _scope = scope();
    let (mut app, mut events, mut operations) = make_test_app_with_channels().await;
    let original = ThreadId::from_u128(716);
    let replacement = ThreadId::from_u128(717);
    attach_thread(&mut app, original);
    app.chat_widget
        .apply_external_edit("retained prompt ".into());
    app.chat_widget
        .handle_paste("structured paste ".repeat(200));
    let draft = app.chat_widget.capture_thread_input_state();
    let mut config = TuiKeymap::default();
    config.list.accept = Some(KeybindingsSpec::One(KeybindingSpec("f11".into())));
    let runtime = RuntimeKeymap::from_config(&config).unwrap();
    app.chat_widget.apply_keymap_update(config, &runtime);
    app.keymap = runtime;
    plan(&mut app, &[("Inspect queued action", StepStatus::Pending)]);
    let key = app.alab_tasks()[0].key.clone();
    let mut session = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;

    for disable in [false, true] {
        interface::set_enabled(true);
        attach_thread(&mut app, original);
        app.alab_task_details(Some(original), &key);
        while events.try_recv().is_ok() {}
        while operations.try_recv().is_ok() {}
        app.handle_tui_event(
            &mut tui,
            &mut session,
            crate::tui::TuiEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        )
        .await?;
        assert!(
            app.chat_widget.active_view_is("alab-task-details"),
            "native accept was remapped"
        );
        app.handle_tui_event(
            &mut tui,
            &mut session,
            crate::tui::TuiEvent::Key(KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE)),
        )
        .await?;
        let back = events.try_recv()?;
        assert!(
            matches!(&back, AppEvent::AlabTasksBack { thread_id: Some(id) } if *id == original)
        );
        assert!(!app.chat_widget.has_active_view());
        if disable {
            interface::set_enabled(false);
        } else {
            attach_thread(&mut app, replacement);
        }
        while operations.try_recv().is_ok() {}
        app.handle_event(&mut tui, &mut session, back).await?;
        assert!(!app.chat_widget.has_active_view());
        assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
        assert!(operations.try_recv().is_err());
    }

    interface::set_enabled(true);
    attach_thread(&mut app, replacement);
    app.handle_event(
        &mut tui,
        &mut session,
        AppEvent::AlabTaskDetails {
            thread_id: Some(original),
            key,
        },
    )
    .await?;
    assert!(!app.chat_widget.has_active_view());
    app.handle_event(&mut tui, &mut session, AppEvent::OpenAlabTasks)
        .await?;
    assert!(app.chat_widget.alab_task_view_state().is_some());
    assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
    session.shutdown().await?;
    Ok(())
}
