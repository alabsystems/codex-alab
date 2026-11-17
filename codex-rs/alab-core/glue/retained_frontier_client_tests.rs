// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
fn data() -> Receipt {
    Receipt(
        metadata::Receipt {
            frontier: "11111111111111111111111111111111".into(),
            snapshot: "22222222222222222222222222222222".into(),
            thread_id: "selected-thread-data".into(),
            task_tools_available: false,
        },
        false,
    )
}
#[test]
fn activation_is_metadata_completion_without_a_fake_resume_response()
-> Result<(), Box<dyn std::error::Error>> {
    let receipt = data();
    let raw = serde_json::json!({"_meta":{"foreign.owner":{"preserved":true},"alab.retainedProjection":serde_json::to_value(&receipt.0)?}});
    let actual = decode_activation(raw, &receipt)?;
    assert_eq!(actual.receipt().frontier_data(), receipt.frontier_data());
    assert_eq!(actual.receipt().snapshot_data(), receipt.snapshot_data());
    assert_eq!(actual.receipt().thread_id_data(), receipt.thread_id_data());
    Ok(())
}
#[test]
fn activation_rejects_wrong_snapshot_thread_frontier_and_private_fields()
-> Result<(), serde_json::Error> {
    let receipt = data();
    for field in ["snapshot", "threadId", "frontier"] {
        let mut wrong = serde_json::to_value(&receipt.0)?;
        wrong[field] = serde_json::json!("other");
        assert!(
            decode_activation(
                serde_json::json!({"_meta":{"alab.retainedProjection":wrong}}),
                &receipt
            )
            .is_err()
        );
    }
    let mut extra = serde_json::to_value(&receipt.0)?;
    extra["permission"] = serde_json::json!(true);
    assert!(
        decode_activation(
            serde_json::json!({"_meta":{"alab.retainedProjection":extra}}),
            &receipt
        )
        .is_err()
    );
    assert!(decode_activation(serde_json::json!({"thread":{}}), &receipt).is_err());
    Ok(())
}

#[test]
fn projection_carries_exact_task_capability_and_preserves_foreign_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    for available in [false, true] {
        let mut receipt = data();
        receipt.0.task_tools_available = available;
        let foreign = serde_json::json!({"complete":[null,{"kept":true}]});
        let mut raw = serde_json::json!({"thread":{"id":receipt.thread_id_data()},"_meta":{
            "foreign.owner":foreign,"alab.retainedProjection":{"receipt":serde_json::to_value(&receipt.0)?,"completeMemory":false}
        }});
        let actual =
            take_projection_receipt(&mut raw, receipt.frontier_data(), receipt.thread_id_data())?;
        assert_eq!(actual.task_tools_available(), available);
        assert_eq!(raw["_meta"]["foreign.owner"], foreign);
        assert!(raw["_meta"].get("alab.retainedProjection").is_none());
    }
    Ok(())
}
#[test]
fn projection_never_guesses_missing_or_malformed_task_capability()
-> Result<(), Box<dyn std::error::Error>> {
    let receipt = data();
    for value in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!("true")),
        Some(serde_json::json!(1)),
    ] {
        let mut metadata = serde_json::to_value(&receipt.0)?;
        metadata
            .as_object_mut()
            .ok_or("receipt object missing")?
            .remove("taskToolsAvailable");
        if let Some(value) = value {
            metadata["taskToolsAvailable"] = value;
        }
        let mut raw = serde_json::json!({"thread":{"id":receipt.thread_id_data()},"_meta":{"alab.retainedProjection":{"receipt":metadata,"completeMemory":false}}});
        assert!(
            take_projection_receipt(&mut raw, receipt.frontier_data(), receipt.thread_id_data())
                .is_err()
        );
    }
    Ok(())
}
#[test]
fn activation_rejects_changed_task_capability_even_with_same_projection_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let receipt = data();
    let mut changed = serde_json::to_value(&receipt.0)?;
    changed["taskToolsAvailable"] = serde_json::json!(true);
    assert!(
        decode_activation(
            serde_json::json!({"_meta":{"alab.retainedProjection":changed}}),
            &receipt
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn project_requires_explicit_closed_memory_snapshot_observation_even_for_empty_turns()
-> Result<(), Box<dyn std::error::Error>> {
    let receipt = data();
    for complete in [false, true] {
        let mut raw = serde_json::json!({"thread":{"id":receipt.thread_id_data(),"turns":[]},"_meta":{
            "foreign.owner":{"preserve":[null,1]},"alab.retainedProjection":{
                "receipt":serde_json::to_value(&receipt.0)?,"completeMemory":complete}}});
        let actual =
            take_projection_receipt(&mut raw, receipt.frontier_data(), receipt.thread_id_data())?;
        assert_eq!(actual.complete_memory_history(), complete);
        assert_eq!(raw["thread"]["turns"], serde_json::json!([]));
        assert_eq!(
            raw["_meta"]["foreign.owner"],
            serde_json::json!({"preserve":[null,1]})
        );
    }
    for complete in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!(1)),
        Some(serde_json::json!("true")),
    ] {
        let mut completion = serde_json::json!({"receipt":serde_json::to_value(&receipt.0)?});
        if let Some(value) = complete {
            completion["completeMemory"] = value;
        }
        let mut raw = serde_json::json!({"thread":{"id":receipt.thread_id_data()},"_meta":{"alab.retainedProjection":completion}});
        assert!(
            take_projection_receipt(&mut raw, receipt.frontier_data(), receipt.thread_id_data())
                .is_err()
        );
    }
    for cursor in ["turnsBackwardsCursor", "itemsBackwardsCursor"] {
        let mut raw = serde_json::json!({"thread":{"id":receipt.thread_id_data(),"turns":[]},"_meta":{
            "alab.retainedProjection":{"receipt":serde_json::to_value(&receipt.0)?,"completeMemory":true}}});
        raw[cursor] = serde_json::json!("not-a-memory-cursor");
        assert!(
            take_projection_receipt(&mut raw, receipt.frontier_data(), receipt.thread_id_data())
                .is_err()
        );
    }
    let mut completion = serde_json::json!({"receipt":serde_json::to_value(&receipt.0)?,"completeMemory":true,"nativePermission":true});
    let mut raw = serde_json::json!({"thread":{"id":receipt.thread_id_data()},"_meta":{"alab.retainedProjection":completion.take()}});
    assert!(
        take_projection_receipt(&mut raw, receipt.frontier_data(), receipt.thread_id_data())
            .is_err()
    );
    Ok(())
}
