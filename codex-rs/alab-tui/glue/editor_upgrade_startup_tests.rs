// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::tui::TuiEvent;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use futures::StreamExt;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[tokio::test(flavor = "current_thread")]
async fn runtime_startup_leaves_keys_and_paste_deadlines_for_the_restored_editor() {
    let scope = codex_alab_tui::upgrade_editor_startup::TestScope::deferred();
    let mut tui = crate::tui::test_support::make_test_tui().unwrap();
    let mut pump = StartupDraftPump::new(
        &tui,
        StartupDraftInitialScreen::Composer,
        StartupDraftSessionAction::Resume,
    );
    let polls = Arc::new(AtomicUsize::new(0));
    let counted = polls.clone();
    pump.events = Box::pin(
        futures::stream::iter([TuiEvent::Key(KeyCode::Char('x').into())]).inspect(move |_| {
            counted.fetch_add(1, Ordering::Relaxed);
        }),
    );
    let deadline = tokio::time::Instant::now();
    pump.pending_paste_newline = Some((deadline, "\n".into()));
    assert_eq!(
        pump.run_until(&mut tui, std::future::ready(7))
            .await
            .unwrap(),
        7
    );
    pump.flush_pending_events(&mut tui).await.unwrap();
    pump.flush_pending_paste_newline(&mut tui).await.unwrap();
    pump.show(&mut tui).unwrap();
    assert_eq!(polls.load(Ordering::Relaxed), 0);
    assert_eq!(pump.pending_paste_newline, Some((deadline, "\n".into())));
    assert!(pump.bottom_pane.composer_is_empty());
    assert!(tui.terminal.viewport_area.is_empty());
    assert!(matches!(
        pump.events.next().await,
        Some(TuiEvent::Key(KeyEvent {
            code: KeyCode::Char('x'),
            ..
        }))
    ));
    assert!(pump.alab_into_startup_draft().is_none());
    scope.release();
    let mut ordinary = StartupDraftPump::new(
        &tui,
        StartupDraftInitialScreen::Composer,
        StartupDraftSessionAction::Resume,
    );
    ordinary
        .bottom_pane
        .set_composer_text("ordinary draft".into(), Vec::new(), Vec::new());
    assert_eq!(
        ordinary.alab_into_startup_draft().unwrap().text,
        "ordinary draft"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_startup_discard_refuses_before_broker_reset_or_terminal_io() {
    let scope = codex_alab_tui::upgrade_editor_startup::TestScope::deferred();
    let mut tui = crate::tui::test_support::make_test_tui().unwrap();
    let resumed = tui.event_broker.resume_events_rx();
    assert!(!resumed.has_changed().unwrap());
    assert_eq!(
        tui.discard_pending_input_before_interactive_screen()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert!(
        !resumed.has_changed().unwrap(),
        "discard must not pause/resume the broker"
    );
    assert_eq!(
        crate::tui::discard_pending_terminal_input()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        codex_alab_tui::input::discard_buffered_input()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        codex_alab_tui::input::discard_pending(|| panic!("deferred discard reached the OS queue"))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    scope.release();
    codex_alab_tui::upgrade_editor_startup::check_input_discard().unwrap();
}
