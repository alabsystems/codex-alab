// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Codec controls only; no Native origin or Core permission is created.
use super::*;
use serde_json::json;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn wire() -> Result<JSONRPCRequest> {
    Ok(serde_json::from_value(
        json!({"id":1,"method":"thread/inject_items","params":{
            "threadId":"original","items":42,"_meta":{"foreign":{"kept":[1,2]},"alab.sideInjection":{"schema":1,"claim":"11111111111111111111111111111111"}}
        }}),
    )?)
}
#[test]
fn side_claim_preserves_full_foreign_parameters_and_original_parser_error() -> Result {
    let mut request = wire()?;
    let claim = take(&mut request)?.ok_or("owned claim missing")?;
    assert_eq!(claim.parameters["items"], 42);
    assert_eq!(claim.parameters["_meta"], json!({"foreign":{"kept":[1,2]}}));
    assert!(
        serde_json::from_value::<codex_app_server_protocol::ClientRequest>(serde_json::to_value(
            &request
        )?)
        .is_err()
    );
    let mut foreign: JSONRPCRequest = serde_json::from_value(
        json!({"id":2,"method":"thread/inject_items","params":{"_meta":"ordinary invalid metadata"}}),
    )?;
    let original = serde_json::to_value(&foreign)?;
    assert!(take(&mut foreign)?.is_none());
    assert_eq!(original, serde_json::to_value(foreign)?);
    Ok(())
}
#[test]
fn side_claim_rejects_every_combined_owner_wrong_method_and_extra_grant_label() -> Result {
    for key in [
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.clientConstructionPrepare",
        "alab.retainedFrontier",
        "alab.sourceSelectionFrontier",
        "alab.retainedConfigured",
        "alab.retainedReader",
        "alab.retainedCallbacks",
    ] {
        let mut request = wire()?;
        request.params.as_mut().ok_or("params missing")?["_meta"][key] = json!({});
        assert!(take(&mut request).is_err(), "combined {key}");
    }
    let mut request = wire()?;
    request.method = "thread/start".into();
    assert!(take(&mut request).is_err());
    let mut request = wire()?;
    request.params.as_mut().ok_or("params missing")?["_meta"][metadata::KEY]["nativeCurrent"] =
        json!(true);
    assert!(take(&mut request).is_err());
    Ok(())
}
