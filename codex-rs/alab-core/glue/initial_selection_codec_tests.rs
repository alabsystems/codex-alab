// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact closed codec controls; no SDK/member/admission success is simulated.
use super::*;
use codex_app_server_protocol::JSONRPCErrorError;
use serde_json::json;

#[path = "initial_selection_error.rs"]
mod producer;

fn next() -> NextSelection {
    NextSelection {
        previous_claim: "0123456789abcdef0123456789abcdef".into(),
        claim: "fedcba9876543210fedcba9876543210".into(),
    }
}
fn error(data: Option<serde_json::Value>) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: -32600,
        message: "original policy failure".into(),
        data,
    }
}

#[test]
fn selection_offer_roundtrip_preserves_every_original_shape_and_presence()
-> Result<(), Box<dyn std::error::Error>> {
    for data in [
        None,
        Some(json!(null)),
        Some(json!(7)),
        Some(json!(false)),
        Some(json!("text")),
        Some(json!([null,7,{"kept":true}])),
        Some(json!({})),
        Some(json!({"_meta":{}})),
        Some(json!({"_meta":false})),
        Some(json!({"_meta":[1,2]})),
        Some(json!({"unowned":[1,2],"_meta":{"foreign.owner":{"kept":true}}})),
        Some(json!({"_meta":{"alab.initialSessionNext":{"existing":"owner"}}})),
        Some(json!({"alab.initialSessionNext":{"kind":"foreign","originalValue":false}})),
    ] {
        let original = error(data);
        let wire = producer::decorate_error(&original, next());
        // Repeated actual wire encoding/decoding represents an observer losing
        // its first response and then seeing the SAME cached carrier. This is
        // codec coverage only, not a claimed Native-accepted C1/C2 test.
        let cached = serde_json::to_value(&wire)?;
        for _ in 0..2 {
            let mut received: JSONRPCErrorError = serde_json::from_value(cached.clone())?;
            assert_eq!(
                selection_offer::take_offer(&mut received, &next().previous_claim)?,
                Some(next())
            );
            assert_eq!(
                received.data, original.data,
                "None and Some(null) must remain distinct"
            );
            assert_eq!(
                serde_json::to_value(received)?,
                serde_json::to_value(&original)?
            );
        }
    }
    Ok(())
}

#[test]
fn unowned_errors_without_the_carrier_stay_exactly_original()
-> Result<(), Box<dyn std::error::Error>> {
    for data in [
        None,
        Some(json!(null)),
        Some(json!(7)),
        Some(json!({"_meta":false})),
        Some(json!({"_meta":{"alab.initialSessionNext":{"existing":"owner"}}})),
    ] {
        let mut original = error(data);
        let before = serde_json::to_value(&original)?;
        assert_eq!(
            selection_offer::take_offer(&mut original, &next().previous_claim)?,
            None
        );
        assert_eq!(serde_json::to_value(original)?, before);
    }
    Ok(())
}

#[test]
fn malformed_carriers_deny_without_mutating_the_original_wire_error()
-> Result<(), Box<dyn std::error::Error>> {
    for (field, value) in [
        ("schema", json!(2)),
        ("kind", json!("promoted")),
        ("hadData", json!("false")),
        ("originalValue", json!(false)),
        ("invented", json!(true)),
    ] {
        let mut wire = producer::decorate_error(&error(None), next());
        wire.data.as_mut().ok_or("carrier missing")?["alab.initialSessionNext"][field] = value;
        let before = serde_json::to_value(&wire)?;
        assert!(selection_offer::take_offer(&mut wire, &next().previous_claim).is_err());
        assert_eq!(serde_json::to_value(wire)?, before);
    }
    for field in ["schema", "kind", "selection", "hadData", "originalValue"] {
        let mut wire = producer::decorate_error(&error(None), next());
        wire.data
            .as_mut()
            .and_then(|data| data.get_mut("alab.initialSessionNext"))
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("carrier object missing")?
            .remove(field);
        assert!(selection_offer::take_offer(&mut wire, &next().previous_claim).is_err());
    }
    let mut wire = producer::decorate_error(&error(None), next());
    wire.data.as_mut().ok_or("carrier missing")?["foreign"] = json!(true);
    assert!(selection_offer::take_offer(&mut wire, &next().previous_claim).is_err());
    Ok(())
}

#[test]
fn next_selection_requires_the_exact_previous_claim_and_valid_distinct_next()
-> Result<(), Box<dyn std::error::Error>> {
    let mut wire = producer::decorate_error(&error(None), next());
    assert!(selection_offer::take_offer(&mut wire, "00000000000000000000000000000000").is_err());
    for claim in [
        "0123456789abcdef0123456789abcdef",
        "bad",
        "ABCDEF0123456789ABCDEF0123456789",
    ] {
        let mut wire = producer::decorate_error(&error(None), next());
        wire.data.as_mut().ok_or("carrier missing")?["alab.initialSessionNext"]["selection"]["claim"] =
            json!(claim);
        assert!(selection_offer::take_offer(&mut wire, &next().previous_claim).is_err());
    }
    Ok(())
}
