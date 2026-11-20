// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use anyhow::Context;
use anyhow::Result;
use codex_app_server_protocol::ThreadResumeParams;
use serde_json::json;

fn offer() -> Result<PreparationOffer> {
    Ok(serde_json::from_value(json!({
        "preparation":"a".repeat(32), "claim":"b".repeat(32),
        "sourceOperation":"c".repeat(32), "selection":"d".repeat(32)
    }))?)
}

#[test]
fn saved_resume_receipt_key_matches_the_accepted_fields_and_json_equality() -> Result<()> {
    let offer = offer()?;
    let child = codex_protocol::ThreadId::new().to_string();
    let params = ThreadResumeParams {
        thread_id: child.clone(),
        ..Default::default()
    };
    let mut original: PreparedArguments = serde_json::from_value(json!({
        "preparation":offer.preparation, "savedOpening":offer.preparation, "method":"thread/resume",
        "parameters":params, "selection":{"prospectiveCreate": ThreadStartParams::default(),
        "presentation":"sideConversation"}
    }))?;
    let from_fields = |request: &PreparedArguments| {
        key_for_fields(
            &offer,
            true,
            request.method.name(),
            &request.parameters,
            &request.selection.prospective_create,
            request.selection.presentation,
        )
    };
    let base = key(&original, &offer).context("tagged original needs a receipt key")?;
    assert_eq!(Some(base), from_fields(&original));
    for changed_offer in [
        PreparationOffer {
            claim: "e".repeat(32),
            ..offer.clone()
        },
        PreparationOffer {
            source_operation: "e".repeat(32),
            ..offer.clone()
        },
        PreparationOffer {
            selection: "e".repeat(32),
            ..offer.clone()
        },
    ] {
        let changed = key(&original, &changed_offer).context("valid alternative offer")?;
        assert_ne!(base, changed);
    }
    original.saved_opening = None;
    assert!(key(&original, &offer).is_none());
    original.saved_opening = Some(offer.preparation.clone());
    original.parameters["threadId"] = json!(codex_protocol::ThreadId::new().to_string());
    assert_ne!(key(&original, &offer), Some(base));
    original.parameters["threadId"] = json!(child);
    original.selection.prospective_create.config = Some(HashMap::from([
        ("other".into(), json!("value")),
        ("nested".into(), serde_json::from_str(r#"[{"b":2,"a":1}]"#)?),
    ]));
    let configured = key(&original, &offer).context("configured original")?;
    assert_ne!(base, configured);
    original.selection.prospective_create.config = Some(HashMap::from([
        ("nested".into(), serde_json::from_str(r#"[{"a":1,"b":2}]"#)?),
        ("other".into(), json!("value")),
    ]));
    assert_eq!(Some(configured), key(&original, &offer));

    let with_number = |number: Value| {
        let mut request = original.clone();
        request.selection.prospective_create.config = Some(HashMap::from([("n".into(), number)]));
        key(&request, &offer)
    };
    let plus = json!(0.0);
    let minus = json!(-0.0);
    assert_eq!(
        with_number(plus.clone()) == with_number(minus.clone()),
        plus == minus
    );
    assert_ne!(with_number(plus), with_number(json!(0)));
    // A raw object whose key happens to be serde's private number token is
    // still an object in an accepted selector's Value. Serialization for the
    // canonical key must keep it distinct from the actual JSON number.
    let mut reserved = serde_json::Map::new();
    reserved.insert(
        "$serde_json::private::Number".into(),
        Value::String("1".into()),
    );
    assert_ne!(with_number(Value::Object(reserved)), with_number(json!(1)));
    Ok(())
}

#[test]
fn saved_resume_receipt_preserves_the_complete_original_rejection() -> Result<()> {
    for data in [
        None,
        Some(Value::Null),
        Some(json!({"detail":[3.75, true, "error"]})),
    ] {
        let error = JSONRPCErrorError {
            code: -32021,
            message: "The original worker stopped".into(),
            data,
        };
        let receipt = Receipt::rejected(&error).context("small error is representable")?;
        assert_eq!(
            receipt.outcome(),
            SavedResumeOutcome::Rejected {
                code: error.code,
                message: error.message,
                had_data: error.data.is_some(),
                value: error.data.unwrap_or(Value::Null),
            }
        );
    }
    let too_big = JSONRPCErrorError {
        code: -1,
        message: String::new(),
        data: Some(json!({"nested":"x".repeat(MAX_ERROR_BYTES)})),
    };
    assert!(Receipt::rejected(&too_big).is_none());
    Ok(())
}

fn sequence_key(index: usize) -> Key {
    let mut key = [0; 32];
    let bytes = index.to_be_bytes();
    key[..bytes.len()].copy_from_slice(&bytes);
    key
}

#[test]
fn saved_resume_receipt_fifo_limits_bytes_and_entry_count_without_refresh_on_read() -> Result<()> {
    let large = Receipt::rejected(&JSONRPCErrorError {
        code: -1,
        message: "m".repeat(MAX_ERROR_BYTES - 64),
        data: None,
    })
    .context("error should fit an individual receipt")?;
    let retained = MAX_TOTAL_BYTES / large.size();
    let mut cache = Receipts::default();
    for index in 0..retained {
        cache.insert(sequence_key(index), large.clone());
    }
    assert!(cache.get(&sequence_key(0)).is_some());
    cache.insert(sequence_key(retained), large);
    assert!(cache.get(&sequence_key(0)).is_none());
    assert!(cache.get(&sequence_key(1)).is_some());
    assert!(cache.bytes <= MAX_TOTAL_BYTES);
    let mut cache = Receipts::default();
    for index in 0..=MAX_ENTRIES {
        cache.insert(sequence_key(index), Receipt::resumed());
    }
    assert!(cache.get(&sequence_key(0)).is_none());
    assert_eq!(cache.entries.len(), MAX_ENTRIES);
    assert_eq!(cache.order.len(), MAX_ENTRIES);
    assert_eq!(
        cache
            .get(&sequence_key(1))
            .context("second receipt stays")?
            .outcome(),
        SavedResumeOutcome::Resumed {}
    );
    Ok(())
}
