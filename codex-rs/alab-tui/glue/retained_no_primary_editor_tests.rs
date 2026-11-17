// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::app_event_sender::AppEventSender;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

const BUDGET: usize = 1024 * 1024;
fn pane() -> BottomPane {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut pane = crate::bottom_pane::tests::test_pane(AppEventSender::new(tx));
    pane.composer.set_disable_paste_burst(true);
    pane.composer.set_vim_enabled(true);
    pane
}
fn fill(pane: &mut BottomPane, text: &str) {
    pane.composer
        .set_text_content(format!("{text} "), Vec::new(), Vec::new());
    pane.composer.move_cursor_to_end();
    pane.composer
        .insert_selected_mention(0..0, "$skill", Some("/original/skill/SKILL.md"));
    pane.composer.attach_image(PathBuf::from("draft.png"));
    pane.composer
        .handle_paste("actual multi-line draft\n".repeat(400));
    pane.composer.popups.active = ActivePopup::None;
    pane.composer
        .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    pane.composer
        .handle_key_event(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE));
}
fn snapshot(pane: &BottomPane) -> serde_json::Value {
    serde_json::to_value(pane.composer.snapshot_draft()).unwrap()
}
#[test]
fn provisional_rich_draft_and_real_stash_round_trip_without_primary_identity() {
    let mut source = pane();
    fill(&mut source, "stashed");
    let saved = source.composer.alab_take_prompt(&source.keymap);
    fill(&mut source, "visible");
    let before = snapshot(&source);
    let slots = HashMap::from([(ThreadId::new(), saved)]);
    let bytes = capture_no_primary(&source, &slots, BUDGET).unwrap();
    assert_eq!(snapshot(&source), before);
    let prepared =
        BottomPane::alab_prepare_retained_no_primary_checkpoint(&bytes, BUDGET, 1024).unwrap();
    let mut destination = pane();
    let mut restored = HashMap::new();
    prepared
        .install_into(&mut destination, &mut restored)
        .unwrap();
    assert_eq!(snapshot(&destination), before);
    assert_eq!(restored.len(), 1);
    assert!(destination.thread_id.is_none());
}
#[test]
fn primary_and_foreign_payloads_are_refused_before_editor_mutation() {
    let mut source = pane();
    fill(&mut source, "visible");
    let bytes = capture_no_primary(&source, &HashMap::new(), BUDGET).unwrap();
    let mut destination = pane();
    destination.set_history_metadata(ThreadId::new(), 0, 0);
    let before = snapshot(&destination);
    let mut empty = HashMap::new();
    assert!(
        BottomPane::alab_prepare_retained_no_primary_checkpoint(&bytes, BUDGET, 1024)
            .unwrap()
            .install_into(&mut destination, &mut empty)
            .is_err()
    );
    assert_eq!(snapshot(&destination), before);
    assert!(capture_no_primary(&destination, &HashMap::new(), BUDGET).is_err());
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("primary".into(), serde_json::json!(ThreadId::new()));
    assert!(
        BottomPane::alab_prepare_retained_no_primary_checkpoint(
            &serde_json::to_vec(&value).unwrap(),
            BUDGET,
            1024
        )
        .is_err()
    );
}
#[test]
fn occupied_stash_and_size_bounds_do_not_replace_actual_editor_owners() {
    let mut source = pane();
    fill(&mut source, "visible");
    assert!(capture_no_primary(&source, &HashMap::new(), 1).is_err());
    let bytes = capture_no_primary(&source, &HashMap::new(), BUDGET).unwrap();
    let mut destination = pane();
    fill(&mut destination, "saved");
    let saved = destination.composer.alab_take_prompt(&destination.keymap);
    fill(&mut destination, "destination");
    let before = snapshot(&destination);
    let mut slots = HashMap::from([(ThreadId::new(), saved)]);
    assert!(
        BottomPane::alab_prepare_retained_no_primary_checkpoint(&bytes, BUDGET, 1024)
            .unwrap()
            .install_into(&mut destination, &mut slots)
            .is_err()
    );
    assert_eq!(snapshot(&destination), before);
    assert_eq!(slots.len(), 1);
}

