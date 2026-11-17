// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower identity controls only: no reply/Native/Source Capture is fabricated.
use super::*;

#[test]
fn identical_form_keys_cannot_substitute_a_new_server_request_id() {
    let captured = [RequestId::Integer(7), RequestId::String("next".into())];
    assert!(validate_bound_ids(&captured, captured.iter()).is_ok());
    let new_callback = [RequestId::Integer(8), captured[1].clone()];
    assert!(validate_bound_ids(&captured, new_callback.iter()).is_err());
    let changed_type = [RequestId::String("7".into()), captured[1].clone()];
    assert!(validate_bound_ids(&captured, changed_type.iter()).is_err());
    assert!(validate_bound_ids(&captured, captured[..1].iter()).is_err());
    assert!(validate_bound_ids(&captured[..1], captured.iter()).is_err());
}

#[test]
fn view_identity_wrapper_has_a_closed_schema_and_requires_actual_queue_count() {
    let params = ToolRequestUserInputParams {
        thread_id: "00000000-0000-0000-0000-000000000001".into(),
        turn_id: "turn".into(),
        item_id: "item".into(),
        questions: Vec::new(),
        is_blocking: true,
        auto_resolution_ms: None,
    };
    let view = ViewData::UserInput {
        original: serde_json::value::to_raw_value(
            &serde_json::json!({"request":params,"queue":[]}),
        )
        .unwrap(),
    };
    let value = CapturedView::capture(view, vec![RequestId::Integer(7)], 4096).unwrap();
    let wire = serde_json::to_vec(&value).unwrap();
    let decoded: CapturedView = serde_json::from_slice(&wire).unwrap();
    assert!(decoded.validate_data(4096).is_ok());
    for kind in [
        serde_json::json!({"userInput": null}),
        serde_json::json!(0),
        serde_json::json!("unknown"),
    ] {
        let mut encoded = serde_json::to_value(&value).unwrap();
        encoded["view"]["kind"] = kind;
        assert!(serde_json::from_value::<CapturedView>(encoded).is_err());
    }
    let mut encoded = serde_json::to_value(&value).unwrap();
    encoded["view"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CapturedView>(encoded).is_err());
    let mut encoded = serde_json::to_value(&value).unwrap();
    encoded["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CapturedView>(encoded).is_err());
    let mut encoded = serde_json::to_value(&value).unwrap();
    encoded["requestIds"] = serde_json::json!([]);
    let decoded = serde_json::from_value::<CapturedView>(encoded).unwrap();
    assert!(decoded.validate_data(4096).is_err());
    let mut encoded = serde_json::to_value(&value).unwrap();
    encoded["schema"] = serde_json::json!(2);
    let decoded = serde_json::from_value::<CapturedView>(encoded).unwrap();
    assert!(decoded.validate_data(4096).is_err());
}
