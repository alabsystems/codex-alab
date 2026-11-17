// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real delayed queue/clock controls. Native-only mutation helpers exercise
//! original timing and request order; no Source owner or Native grant is made.
use super::*;
use crate::app_command::AppCommand;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use crossterm::event::KeyModifiers;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::unbounded_channel;

const LIMIT: usize = 1_048_576;
fn request(thread: ThreadId, id: &str) -> ApprovalRequest {
    ApprovalRequest::Exec(ExecApprovalRequest {
        kind: Default::default(),
        thread_id: thread,
        thread_label: None,
        id: id.into(),
        environment_id: None,
        command: vec!["echo".into(), id.into()],
        reason: None,
        available_decisions: vec![
            CommandExecutionApprovalDecision::Accept,
            CommandExecutionApprovalDecision::Cancel,
        ],
        network_approval_context: None,
        additional_permissions: None,
    })
}
fn pane() -> (BottomPane, UnboundedReceiver<AppEvent>) {
    let (tx, rx) = unbounded_channel();
    (
        crate::bottom_pane::tests::test_pane(AppEventSender::new(tx)),
        rx,
    )
}
fn queue(pane: &mut BottomPane, thread: ThreadId, ids: &[&str], at: Instant) {
    // The real original callback path consults this untouched destination
    // clock. Generation stays zero until actual local composer activity.
    pane.last_composer_activity_at = Some(at);
    for id in ids {
        pane.push_approval_request(request(thread, id), &Features::with_defaults());
    }
}
#[test]
fn original_delayed_deadline_restores_remaining_pause_without_restarting_it() -> io::Result<()> {
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    let activity = Instant::now() - Duration::from_millis(300);
    queue(&mut original, thread, &["first"], activity);
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    let saved = PreparedDelayed::decode(&raw, LIMIT)?;
    let (mut actual, _) = pane();
    queue(&mut actual, thread, &["first"], Instant::now());
    saved.validate_destination(&actual)?;
    saved.install_native_clock_at(&mut actual, activity + Duration::from_millis(500))?;
    let restored = actual
        .last_composer_activity_at
        .expect("original activity remains");
    let error = if restored >= activity {
        restored.duration_since(activity)
    } else {
        activity.duration_since(restored)
    };
    assert!(error < Duration::from_millis(2));
    let before = activity + APPROVAL_PROMPT_TYPING_IDLE_DELAY - Duration::from_millis(5);
    actual.pre_draw_tick_at(before);
    assert!(actual.view_stack.is_empty());
    actual
        .pre_draw_tick_at(activity + APPROVAL_PROMPT_TYPING_IDLE_DELAY + Duration::from_millis(5));
    assert_eq!(actual.view_stack.len(), 1);
    assert!(actual.delayed_approval_requests.is_empty());
    Ok(())
}
#[test]
fn expired_original_pause_promotes_same_actual_fifo_and_preserves_new_requests() -> io::Result<()> {
    let thread = ThreadId::new();
    let (mut original, mut old_events) = pane();
    let at = Instant::now();
    queue(&mut original, thread, &["first", "second", "third"], at);
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    let (mut actual, mut events) = pane();
    queue(
        &mut actual,
        thread,
        &["first", "second", "third", "new"],
        Instant::now(),
    );
    let saved = PreparedDelayed::decode(&raw, LIMIT)?;
    saved.validate_destination(&actual)?;
    saved.install_native_clock_at(&mut actual, at + Duration::from_secs(2))?;
    assert_eq!(actual.view_stack.len(), 1);
    assert!(actual.delayed_approval_requests.is_empty());
    let mut decided = Vec::new();
    for _ in 0..4 {
        actual.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        while let Ok(event) = events.try_recv() {
            if let AppEvent::SubmitThreadOp {
                op: AppCommand::ExecApproval { id, decision, .. },
                ..
            } = event
            {
                assert_eq!(decision, CommandExecutionApprovalDecision::Accept);
                decided.push(id);
            }
        }
    }
    assert_eq!(decided, ["first", "second", "third", "new"]);
    assert!(old_events.try_recv().is_err());
    Ok(())
}
#[test]
fn actual_new_typing_refuses_late_timer_restore_even_without_text_change() -> io::Result<()> {
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    queue(&mut original, thread, &["first"], Instant::now());
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    let (mut actual, _) = pane();
    queue(&mut actual, thread, &["first"], Instant::now());
    let newer = Instant::now();
    actual.record_composer_activity_at(newer);
    assert!(
        PreparedDelayed::decode(&raw, LIMIT)?
            .validate_destination(&actual)
            .is_err()
    );
    assert_eq!(actual.last_composer_activity_at, Some(newer));
    assert_eq!(actual.delayed_approval_requests.len(), 1);
    Ok(())
}
#[test]
fn changed_real_queue_order_or_payload_refuses_without_replacing_resources() -> io::Result<()> {
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    queue(&mut original, thread, &["first", "second"], Instant::now());
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    for ids in [["second", "first"], ["first", "changed"]] {
        let (mut actual, _) = pane();
        queue(&mut actual, thread, &ids, Instant::now());
        assert!(
            PreparedDelayed::decode(&raw, LIMIT)?
                .validate_destination(&actual)
                .is_err()
        );
        assert_eq!(actual.delayed_approval_requests.len(), 2);
        assert!(actual.view_stack.is_empty());
    }
    Ok(())
}
#[test]
fn request_data_alone_cannot_supply_the_actual_reply_owners() -> io::Result<()> {
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    queue(&mut original, thread, &["first"], Instant::now());
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    let (mut actual, mut events) = pane();
    let newer = Instant::now();
    queue(&mut actual, thread, &["first"], newer);
    assert!(
        PreparedDelayed::decode(&raw, LIMIT)?
            .install_bound(&mut actual, &[])
            .is_err()
    );
    assert_eq!(actual.last_composer_activity_at, Some(newer));
    assert_eq!(actual.delayed_approval_requests.len(), 1);
    assert!(actual.view_stack.is_empty());
    assert!(events.try_recv().is_err());
    Ok(())
}
#[test]
fn pruned_callback_mask_keeps_clock_and_exact_surviving_fifo() -> io::Result<()> {
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    queue(
        &mut original,
        thread,
        &["first", "second", "third"],
        Instant::now(),
    );
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    assert!(editing_retaining(&raw, &[true], LIMIT).is_err());
    let kept = editing_retaining(&raw, &[true, false, true], LIMIT)?;
    let old = Data::decode(&raw, LIMIT)?;
    let data = Data::decode(&kept, LIMIT)?;
    assert_eq!(
        serde_json::to_value(old.last_activity).map_err(invalid)?,
        serde_json::to_value(data.last_activity).map_err(invalid)?
    );
    let (mut actual, _) = pane();
    queue(
        &mut actual,
        thread,
        &["first", "third", "new"],
        Instant::now(),
    );
    PreparedDelayed::decode(&kept, LIMIT)?.validate_destination(&actual)?;
    assert_eq!(requests_data(&kept, LIMIT)?.len(), 2);
    Ok(())
}
#[test]
fn closed_clock_request_and_optional_fields_reject_projection_or_omission() -> io::Result<()> {
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    queue(&mut original, thread, &["first"], Instant::now());
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    assert!(PreparedDelayed::decode(&raw, 1).is_err());
    for field in ["last_activity", "generation", "requests"] {
        let mut wire: serde_json::Value = serde_json::from_str(raw.get()).map_err(invalid)?;
        wire.as_object_mut().expect("object").remove(field);
        let changed = serde_json::value::to_raw_value(&wire).map_err(invalid)?;
        assert!(PreparedDelayed::decode(&changed, LIMIT).is_err());
    }
    let mut wire: serde_json::Value = serde_json::from_str(raw.get()).map_err(invalid)?;
    wire["requests"][0]["foreign"] = serde_json::json!(true);
    let changed = serde_json::value::to_raw_value(&wire).map_err(invalid)?;
    assert!(PreparedDelayed::decode(&changed, LIMIT).is_err());
    let mut wire: serde_json::Value = serde_json::from_str(raw.get()).map_err(invalid)?;
    wire["last_activity"]["nanos"] = serde_json::json!(1_000_000_000u32);
    let changed = serde_json::value::to_raw_value(&wire).map_err(invalid)?;
    assert!(PreparedDelayed::decode(&changed, LIMIT).is_err());
    Ok(())
}
#[test]
fn real_activity_generation_exhaustion_never_wraps_into_an_untouched_destination() -> io::Result<()>
{
    let (mut actual, _) = pane();
    actual.alab_delay_activity.state()?.value = Some(u64::MAX);
    actual.record_composer_activity_at(Instant::now());
    assert!(actual.alab_delay_activity.value().is_err());
    assert!(actual.alab_capture_delayed_approvals(LIMIT).is_err());
    actual.record_composer_activity_at(Instant::now());
    assert!(actual.alab_delay_activity.value().is_err());
    Ok(())
}
#[test]
fn actual_constructor_primes_before_original_replay_and_newer_typing_keeps_its_deadline()
-> io::Result<()> {
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    queue(
        &mut original,
        thread,
        &["first"],
        Instant::now() - Duration::from_millis(300),
    );
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    let saved = PreparedDelayed::decode(&raw, LIMIT)?;
    let (mut actual, _) = pane();
    saved.prime_before_original_replay(&mut actual)?;
    let new_activity = Instant::now();
    actual.record_composer_activity_at(new_activity);
    actual.push_approval_request(request(thread, "first"), &Features::with_defaults());
    saved.prime_before_original_replay(&mut actual)?;
    saved.validate_destination(&actual)?;
    saved.install_native_clock_at(&mut actual, new_activity + Duration::from_millis(500))?;
    assert_eq!(actual.last_composer_activity_at, Some(new_activity));
    assert!(actual.view_stack.is_empty());
    actual.pre_draw_tick_at(new_activity + APPROVAL_PROMPT_TYPING_IDLE_DELAY);
    assert_eq!(actual.view_stack.len(), 1);
    Ok(())
}
#[test]
fn clock_priming_after_actual_input_or_after_real_replay_refuses_without_mutation() -> io::Result<()>
{
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    queue(&mut original, thread, &["first"], Instant::now());
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    let (mut actual, _) = pane();
    let activity = Instant::now();
    actual.record_composer_activity_at(activity);
    assert!(
        PreparedDelayed::decode(&raw, LIMIT)?
            .prime_before_original_replay(&mut actual)
            .is_err()
    );
    assert_eq!(actual.last_composer_activity_at, Some(activity));
    let (mut actual, _) = pane();
    actual.push_approval_request(request(thread, "first"), &Features::with_defaults());
    assert!(
        PreparedDelayed::decode(&raw, LIMIT)?
            .prime_before_original_replay(&mut actual)
            .is_err()
    );
    assert!(actual.last_composer_activity_at.is_none());
    assert_eq!(actual.view_stack.len(), 1);
    Ok(())
}
#[test]
fn distinct_late_clock_cannot_replace_a_clock_already_applied_to_the_actual_pane() -> io::Result<()>
{
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    queue(&mut original, thread, &["first"], Instant::now());
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    let (mut actual, _) = pane();
    PreparedDelayed::decode(&raw, LIMIT)?.prime_before_original_replay(&mut actual)?;
    let first = actual.last_composer_activity_at;
    let mut wire: serde_json::Value = serde_json::from_str(raw.get()).map_err(invalid)?;
    wire["generation"] = serde_json::json!(7);
    let changed = serde_json::value::to_raw_value(&wire).map_err(invalid)?;
    assert!(
        PreparedDelayed::decode(&changed, LIMIT)?
            .prime_before_original_replay(&mut actual)
            .is_err()
    );
    assert_eq!(actual.last_composer_activity_at, first);
    assert_eq!(actual.alab_delay_activity.value()?, 0);
    Ok(())
}

