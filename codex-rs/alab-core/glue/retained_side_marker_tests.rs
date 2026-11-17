// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn real_serde_checkpoint_keeps_data_without_recreating_local_occurrence_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let actual = RetainedSideMarker::new_local("a".repeat(32))?;
    let same = actual.clone();
    assert!(actual.same_local(&same));
    let bytes = serde_json::to_vec(&actual)?;
    let restored: RetainedSideMarker = serde_json::from_slice(&bytes)?;
    assert_eq!(restored, actual);
    assert!(!actual.same_local(&restored));
    assert!(!restored.same_local(&restored));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes)?,
        serde_json::json!({"schema":1,"token":"a".repeat(32)})
    );
    Ok(())
}

#[test]
fn matching_token_data_never_substitutes_a_distinct_live_local_allocation()
-> Result<(), Box<dyn std::error::Error>> {
    let first = RetainedSideMarker::new_local("b".repeat(32))?;
    let second = RetainedSideMarker::new_local("b".repeat(32))?;
    assert_eq!(first, second);
    assert!(!first.same_local(&second));
    Ok(())
}

#[test]
fn occurrence_data_rejects_schema_token_and_foreign_local_fields() {
    for data in [
        serde_json::json!({"schema":2,"token":"a".repeat(32)}),
        serde_json::json!({"schema":1,"token":"A".repeat(32)}),
        serde_json::json!({"schema":1,"token":"a".repeat(31)}),
        serde_json::json!({"schema":1,"token":"a".repeat(32),"local":true}),
    ] {
        assert!(serde_json::from_value::<RetainedSideMarker>(data).is_err());
    }
}
