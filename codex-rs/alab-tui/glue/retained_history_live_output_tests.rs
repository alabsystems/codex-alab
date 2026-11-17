// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
#[test]
fn malformed_live_parser_counter_or_pending_cr_cannot_become_native_state() -> io::Result<()> {
    let mut original = LiveCommandOutput::default();
    original.push_str("actual partial\r");
    let mut wrong_count = Data::capture(&original);
    wrong_count.completed_lines += 1;
    assert!(wrong_count.restore().is_err());
    let mut wrong_pending = Data::capture(&original);
    wrong_pending.pending_carriage_return = true;
    assert!(wrong_pending.restore().is_err());
    let mut truncated = LiveCommandOutput::default();
    truncated.push_str(&"x".repeat(LIVE_COMMAND_OUTPUT_MAX_BYTES + 1));
    let mut wrong_head = Data::capture(&truncated);
    wrong_head.head.push("not a completed original line".into());
    assert!(wrong_head.restore().is_err());
    Ok(())
}

#[test]
fn real_truncated_parser_cannot_restore_phantom_partial_or_embedded_line_terminators()
-> io::Result<()> {
    let mut original = LiveCommandOutput::default();
    original.push_str(&"x".repeat(LIVE_COMMAND_OUTPUT_MAX_BYTES + 1));
    // Leave a real short partial so adding LF stays below both original byte bounds.
    original.push_str("\nsmall");
    let mut phantom = Data::capture(&original);
    phantom.current.head.clear();
    phantom.current.tail.clear();
    phantom.current.omitted_bytes = 0;
    assert!(phantom.restore().is_err());
    let mut bad_current_head = Data::capture(&original);
    bad_current_head.current.head.push('\n');
    assert!(bad_current_head.restore().is_err());
    let mut bad_current_tail = Data::capture(&original);
    bad_current_tail.current.tail.push('\n');
    assert!(bad_current_tail.restore().is_err());
    original.push_str("\n");
    let mut bad_completed = Data::capture(&original);
    bad_completed
        .head
        .first_mut()
        .ok_or_else(|| invalid("actual completed parser line missing"))?
        .push('\n');
    assert!(bad_completed.restore().is_err());
    Ok(())
}