#[test]
fn late_native_editor_move_preserves_register_pending_command_and_real_approval() -> io::Result<()>
{
    use crate::bottom_pane::ApprovalRequest;
    use crate::bottom_pane::ExecApprovalRequest;
    use codex_app_server_protocol::CommandExecutionApprovalDecision;
    let mut source = pane();
    fill(&mut source, "original rich owner");
    for ch in ['y', '$', 'd'] {
        source
            .composer
            .handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    assert!(source.composer.draft.textarea.is_vim_operator_pending());
    let original = snapshot(&source);
    let mut destination = pane();
    destination.alab_share_delayed_clock_from(&mut source)?;
    // Genuine late pump input changes the shared pause, never the widget's
    // independent local input generation.
    source.record_composer_activity_at(std::time::Instant::now());
    let thread = ThreadId::new();
    destination.last_composer_activity_at = None;
    destination.push_approval_request(
        ApprovalRequest::Exec(ExecApprovalRequest {
            kind: Default::default(),
            thread_id: thread,
            thread_label: None,
            id: "same-original-approval".into(),
            environment_id: None,
            command: vec!["original-command".into()],
            reason: None,
            available_decisions: vec![
                CommandExecutionApprovalDecision::Accept,
                CommandExecutionApprovalDecision::Cancel,
            ],
            network_approval_context: None,
            additional_permissions: None,
        }),
        &codex_features::Features::with_defaults(),
    );
    // Shared typing time can defer this real row; force only the original
    // native deadline forward, not a decoded/modal success marker.
    destination.pre_draw_tick_at(std::time::Instant::now() + std::time::Duration::from_secs(2));
    assert_eq!(destination.view_stack.len(), 1);
    let approval = &*destination.view_stack[0] as *const dyn crate::bottom_pane::BottomPaneView;
    destination.alab_handoff_retained_no_primary_editor(&mut source)?;
    assert_eq!(snapshot(&destination), original);
    assert!(
        destination
            .composer
            .draft
            .textarea
            .is_vim_operator_pending()
    );
    assert!(std::ptr::eq(approval, &*destination.view_stack[0]));
    assert!(source.composer.is_empty());
    // Finish the SAME native pending operator, then paste the SAME register.
    destination
        .composer
        .handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
    assert!(
        !destination
            .composer
            .draft
            .textarea
            .is_vim_operator_pending()
    );
    destination
        .composer
        .handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
    assert!(destination.composer.attachments.local_images.len() >= 1);
    Ok(())
}

#[test]
fn independent_blank_widget_edit_refuses_without_taking_either_native_editor() -> io::Result<()> {
    let mut source = pane();
    fill(&mut source, "late original draft");
    let before = snapshot(&source);
    let mut destination = pane();
    destination.alab_share_delayed_clock_from(&mut source)?;
    destination.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(destination.composer.is_empty());
    assert!(destination.alab_delay_activity.local_value()? > 0);
    assert!(
        destination
            .alab_handoff_retained_no_primary_editor(&mut source)
            .is_err()
    );
    assert_eq!(snapshot(&source), before);
    assert!(destination.composer.is_empty());
    // Refusal happened before final clock retirement too.
    destination.alab_share_delayed_clock_from(&mut source)?;
    Ok(())
}

#[test]
fn native_move_keeps_current_destination_input_policy_and_thread_metadata() -> io::Result<()> {
    let mut source = pane();
    fill(&mut source, "original draft");
    source.composer.draft.input_enabled = true;
    let mut destination = pane();
    let thread = ThreadId::new();
    destination.set_history_metadata(thread, 17, 23);
    destination.composer.draft.input_enabled = false;
    destination.composer.draft.input_disabled_placeholder =
        Some("actual destination policy".into());
    destination.alab_share_delayed_clock_from(&mut source)?;
    destination.alab_handoff_retained_no_primary_editor(&mut source)?;
    assert!(!destination.composer.draft.input_enabled);
    assert_eq!(
        destination
            .composer
            .draft
            .input_disabled_placeholder
            .as_deref(),
        Some("actual destination policy")
    );
    assert_eq!(destination.thread_id, Some(thread));
    Ok(())
}

#[test]
fn actual_startup_recall_is_required_and_survives_rich_decode_then_native_handoff() -> io::Result<()>
{
    use crate::bottom_pane::chat_composer_history::HistoryEntry;
    let mut original = pane();
    let recalled = HistoryEntry::new("actual startup recall entry".into());
    original
        .composer
        .history
        .record_local_submission(recalled.clone());
    fill(&mut original, "current original draft");
    let bytes = capture_no_primary(&original, &HashMap::new(), BUDGET)?;
    let mut missing: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    missing["body"]
        .as_object_mut()
        .unwrap()
        .remove("local_history");
    assert!(
        BottomPane::alab_prepare_retained_no_primary_checkpoint(
            &serde_json::to_vec(&missing).unwrap(),
            BUDGET,
            1024
        )
        .is_err()
    );
    let mut pump = pane();
    BottomPane::alab_prepare_retained_no_primary_checkpoint(&bytes, BUDGET, 1024)?
        .install_into(&mut pump, &mut HashMap::new())?;
    assert_eq!(
        pump.composer.history.startup_local_history(),
        std::slice::from_ref(&recalled)
    );
    let mut destination = pane();
    destination.alab_share_delayed_clock_from(&mut pump)?;
    destination.alab_handoff_retained_no_primary_editor(&mut pump)?;
    assert_eq!(
        destination.composer.history.startup_local_history(),
        std::slice::from_ref(&recalled)
    );
    Ok(())
}
