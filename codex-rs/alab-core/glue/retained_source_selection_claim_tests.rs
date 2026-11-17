// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Syntax/correlation controls only; no token is treated as Native authority.
use super::*;
use serde_json::json;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn wire() -> Result<JSONRPCRequest> {
    Ok(serde_json::from_value(
        json!({"id":1,"method":"thread/resume","params":{
            "threadId":"original-target","model":42,"_meta":{"foreign":{"kept":[1,2]},
            "alab.sourceSelectionFrontier":{"phase":"project","history":"currentDisplay","claim":"11111111111111111111111111111111",
                "sourceOperation":"22222222222222222222222222222222","selection":"33333333333333333333333333333333",
                "frontier":"44444444444444444444444444444444","threadId":"original-target","go":null}}
        }}),
    )?)
}
#[test]
fn source_codec_preserves_complete_original_and_foreign_parser_errors() -> Result<()> {
    let mut request = wire()?;
    let claim = take(&mut request)?.ok_or("Source DATA missing")?;
    assert!(claim.0.thread_id() == "original-target");
    assert!(
        request.params.as_ref().ok_or("params missing")?["_meta"]
            == json!({"foreign":{"kept":[1,2]}})
    );
    assert!(request.params.as_ref().ok_or("params missing")?["model"] == 42);
    assert!(
        serde_json::from_value::<codex_app_server_protocol::ClientRequest>(serde_json::to_value(
            request
        )?)
        .is_err()
    );
    let mut foreign: JSONRPCRequest =
        serde_json::from_value(json!({"id":2,"method":"thread/resume","params":{
            "threadId":"original-target","_meta":"unowned-invalid-metadata"
        }}))?;
    let original = serde_json::to_value(&foreign)?;
    assert!(take(&mut foreign)?.is_none());
    assert!(serde_json::to_value(foreign)? == original);
    Ok(())
}
#[test]
fn source_codec_cannot_mix_runtime_configured_preparation_or_change_target() -> Result<()> {
    for owner in [
        "alab.retainedFrontier",
        "alab.retainedConfigured",
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.clientConstructionPrepare",
    ] {
        let mut request = wire()?;
        request.params.as_mut().ok_or("params missing")?["_meta"][owner] = json!({});
        assert!(
            take(&mut request).is_err(),
            "combined owner {owner} accepted"
        );
    }
    let mut changed = wire()?;
    changed.params.as_mut().ok_or("params missing")?["threadId"] = json!("different-target");
    assert!(take(&mut changed).is_err());
    let mut extra = wire()?;
    extra.params.as_mut().ok_or("params missing")?["_meta"][metadata::KEY]["nativeCurrent"] =
        json!(true);
    assert!(take(&mut extra).is_err());
    Ok(())
}

#[test]
fn source_history_intent_is_required_and_cannot_be_a_caller_ready_label() -> Result<()> {
    for intent in ["original", "currentDisplay"] {
        let mut actual = wire()?;
        actual.params.as_mut().ok_or("params missing")?["_meta"][metadata::KEY]["history"] =
            json!(intent);
        assert!(take(&mut actual)?.is_some());
        assert_eq!(
            actual.params.as_ref().ok_or("params missing")?["model"],
            json!(42)
        );
    }
    for missing in [false, true] {
        let mut actual = wire()?;
        let project = &mut actual.params.as_mut().ok_or("params missing")?["_meta"][metadata::KEY];
        if missing {
            project
                .as_object_mut()
                .ok_or("project missing")?
                .remove("history");
        } else {
            project["history"] = json!("callerReady");
        }
        assert!(take(&mut actual).is_err());
    }
    Ok(())
}
