// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Codec/original request DATA only; no Empty handle, Origin or SDK receipt.
use super::*;
fn request(params: Value) -> JSONRPCRequest {
    JSONRPCRequest {
        id: codex_app_server_protocol::RequestId::String("actual-config-call".into()),
        method: "config/read".into(),
        params: Some(params),
        trace: None,
    }
}
#[test]
fn ordinary_configuration_request_is_unchanged() -> io::Result<()> {
    let mut actual = request(
        serde_json::json!({"includeLayers":true,"cwd":"/original","_meta":{"foreign":null}}),
    );
    let before = serde_json::to_value(&actual).map_err(io::Error::other)?;
    assert!(take(&mut actual)?.is_none());
    assert_eq!(
        serde_json::to_value(actual).map_err(io::Error::other)?,
        before
    );
    Ok(())
}
#[test]
fn configured_projection_removes_only_owned_metadata_and_invents_no_thread() -> io::Result<()> {
    let mut actual = request(
        serde_json::json!({"includeLayers":true,"cwd":"/original","_meta":{
        "foreign":{"retained":null}, "alab.retainedConfigured":{"phase":"project","frontier":"a".repeat(32)}}}),
    );
    let claim = take(&mut actual)?.ok_or_else(invalid)?;
    assert!(matches!(claim.selection, Selection::Project { .. }));
    assert_eq!(
        actual.id,
        codex_app_server_protocol::RequestId::String("actual-config-call".into())
    );
    assert_eq!(
        actual.params,
        Some(
            serde_json::json!({"includeLayers":true,"cwd":"/original","_meta":{"foreign":{"retained":null}}})
        )
    );
    Ok(())
}
#[test]
fn wrong_method_fake_core_and_competing_selection_namespace_are_denied() {
    let mut wrong = request(
        serde_json::json!({"_meta":{"alab.retainedConfigured":{"phase":"project","frontier":"a".repeat(32)}}}),
    );
    wrong.method = "thread/read".into();
    let before = wrong.clone();
    assert!(take(&mut wrong).is_err());
    assert_eq!(wrong, before);
    let mut core = request(
        serde_json::json!({"_meta":{"alab.retainedConfigured":{"phase":"project","frontier":"a".repeat(32),"threadId":"invented"}}}),
    );
    assert!(take(&mut core).is_err());
    for foreign in [
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.retainedFrontier",
        "alab.clientConstructionPrepare",
        "alab.sourceSelectionFrontier",
    ] {
        let mut competing = request(
            serde_json::json!({"_meta":{"alab.retainedConfigured":{"phase":"project","frontier":"a".repeat(32)}}}),
        );
        if let Some(meta) = competing
            .params
            .as_mut()
            .and_then(|p| p.get_mut("_meta"))
            .and_then(Value::as_object_mut)
        {
            meta.insert(foreign.into(), Value::Null);
        }
        let before = competing.clone();
        assert!(take(&mut competing).is_err());
        assert_eq!(competing, before);
    }
}

#[test]
fn accepted_configuration_freezes_all_original_unowned_arguments() -> io::Result<()> {
    let mut first = request(
        serde_json::json!({"includeLayers":true,"cwd":"/original","_meta":{
        "foreign":{"keep":1},"alab.retainedConfigured":{"phase":"project","frontier":"a".repeat(32)}}}),
    );
    let first = take(&mut first)?.ok_or_else(invalid)?;
    let mut changed = request(
        serde_json::json!({"includeLayers":true,"cwd":"/original","_meta":{
        "foreign":{"keep":2},"alab.retainedConfigured":{"phase":"project","frontier":"a".repeat(32)}}}),
    );
    let changed = take(&mut changed)?.ok_or_else(invalid)?;
    assert_ne!(first.arguments, changed.arguments);
    assert!(first.arguments.get("threadId").is_none());
    Ok(())
}
