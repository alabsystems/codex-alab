// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Lower wire/checkpoint DATA fixture, no actual SDK reservation or grant.
use super::*;
use serde_json::json;
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
#[test]
fn side_rollback_recovery_keeps_same_delete_params_and_original_error_without_a_second_choice()
-> TestResult {
    let construction = ClientConstructionOperation::side_codec_fixture()?;
    let rollback = SideRollbackOperation::new(&construction)?;
    let wire: codex_app_server_protocol::JSONRPCRequest = serde_json::from_value(
        json!({"id":1,"method":"thread/delete","params":{"threadId":"original-child","_meta":{"foreign":{"kept":true}}}}),
    )?;
    rollback.freeze(&wire)?;
    let original = Err(codex_app_server_protocol::JSONRPCErrorError {
        code: -32600,
        message: "exact original deletion failure".into(),
        data: Some(serde_json::Value::Null),
    });
    rollback.record(original.clone())?;
    let restored = SideRollbackOperation::restore(
        &construction,
        serde_json::from_value(serde_json::to_value(rollback.checkpoint())?)?,
    )?;
    assert_eq!(restored.freeze(&wire)?.1, Some(original));
    let mut changed = wire.clone();
    changed.params.as_mut().ok_or("params missing")?["_meta"]["foreign"] = json!({"changed":true});
    assert!(restored.freeze(&changed).is_err());
    let mut changed = wire;
    changed.method = "thread/unsubscribe".into();
    assert!(restored.freeze(&changed).is_err());
    assert!(SideRollbackOperation::new(&ClientConstructionOperation::for_start()).is_err());
    let mut malformed = serde_json::to_value(restored.checkpoint())?;
    malformed["claim"] = json!("c".repeat(32));
    assert!(
        SideRollbackOperation::restore(&construction, serde_json::from_value(malformed)?).is_err()
    );
    Ok(())
}
