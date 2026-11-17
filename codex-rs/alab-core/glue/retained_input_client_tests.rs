// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Local checkpoint DATA only; restoring this creates no reply/cell authority.
use super::*;
fn checkpoint() -> serde_json::Value {
    serde_json::json!({"schema":1,"service":"00000000000000000000000000000001","operation":"00000000000000000000000000000002","requestId":17,"descriptor":null,"value":null,"receipt":null})
}
#[test]
fn immutable_value_and_operation_survive_before_first_send_checkpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let owner = OriginalInputReservation::restore_checkpoint(checkpoint())?;
    let original = serde_json::json!({"answers":{"q":{"answers":["queued A"]}}});
    let first = owner.freeze(original.clone())?;
    if owner.freeze(serde_json::json!({"answers":{}})).is_ok() {
        return Err("second answer replaced original Value".into());
    }
    let restored = OriginalInputReservation::restore_checkpoint(first.checkpoint()?)?;
    let pending = restored
        .pending_publication()
        .ok_or("original queued publication lost")?;
    if pending.checkpoint()? != first.checkpoint()? {
        return Err("restoration changed original operation/value".into());
    }
    let _same = restored.freeze(original)?;
    Ok(())
}
#[test]
fn checkpoint_refuses_rebound_callback_descriptor_and_missing_value_receipt() {
    let mut value = checkpoint();
    value["descriptor"] = serde_json::json!({"epoch":"00000000000000000000000000000003","ordinal":1,"operation":"00000000000000000000000000000004","requestId":17,"proof":"a".repeat(64)});
    assert!(OriginalInputReservation::restore_checkpoint(value).is_err());
    let mut value = checkpoint();
    value["receipt"] = serde_json::json!({"descriptor":{"epoch":"00000000000000000000000000000003","ordinal":1,"operation":"00000000000000000000000000000002","requestId":17,"proof":"a".repeat(64)},"outcome":{"state":"published"},"proof":"b".repeat(64)});
    assert!(OriginalInputReservation::restore_checkpoint(value).is_err());
}
#[test]
fn original_publication_outcome_codec_rejects_effect_claims_and_extra_fields() {
    for value in [
        serde_json::json!({"state":"published","effectsSettled":true}),
        serde_json::json!({"state":"notAccepted"}),
        serde_json::json!({"state":"coreConsumed"}),
    ] {
        assert!(serde_json::from_value::<PublicationOutcome>(value).is_err());
    }
}

#[test]
fn checkpoint_requires_explicit_optional_owner_value_and_result_fields() {
    for field in ["descriptor", "value", "receipt"] {
        let mut data = checkpoint();
        data.as_object_mut()
            .expect("checkpoint object")
            .remove(field);
        assert!(
            OriginalInputReservation::restore_checkpoint(data).is_err(),
            "missing {field} became an implicit empty owner"
        );
    }
    assert!(OriginalInputReservation::restore_checkpoint(checkpoint()).is_ok());
}
