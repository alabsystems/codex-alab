// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Private closed codec only; values construct no server Empty/Native witness.
use super::*;
#[test]
fn completion_carries_runtime_and_empty_slot_data_without_core_or_config_defaults()
-> Result<(), &'static str> {
    let mut value = serde_json::json!({"_meta":{"foreign":null,"alab.retainedConfiguration":{
        "frontier":"a".repeat(32),"snapshot":"b".repeat(32),"runtime":"c".repeat(32),"version":0}}});
    let actual = remove_receipt(&mut value).map_err(|_| "valid private syntax rejected")?;
    assert_eq!(actual.runtime_data(), "c".repeat(32));
    assert_eq!(actual.slot_version_data(), 0);
    assert_eq!(value, serde_json::json!({"_meta":{"foreign":null}}));
    Ok(())
}
#[test]
fn receipt_rejects_fake_core_flags_and_invalid_runtime_identity() {
    for extra in ["threadId", "empty", "ready", "go"] {
        let mut value = serde_json::json!({"_meta":{"alab.retainedConfiguration":{
            "frontier":"a".repeat(32),"snapshot":"b".repeat(32),"runtime":"c".repeat(32),"version":0}}});
        if let Some(meta) = value
            .get_mut("_meta")
            .and_then(|m| m.get_mut(metadata::RESULT_KEY))
            .and_then(serde_json::Value::as_object_mut)
        {
            meta.insert(extra.into(), serde_json::Value::Bool(true));
        }
        assert!(remove_receipt(&mut value).is_err());
    }
    let mut invalid = serde_json::json!({"_meta":{"alab.retainedConfiguration":{
        "frontier":"a".repeat(32),"snapshot":"b".repeat(32),"runtime":"C".repeat(32),"version":0}}});
    assert!(remove_receipt(&mut invalid).is_err());
}
