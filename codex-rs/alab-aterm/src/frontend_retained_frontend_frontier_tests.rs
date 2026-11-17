// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Closed-codec negatives only. No test constructs a controller, public
//! frontier/UI receipt, Native admission, Go or a positive SDK grant.
use super::*;
use serde_json::Value;
use serde_json::json;

fn envelope(replacement: bool) -> Value {
    let mut value = json!({
        "schema":1,
        "request":"11111111111111111111111111111111",
        "operation":"22222222222222222222222222222222",
        "member":"33333333333333333333333333333333",
        "session":{"sid":"test-session","process_pid":1,"process_birth":"codec-fixture","epoch":"test-epoch"},
        "service":"44444444444444444444444444444444",
        "state":if replacement { "frontier" } else { "initial-frontier" },
        "frontier":"55555555555555555555555555555555"
    });
    if replacement {
        value["go"] = json!("66666666666666666666666666666666");
    }
    // Private syntax baseline makes rejection controls non-vacuous; no
    // public receipt, Controller or SDK observation is constructed here.
    assert!(Response::decode(&value.to_string(), replacement).is_ok());
    value
}

#[test]
fn initial_closed_envelope_rejects_go_even_null_and_other_role_fields() {
    for (key, value) in [
        ("go", Value::Null),
        ("go", json!("66666666666666666666666666666666")),
        ("human", json!(1)),
        ("app_owner", json!("77777777777777777777777777777777")),
        ("attachment", json!("88888888888888888888888888888888")),
    ] {
        let mut frame = envelope(false);
        frame[key] = value;
        assert!(Response::decode(&frame.to_string(), false).is_err());
    }
}

#[test]
fn replacement_closed_envelope_requires_string_go_and_rejects_other_role_fields() {
    for value in [Value::Null, json!(1), json!(false), json!([]), json!({})] {
        let mut frame = envelope(true);
        frame["go"] = value;
        assert!(Response::decode(&frame.to_string(), true).is_err());
    }
    let mut missing = envelope(true);
    missing
        .as_object_mut()
        .unwrap_or_else(|| panic!("fixture object"))
        .remove("go");
    assert!(Response::decode(&missing.to_string(), true).is_err());
    for key in ["human", "app_owner", "attachment"] {
        let mut frame = envelope(true);
        frame[key] = json!("77777777777777777777777777777777");
        assert!(Response::decode(&frame.to_string(), true).is_err());
    }
}

#[test]
fn both_closed_envelopes_require_every_common_field_and_reject_duplicates() {
    for replacement in [false, true] {
        for key in [
            "schema",
            "request",
            "operation",
            "member",
            "session",
            "service",
            "state",
            "frontier",
        ] {
            let mut frame = envelope(replacement);
            frame
                .as_object_mut()
                .unwrap_or_else(|| panic!("fixture object"))
                .remove(key);
            assert!(Response::decode(&frame.to_string(), replacement).is_err());
        }
        let duplicate = envelope(replacement)
            .to_string()
            .replace("\"schema\":1", "\"schema\":1,\"schema\":1");
        assert!(Response::decode(&duplicate, replacement).is_err());
    }
}