#[test]
fn original_pump_clock_handoff_moves_new_input_and_marker_once_into_real_widget() -> io::Result<()>
{
    let thread = ThreadId::new();
    let (mut original, _) = pane();
    queue(&mut original, thread, &["first"], Instant::now());
    let raw = original.alab_capture_delayed_approvals(LIMIT)?;
    let prepared = PreparedDelayed::decode(&raw, LIMIT)?;
    let (mut pump, _) = pane();
    prepared.prime_before_original_replay(&mut pump)?;
    let newer = Instant::now();
    pump.record_composer_activity_at(newer);
    let (mut widget, _) = pane();
    widget.alab_transfer_delayed_clock_from(&mut pump)?;
    assert_eq!(widget.last_composer_activity_at, Some(newer));
    assert_eq!(widget.alab_delay_activity.value()?, 1);
    prepared.prime_before_original_replay(&mut widget)?;
    assert_eq!(widget.last_composer_activity_at, Some(newer));
    assert!(pump.alab_capture_delayed_approvals(LIMIT).is_err());
    let (mut another, _) = pane();
    assert!(another.alab_transfer_delayed_clock_from(&mut pump).is_err());
    assert!(another.last_composer_activity_at.is_none());
    Ok(())
}

#[test]
fn destination_refusal_keeps_original_clock_available_for_its_real_handoff() -> io::Result<()> {
    let (mut pump, _) = pane();
    let original = Instant::now();
    pump.record_composer_activity_at(original);
    let (mut changed_widget, _) = pane();
    changed_widget.record_composer_activity_at(Instant::now());
    assert!(
        changed_widget
            .alab_transfer_delayed_clock_from(&mut pump)
            .is_err()
    );
    assert_eq!(pump.last_composer_activity_at, Some(original));
    assert_eq!(pump.alab_delay_activity.value()?, 1);
    let (mut widget, _) = pane();
    widget.alab_transfer_delayed_clock_from(&mut pump)?;
    assert_eq!(widget.last_composer_activity_at, Some(original));
    assert_eq!(widget.alab_delay_activity.value()?, 1);
    Ok(())
}

