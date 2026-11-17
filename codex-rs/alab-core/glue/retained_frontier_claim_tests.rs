// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use codex_app_server_protocol::RequestId;
fn request(method: &str, params: Value) -> JSONRPCRequest {
    JSONRPCRequest {
        id: RequestId::Integer(19),
        method: method.into(),
        params: Some(params),
        trace: None,
    }
}
fn project() -> Value {
    serde_json::json!({"phase":"project","history":"currentDisplay","frontier":"11111111111111111111111111111111","threadId":"actual-selection-data"})
}
#[test]
fn ordinary_unowned_request_is_unchanged() -> io::Result<()> {
    let mut actual = request(
        "thread/resume",
        serde_json::json!({"threadId":"same","cwd":"/original","_meta":{"foreign.owner":{"k":"v"}}}),
    );
    let before = serde_json::to_value(&actual)?;
    assert!(take(&mut actual)?.is_none());
    assert_eq!(serde_json::to_value(actual)?, before);
    Ok(())
}
#[test]
fn own_selection_preserves_original_id_arguments_and_foreign_metadata() -> io::Result<()> {
    let mut actual = request(
        "thread/resume",
        serde_json::json!({"threadId":"actual-selection-data","excludeTurns":true,"cwd":"/original","_meta":{"alab.retainedFrontier":project(),"foreign.owner":{"untouched":7}}}),
    );
    let claim = take(&mut actual)?.ok_or_else(invalid)?;
    assert_eq!(claim.0.thread_id(), "actual-selection-data");
    assert_eq!(actual.id, RequestId::Integer(19));
    assert_eq!(
        actual.params,
        Some(
            serde_json::json!({"threadId":"actual-selection-data","excludeTurns":true,"cwd":"/original","_meta":{"foreign.owner":{"untouched":7}}})
        )
    );
    // Claim contains DATA only and exposes no CurrentOrigin/Gate/Go constructor.
    Ok(())
}
#[test]
fn malformed_cross_method_cross_thread_and_conflicting_owners_are_rejected() {
    let mut malformed = project();
    malformed["permission"] = serde_json::json!(true);
    for (method, params) in [
        (
            "thread/start",
            serde_json::json!({"threadId":"actual-selection-data","_meta":{"alab.retainedFrontier":project()}}),
        ),
        (
            "thread/resume",
            serde_json::json!({"threadId":"other","_meta":{"alab.retainedFrontier":project()}}),
        ),
        (
            "thread/resume",
            serde_json::json!({"threadId":"actual-selection-data","_meta":{"alab.retainedFrontier":malformed}}),
        ),
        (
            "thread/resume",
            serde_json::json!({"threadId":"actual-selection-data","_meta":{"alab.retainedFrontier":project(),"alab.initialSession":{}}}),
        ),
    ] {
        assert!(take(&mut request(method, params)).is_err());
    }
}

#[test]
fn history_intent_is_explicit_closed_data_and_keeps_original_exclude_turns() -> io::Result<()> {
    for intent in ["original", "currentDisplay"] {
        let mut selection = project();
        selection["history"] = serde_json::json!(intent);
        let mut actual = request(
            "thread/resume",
            serde_json::json!({
                "threadId":"actual-selection-data", "excludeTurns":true,
                "_meta":{"alab.retainedFrontier":selection,"foreign":{"keep":7}}
            }),
        );
        assert!(take(&mut actual)?.is_some());
        assert_eq!(
            actual
                .params
                .as_ref()
                .and_then(|params| params.get("excludeTurns")),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            actual
                .params
                .as_ref()
                .and_then(|params| params.get("_meta")),
            Some(&serde_json::json!({"foreign":{"keep":7}}))
        );
    }
    for missing in [false, true] {
        let mut selection = project();
        if missing {
            selection
                .as_object_mut()
                .ok_or_else(|| io::Error::other("fixture object"))?
                .remove("history");
        } else {
            selection["history"] = serde_json::json!("callerReady");
        }
        let mut actual = request(
            "thread/resume",
            serde_json::json!({
                "threadId":"actual-selection-data", "_meta":{"alab.retainedFrontier":selection}
            }),
        );
        assert!(take(&mut actual).is_err());
    }
    Ok(())
}
