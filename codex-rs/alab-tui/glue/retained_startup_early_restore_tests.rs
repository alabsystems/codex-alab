// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original StartupDraftPump/BottomPane controls; no App, Native
//! controller, accepted Source operation, UI ACK or model grant is fabricated.
use super::*;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

const BYTES: usize = 1024 * 1024;
const GENERATIONS: usize = 1024;

#[test]
fn real_provisional_editor_confirmation_and_paste_state_restore_before_events() {
    let mut source = crate::startup_draft::tests::quiet_startup_test_pump();
    source.bottom_pane.set_composer_text(
        "original\nprovisional draft".into(),
        Vec::new(),
        Vec::new(),
    );
    source
        .bottom_pane
        .handle_paste("whole original paste\n".repeat(400));
    let _input = source
        .bottom_pane
        .handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    source.submission_pending = true;
    source.pending_paste_newline = Some((Instant::now(), "original pending newline".into()));
    source.initial_screen = StartupDraftInitialScreen::SessionPicker;
    source.session_action = StartupDraftSessionAction::Fork;
    let text = source.bottom_pane.composer_text();
    let cursor = source.bottom_pane.composer_cursor();
    let pastes = source.bottom_pane.composer_pending_pastes();
    assert!(!pastes.is_empty());
    let raw = serde_json::value::to_raw_value(&capture_ui(&source, None, BYTES).unwrap()).unwrap();
    let prepared = Prepared::decode(&raw, BYTES, GENERATIONS).unwrap();
    let mut destination = crate::startup_draft::tests::quiet_startup_test_pump();
    let _installed = prepared.install(&mut destination).unwrap();
    assert_eq!(destination.bottom_pane.composer_text(), text);
    assert_eq!(destination.bottom_pane.composer_cursor(), cursor);
    assert_eq!(destination.bottom_pane.composer_pending_pastes(), pastes);
    assert!(destination.submission_pending);
    assert_eq!(
        destination.pending_paste_newline.as_ref().unwrap().1,
        "original pending newline"
    );
    assert!(matches!(
        destination.initial_screen,
        StartupDraftInitialScreen::SessionPicker
    ));
    assert!(matches!(
        destination.session_action,
        StartupDraftSessionAction::Fork
    ));
    // Original sender/event stream remains owned and is not replaced by DATA.
    assert!(destination.app_event_rx.is_empty());
    assert_eq!(source.bottom_pane.composer_text(), text);
}

#[test]
fn real_new_human_draft_survives_refused_late_install() {
    let source = crate::startup_draft::tests::quiet_startup_test_pump();
    let raw = serde_json::value::to_raw_value(&capture_ui(&source, None, BYTES).unwrap()).unwrap();
    let mut destination = crate::startup_draft::tests::quiet_startup_test_pump();
    destination.bottom_pane.set_composer_text(
        "new real human input".into(),
        Vec::new(),
        Vec::new(),
    );
    let prepared = Prepared::decode(&raw, BYTES, GENERATIONS).unwrap();
    assert!(prepared.install(&mut destination).is_err());
    assert_eq!(
        destination.bottom_pane.composer_text(),
        "new real human input"
    );
}

#[test]
fn original_choice_cancellation_compares_saved_thread_and_cwd_without_backend_invention() {
    let first = codex_protocol::ThreadId::new();
    let second = codex_protocol::ThreadId::new();
    let current = SessionSelection::Resume(crate::resume_picker::SessionTarget {
        backend_id: crate::backend_id::BackendId::new(),
        path: None,
        thread_id: second,
        cwd: None,
        history_mode: None,
    });
    let mut installed = Installed {
        expected: Some(SelectionData::Resume {
            target: TargetData {
                path: None,
                thread_id: first,
                cwd: None,
                history_mode: None,
            },
        }),
    };
    assert!(installed.original_destination_changed(&current));
    assert!(!installed.original_destination_changed(&current));
}

#[test]
fn real_early_install_primes_same_pump_clock_before_subsequent_human_input() -> io::Result<()> {
    let mut source = crate::startup_draft::tests::quiet_startup_test_pump();
    source
        .bottom_pane
        .set_composer_text("original native draft".into(), Vec::new(), Vec::new());
    let original_activity = std::time::Instant::now() - std::time::Duration::from_millis(300);
    source
        .bottom_pane
        .record_composer_activity_at(original_activity);
    let raw = serde_json::value::to_raw_value(&capture_ui(&source, None, BYTES)?).unwrap();
    let clock = source.bottom_pane.alab_capture_delayed_approvals(BYTES)?;
    let mut actual = crate::startup_draft::tests::quiet_startup_test_pump();
    Prepared::decode(&raw, BYTES, GENERATIONS)?.install_with_clock(
        &mut actual,
        crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed::decode(&clock, BYTES)?,
    )?;
    let before = actual
        .bottom_pane
        .composer_draft_snapshot()
        .last_composer_activity_at
        .unwrap();
    let error = if before >= original_activity {
        before.duration_since(original_activity)
    } else {
        original_activity.duration_since(before)
    };
    assert!(error < std::time::Duration::from_millis(2));
    actual
        .bottom_pane
        .handle_key_event(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    let new_activity = actual
        .bottom_pane
        .composer_draft_snapshot()
        .last_composer_activity_at
        .unwrap();
    assert!(new_activity > before);
    crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed::decode(&clock, BYTES)?
        .prime_before_original_replay(&mut actual.bottom_pane)?;
    assert_eq!(
        actual
            .bottom_pane
            .composer_draft_snapshot()
            .last_composer_activity_at,
        Some(new_activity)
    );
    // Original ASCII input may remain in the paste-burst first-char buffer.
    // Flush the SAME accepted input after the repeated-prime clock check.
    actual.bottom_pane.flush_composer_paste_burst();
    assert!(actual.bottom_pane.composer_text().contains('x'));
    Ok(())
}
