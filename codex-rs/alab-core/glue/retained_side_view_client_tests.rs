// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Immutable wire/error DATA only. No Native/Selection resource is made.
use super::*;
use serde_json::json;
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn view() -> SideView {
    SideView {
        schema: 1,
        claim: "a".repeat(32),
        thread_id: "actual-selected-child-data".into(),
    }
}
fn wire() -> TestResult<codex_app_server_protocol::JSONRPCRequest> {
    Ok(serde_json::from_value(
        json!({"id":1,"method":"thread/unsubscribe","params":{"threadId":"actual-selected-child-data","_meta":{"foreign":{"exact":[1,null,2]}}}}),
    )?)
}
#[test]
fn frozen_side_close_keeps_full_original_metadata_and_cached_error_none_null_distinct() -> TestResult
{
    for data in [
        None,
        Some(serde_json::Value::Null),
        Some(json!({"foreign":[null,"original"]})),
    ] {
        let actual = SideViewCloseOperation::new(view())?;
        let original = wire()?;
        let (selection, cached) = actual.freeze(&original)?;
        assert!(cached.is_none());
        let mut decorated = original.clone();
        decorate(&mut decorated, &selection)?;
        assert_eq!(
            decorated.params.as_ref().ok_or("params missing")?["_meta"]["foreign"],
            json!({"exact":[1,null,2]})
        );
        actual.record(Err(codex_app_server_protocol::JSONRPCErrorError {
            code: -32600,
            message: "exact original failure".into(),
            data: data.clone(),
        }))?;
        let restored = SideViewCloseOperation::restore(serde_json::from_value(
            serde_json::to_value(actual.checkpoint())?,
        )?)?;
        let (_, result) = restored.freeze(&original)?;
        assert_eq!(
            result,
            Some(Err(codex_app_server_protocol::JSONRPCErrorError {
                code: -32600,
                message: "exact original failure".into(),
                data
            }))
        );
        for field in ["threadId", "_meta"] {
            let mut changed = original.clone();
            changed.params.as_mut().ok_or("params missing")?[field] = json!("changed");
            assert!(restored.freeze(&changed).is_err());
        }
        let mut changed = original;
        changed.method = "thread/delete".into();
        assert!(restored.freeze(&changed).is_err());
    }
    Ok(())
}
#[test]
fn close_checkpoint_cannot_claim_observation_without_an_original_reply_or_frozen_wire() -> TestResult
{
    let actual = SideViewCloseOperation::new(view())?;
    let mut checkpoint = actual.checkpoint();
    checkpoint.observed = true;
    assert!(SideViewCloseOperation::restore(checkpoint).is_err());
    let mut checkpoint = actual.checkpoint();
    checkpoint.result = Some(SavedReply::capture(&Ok(json!({"status":"unsubscribed"}))));
    assert!(SideViewCloseOperation::restore(checkpoint).is_err());
    let mut wire = wire()?;
    wire.params.as_mut().ok_or("params missing")?["_meta"]["alab.sideRollback"] =
        json!({"phase":"rollback"});
    let selection = metadata::Selection::Close {
        view: view(),
        operation: "b".repeat(32),
    };
    assert!(decorate(&mut wire, &selection).is_err());
    Ok(())
}

#[test]
fn restored_side_close_requires_a_typed_success_status_in_its_cached_reply() -> TestResult {
    let owner = SideViewCloseOperation::new(view())?;
    owner.freeze(&wire()?)?;
    for status in [
        json!({}),
        json!({"status": null}),
        json!({"status": "unknownStatus"}),
    ] {
        let mut saved = serde_json::to_value(owner.checkpoint())?;
        saved["result"] = json!({"kind":"success","value":status});
        for observed in [false, true] {
            saved["observed"] = json!(observed);
            let checkpoint = serde_json::from_value(saved.clone())?;
            assert!(SideViewCloseOperation::restore(checkpoint).is_err());
        }
    }
    Ok(())
}

#[test]
fn closed_cleanup_observation_and_error_data_discriminator_reject_foreign_or_ambiguous_shapes()
-> TestResult {
    let actual = SideViewCloseOperation::new(view())?;
    actual.freeze(&wire()?)?;
    let mut checkpoint = actual.checkpoint();
    checkpoint.result = Some(SavedReply::Error {
        code: -1,
        message: "original".into(),
        had_data: false,
        value: json!({"foreign":true}),
    });
    assert!(SideViewCloseOperation::restore(checkpoint).is_err());
    let data = json!({"schema":1,"operation":"b".repeat(32),"claim":"a".repeat(32),"threadId":"actual-selected-child-data","state":"unowned"});
    let observed: metadata::observation::Observation = serde_json::from_value(data.clone())?;
    assert!(observed.matches(
        &"b".repeat(32),
        &"a".repeat(32),
        "actual-selected-child-data"
    ));
    assert!(!observed.matches(
        &"c".repeat(32),
        &"a".repeat(32),
        "actual-selected-child-data"
    ));
    for state in [json!(true), json!({"unowned":null}), json!("current")] {
        let mut malformed = data.clone();
        malformed["state"] = state;
        assert!(serde_json::from_value::<metadata::observation::Observation>(malformed).is_err());
    }
    let mut malformed = data.clone();
    malformed["nativeCurrent"] = json!(true);
    assert!(serde_json::from_value::<metadata::observation::Observation>(malformed).is_err());
    let mut exact = json!({"_meta":{}});
    exact["_meta"][metadata::OBSERVATION_KEY] = data;
    super::super::side_cleanup_reply::observation_value(exact.clone(), metadata::OBSERVATION_KEY)?;
    let mut malformed = exact;
    malformed["status"] = json!("unsubscribed");
    assert!(
        super::super::side_cleanup_reply::observation_value(malformed, metadata::OBSERVATION_KEY)
            .is_err()
    );
    Ok(())
}
