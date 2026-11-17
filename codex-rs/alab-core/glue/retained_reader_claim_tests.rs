// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Closed request DATA parser; no Native currentness is simulated.
use super::*;
#[test]
fn managed_reader_removes_only_owned_metadata_and_rejects_all_competing_owners()
-> Result<(), Box<dyn std::error::Error>> {
    let valid = serde_json::json!({"id":"original","method":"config/read","params":{
        "includeLayers":true,"_meta":{"foreign.owner":{"kept":null},"alab.retainedReader":{
            "service":"a".repeat(32),"operation":"b".repeat(32),"kind":"managed"}}}});
    let mut request: JSONRPCRequest = serde_json::from_value(valid.clone())?;
    let selected = take(&mut request)?.ok_or("actual DATA selection missing")?;
    assert!(selected.valid());
    assert_eq!(
        request.params.as_ref().and_then(|p| p.get("_meta")),
        Some(&serde_json::json!({"foreign.owner":{"kept":null}}))
    );
    for key in [
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.retainedFrontier",
        "alab.clientConstructionPrepare",
        "alab.sourceSelectionFrontier",
        "alab.retainedConfigured",
    ] {
        let mut raw = valid.clone();
        raw["params"]["_meta"][key] = serde_json::json!({});
        let mut request: JSONRPCRequest = serde_json::from_value(raw)?;
        let before = serde_json::to_value(&request)?;
        assert!(take(&mut request).is_err());
        assert_eq!(
            serde_json::to_value(request)?,
            before,
            "refusal changed original request"
        );
    }
    let mut raw = valid;
    raw["method"] = serde_json::json!("thread/start");
    let mut request: JSONRPCRequest = serde_json::from_value(raw)?;
    assert!(take(&mut request).is_err());
    Ok(())
}
