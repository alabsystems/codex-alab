// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Drive the original native mouse handler on both sides of the data round trip.
use super::*;

fn event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::NONE,
    }
}
fn reading() -> TranscriptView {
    let mut view = TranscriptView {
        position: Position::Reading(Anchor {
            key: EntryKey::Live,
            index: 0,
            offset: 3,
            row_bias: -1,
        }),
        ..TranscriptView::default()
    };
    view.follow_control.area = Some(Rect::new(4, 6, 12, 1));
    view
}
fn roundtrip(view: &TranscriptView) -> io::Result<TranscriptView> {
    let raw = serde_json::value::to_raw_value(&Data::capture(view)).map_err(invalid)?;
    let data = Data::decode(&raw, 4096)?;
    // The complete view codec restores position independently of this leaf.
    let mut restored = TranscriptView {
        position: view.position,
        ..TranscriptView::default()
    };
    data.restore(&mut restored);
    Ok(restored)
}
#[test]
fn original_pointer_down_then_restored_pointer_up_follows_once() -> io::Result<()> {
    let mut original = reading();
    original.handle_follow_control_mouse(event(MouseEventKind::Down(MouseButton::Left), 7, 6));
    let mut restored = roundtrip(&original)?;
    assert_eq!(
        restored.follow_control.pointer,
        Some(ScreenPosition::new(7, 6))
    );
    assert!(restored.follow_control.pressed);
    restored.handle_follow_control_mouse(event(MouseEventKind::Up(MouseButton::Left), 7, 6));
    assert!(matches!(restored.position, Position::Latest));
    assert!(!restored.follow_control.pressed);
    Ok(())
}
#[test]
fn restored_original_drag_outside_does_not_jump_to_latest() -> io::Result<()> {
    let mut original = reading();
    original.handle_follow_control_mouse(event(MouseEventKind::Down(MouseButton::Left), 7, 6));
    original.handle_follow_control_mouse(event(MouseEventKind::Drag(MouseButton::Left), 2, 9));
    let mut restored = roundtrip(&original)?;
    restored.handle_follow_control_mouse(event(MouseEventKind::Up(MouseButton::Left), 2, 9));
    assert!(matches!(restored.position, Position::Reading(_)));
    assert!(!restored.follow_control.pressed);
    Ok(())
}
#[test]
fn original_hidden_control_cannot_intercept_after_roundtrip() -> io::Result<()> {
    let original = TranscriptView::default();
    let mut restored = roundtrip(&original)?;
    assert!(
        restored
            .handle_follow_control_mouse(event(MouseEventKind::Down(MouseButton::Left), 0, 0))
            .is_none()
    );
    assert!(!restored.follow_control.pressed);
    Ok(())
}
#[test]
fn pointer_data_requires_explicit_optional_fields_and_closed_schema() -> io::Result<()> {
    let value = serde_json::to_value(Data::capture(&TranscriptView::default())).map_err(invalid)?;
    let decode = |v: serde_json::Value| -> io::Result<Data> {
        Data::decode(&serde_json::value::to_raw_value(&v).map_err(invalid)?, 4096)
    };
    assert!(decode(value.clone()).is_ok());
    for key in ["area", "pointer", "pressed", "schema"] {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(decode(bad).is_err(), "{key}");
    }
    let mut bad = value.clone();
    bad["foreign"] = serde_json::Value::Null;
    assert!(decode(bad).is_err());
    let mut bad = value.clone();
    bad["schema"] = 2.into();
    assert!(decode(bad).is_err());
    let mut bad = value;
    bad["area"] = serde_json::json!([0, 0, 65536, 1]);
    assert!(decode(bad).is_err());
    Ok(())
}

#[test]
fn absent_native_target_or_pointer_cannot_create_a_pressed_gesture() -> io::Result<()> {
    for (area, pointer, pressed) in [
        (None, None, true),
        (None, Some([7, 6]), false),
        (None, Some([7, 6]), true),
        (Some([4, 6, 12, 1]), None, true),
    ] {
        let raw = serde_json::value::to_raw_value(
            &serde_json::json!({"schema":1,"area":area,"pointer":pointer,"pressed":pressed}),
        )
        .map_err(invalid)?;
        assert!(Data::decode(&raw, 4096).is_err());
    }
    // A genuine pressed drag may have left its visible target.
    let raw = serde_json::value::to_raw_value(
        &serde_json::json!({"schema":1,"area":[4,6,12,1],"pointer":[2,9],"pressed":true}),
    )
    .map_err(invalid)?;
    assert!(Data::decode(&raw, 4096).is_ok());
    Ok(())
}
