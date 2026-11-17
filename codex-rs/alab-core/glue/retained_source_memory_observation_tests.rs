// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower closed codec controls only, never Native/current/UI acceptance.
use super::*;
#[test]
fn source_project_memory_data_is_required_closed_and_never_an_activation_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let project = metadata::Project {
        history: metadata::HistoryIntent::CurrentDisplay,
        claim: "11111111111111111111111111111111".into(),
        source_operation: "22222222222222222222222222222222".into(),
        selection: "33333333333333333333333333333333".into(),
        frontier: "44444444444444444444444444444444".into(),
        thread_id: "actual-decoder-target-data".into(),
        go: None,
    };
    let identity = serde_json::json!({"schema":1,"claim":project.claim,
        "sourceOperation":project.source_operation,"selection":project.selection,
        "frontier":project.frontier,"snapshot":"55555555555555555555555555555555",
        "threadId":project.thread_id,"taskToolsAvailable":false,"presentation":"primary"});
    for complete in [false, true] {
        let mut raw = serde_json::json!({"thread":{"id":project.thread_id,"turns":[]},"_meta":{
            "foreign.owner":[null,42],"alab.sourceSelectionProjection":{"receipt":identity,"completeMemory":complete,"sideBoundary":null}}});
        let decoded = take_receipt(&mut raw, &project)?;
        assert_eq!(decoded.complete_memory_history(), complete);
        assert!(!decoded.task_tools_available());
        assert_eq!(raw["_meta"]["foreign.owner"], serde_json::json!([null, 42]));
        let wire = serde_json::to_value(metadata::Selection::Observe(decoded.0))?;
        assert!(wire.get("completeMemory").is_none());
    }
    for complete in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!(1)),
        Some(serde_json::json!("true")),
    ] {
        let mut completion = serde_json::json!({"receipt":identity,"sideBoundary":null});
        if let Some(value) = complete {
            completion["completeMemory"] = value;
        }
        let mut raw = serde_json::json!({"thread":{"id":project.thread_id,"turns":[]},"_meta":{"alab.sourceSelectionProjection":completion}});
        assert!(take_receipt(&mut raw, &project).is_err());
    }
    let mut raw = serde_json::json!({"thread":{"id":project.thread_id,"turns":[]},"_meta":{
        "alab.sourceSelectionProjection":{"receipt":identity,"completeMemory":true,"sideBoundary":null,"nativeCurrent":true}}});
    assert!(take_receipt(&mut raw, &project).is_err());
    Ok(())
}

#[test]
fn source_side_boundary_is_required_closed_and_only_complete_side_display_data()
-> Result<(), Box<dyn std::error::Error>> {
    let project = metadata::Project {
        history: metadata::HistoryIntent::CurrentDisplay,
        claim: "1".repeat(32),
        source_operation: "2".repeat(32),
        selection: "3".repeat(32),
        frontier: "4".repeat(32),
        thread_id: "original-side-data".into(),
        go: None,
    };
    let identity = serde_json::json!({"schema":1,"claim":project.claim,"sourceOperation":project.source_operation,"selection":project.selection,"frontier":project.frontier,"snapshot":"5".repeat(32),"threadId":project.thread_id,"taskToolsAvailable":false,"presentation":"sideConversation"});
    let boundary = serde_json::json!({"schema":1,"occurrence":"6".repeat(32),"before":[{"turnId":"original-turn","itemId":"original-item"}]});
    let original = serde_json::json!({"thread":{"id":project.thread_id,"turns":[]},"_meta":{"foreign":{"kept":[1,2]},"alab.sourceSelectionProjection":{"receipt":identity,"completeMemory":true,"sideBoundary":boundary}}});
    let mut wire = original.clone();
    let receipt = take_receipt(&mut wire, &project)?;
    assert!(
        receipt
            .side_display_boundary()
            .is_some_and(|cut| cut.excludes("original-turn", "original-item"))
    );
    assert_eq!(wire["_meta"], serde_json::json!({"foreign":{"kept":[1,2]}}));
    for field in ["sideBoundary", "completeMemory"] {
        let mut bad = original.clone();
        bad["_meta"][metadata::RESULT_KEY]
            .as_object_mut()
            .ok_or("metadata missing")?
            .remove(field);
        assert!(take_receipt(&mut bad, &project).is_err());
    }
    let mut bad = original.clone();
    bad["_meta"][metadata::RESULT_KEY]["receipt"]["presentation"] = serde_json::json!("primary");
    assert!(take_receipt(&mut bad, &project).is_err());
    let mut bad = original.clone();
    bad["_meta"][metadata::RESULT_KEY]["completeMemory"] = serde_json::json!(false);
    assert!(take_receipt(&mut bad, &project).is_err());
    let mut bad = original.clone();
    bad["_meta"][metadata::RESULT_KEY]["sideBoundary"]["nativeCurrent"] = serde_json::json!(true);
    assert!(take_receipt(&mut bad, &project).is_err());
    let mut bad = original;
    bad["_meta"][metadata::RESULT_KEY]["sideBoundary"]["before"] = serde_json::json!([{"turnId":"original-turn","itemId":"original-item"},{"turnId":"original-turn","itemId":"original-item"}]);
    assert!(take_receipt(&mut bad, &project).is_err());
    Ok(())
}