#[test]
fn clock_handoff_never_strands_original_queued_callback_resources() -> io::Result<()> {
    let thread = ThreadId::new();
    let (mut pump, _) = pane();
    let at = Instant::now();
    queue(&mut pump, thread, &["first"], at);
    let (mut widget, _) = pane();
    assert!(widget.alab_transfer_delayed_clock_from(&mut pump).is_err());
    assert_eq!(pump.last_composer_activity_at, Some(at));
    assert_eq!(pump.delayed_approval_requests.len(), 1);
    assert!(widget.last_composer_activity_at.is_none());
    assert!(widget.delayed_approval_requests.is_empty());
    Ok(())
}

#[test]
fn real_pump_input_after_widget_construction_delays_actual_replayed_callback() -> io::Result<()> {
    let thread = ThreadId::new();
    let (mut pump, _) = pane();
    let empty_clock = pump.alab_capture_delayed_approvals(LIMIT)?;
    PreparedDelayed::decode(&empty_clock, LIMIT)?.prime_before_original_replay(&mut pump)?;
    let (mut widget, _) = pane();
    widget.alab_share_delayed_clock_from(&mut pump)?;
    let late_typing = Instant::now();
    pump.record_composer_activity_at(late_typing);
    widget.push_approval_request(request(thread, "first"), &Features::with_defaults());
    assert!(widget.view_stack.is_empty());
    assert_eq!(widget.delayed_approval_requests.len(), 1);
    assert_eq!(
        widget.composer_draft_snapshot().last_composer_activity_at,
        Some(late_typing)
    );
    widget.pre_draw_tick_at(late_typing + APPROVAL_PROMPT_TYPING_IDLE_DELAY);
    assert_eq!(widget.view_stack.len(), 1);
    widget.alab_transfer_delayed_clock_from(&mut pump)?;
    assert!(pump.alab_delay_activity.value().is_err());
    assert_eq!(widget.alab_delay_activity.value()?, 1);
    Ok(())
}

#[test]
fn repeated_share_keeps_same_actual_clock_and_activity_from_either_original_pane() -> io::Result<()>
{
    let (mut pump, _) = pane();
    let (mut widget, _) = pane();
    widget.alab_share_delayed_clock_from(&mut pump)?;
    let first = Instant::now();
    widget.record_composer_activity_at(first);
    widget.alab_share_delayed_clock_from(&mut pump)?;
    assert_eq!(
        pump.composer_draft_snapshot().last_composer_activity_at,
        Some(first)
    );
    let second = first + Duration::from_millis(10);
    pump.record_composer_activity_at(second);
    assert_eq!(
        widget.composer_draft_snapshot().last_composer_activity_at,
        Some(second)
    );
    assert_eq!(widget.alab_delay_activity.value()?, 2);
    let (mut foreign, _) = pane();
    assert!(widget.alab_share_delayed_clock_from(&mut foreign).is_err());
    assert_eq!(
        widget.composer_draft_snapshot().last_composer_activity_at,
        Some(second)
    );
    assert_eq!(foreign.alab_delay_activity.value()?, 0);
    Ok(())
}

#[test]
fn actual_shared_clock_remains_after_ordinary_pump_drop() -> io::Result<()> {
    let (mut widget, _) = pane();
    let last = Instant::now();
    {
        let (mut pump, _) = pane();
        widget.alab_share_delayed_clock_from(&mut pump)?;
        pump.record_composer_activity_at(last);
    }
    assert_eq!(
        widget.composer_draft_snapshot().last_composer_activity_at,
        Some(last)
    );
    assert_eq!(widget.alab_delay_activity.value()?, 1);
    Ok(())
}
