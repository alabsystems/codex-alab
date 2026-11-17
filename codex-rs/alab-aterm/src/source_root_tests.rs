// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Decoder checks only: these data fixtures grant no SDK or root authority.
use super::*;

#[test]
fn lifecycle_observations_bind_exact_operation_and_receipt_scope() {
    let tool = "a".repeat(32);
    let birth = "b".repeat(32);
    let running =
        serde_json::json!({"version":1,"tool_operation":tool,"birth":birth,"state":"running"});
    assert!(matches!(
        observed(serde_json::from_value(running.clone()).unwrap(), &tool).unwrap(),
        ToolState::Running { .. }
    ));
    let finished = serde_json::json!({"version":1,"tool_operation":tool,"birth":birth,"state":"finished","wait_status":0,"receipt_scope":"captured-cohort"});
    assert!(matches!(
        observed(serde_json::from_value(finished.clone()).unwrap(), &tool).unwrap(),
        ToolState::Finished { wait_status: 0, .. }
    ));
    for (field, value) in [
        ("version", serde_json::json!(2)),
        ("tool_operation", serde_json::json!("c".repeat(32))),
        ("birth", serde_json::json!("short")),
        ("receipt_scope", serde_json::json!("all-source-work")),
    ] {
        let mut changed = finished.clone();
        changed[field] = value;
        assert!(observed(serde_json::from_value(changed).unwrap(), &tool).is_err());
    }
    for mut changed in [running, finished] {
        changed["ready"] = serde_json::json!(true);
        assert!(serde_json::from_value::<State>(changed).is_err());
    }
}

#[test]
fn io_offers_bind_exact_operation_and_reject_extra_authority_claims() {
    let tool = "a".repeat(32);
    for (state, field) in [("pending", "nonce"), ("retained", "birth")] {
        let mut offer = serde_json::json!({"version":1,"tool_operation":tool,"state":state});
        offer[field] = serde_json::json!("b".repeat(32));
        assert!(offered(serde_json::from_value(offer.clone()).unwrap(), &tool).is_ok());
        for (name, value) in [
            ("version", serde_json::json!(2)),
            ("tool_operation", serde_json::json!("c".repeat(32))),
            (field, serde_json::json!("short")),
        ] {
            let mut changed = offer.clone();
            changed[name] = value;
            assert!(offered(serde_json::from_value(changed).unwrap(), &tool).is_err());
        }
        offer["ready"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Offer>(offer).is_err());
    }
}

#[test]
fn exec_observations_are_correlated_and_do_not_claim_readiness() {
    let tool = "a".repeat(32);
    let awaiting = serde_json::json!({"version":1,"tool_operation":tool,"state":"awaiting_birth"});
    assert!(matches!(
        execution_observed(serde_json::from_value(awaiting).unwrap(), &tool).unwrap(),
        codex_alab_core::native_root::ToolExecution::AwaitingBirth
    ));
    for state in ["pending", "no_exec_error", "failed"] {
        let mut value = serde_json::json!({"version":1,"tool_operation":tool,"birth":"b".repeat(32),"state":state});
        if state == "failed" {
            value["errno"] = serde_json::json!(2);
        }
        assert!(execution_observed(serde_json::from_value(value.clone()).unwrap(), &tool).is_ok());
        let mut wrong = value.clone();
        wrong["tool_operation"] = serde_json::json!("c".repeat(32));
        assert!(execution_observed(serde_json::from_value(wrong).unwrap(), &tool).is_err());
        value["ready"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Execution>(value).is_err());
    }
    assert!(serde_json::from_value::<Execution>(serde_json::json!({"version":1,"tool_operation":tool,"birth":"b".repeat(32),"state":"executed"})).is_err());
}

#[test]
fn rejected_attempts_are_distinct_from_captured_births() {
    use codex_alab_core::native_root::ToolExecution;
    let tool = "a".repeat(32);
    let attempt = "b".repeat(32);
    let rejected = serde_json::json!({"version":1,"tool_operation":tool,"state":"rejected"});
    assert_eq!(
        offered(serde_json::from_value(rejected).unwrap(), &tool).unwrap(),
        IoOffer::Rejected
    );
    let not_spawned = serde_json::json!({"version":1,"tool_operation":tool,"state":"not_spawned","attempt":attempt,"errno":2});
    assert_eq!(
        execution_observed(serde_json::from_value(not_spawned.clone()).unwrap(), &tool).unwrap(),
        ToolExecution::NotSpawned {
            attempt: attempt.clone(),
            errno: Some(2)
        }
    );
    let refused =
        serde_json::json!({"version":1,"tool_operation":tool,"state":"refused","errno":null});
    assert_eq!(
        execution_observed(serde_json::from_value(refused.clone()).unwrap(), &tool).unwrap(),
        ToolExecution::Refused { errno: None }
    );
    for mut value in [not_spawned, refused] {
        value["birth"] = serde_json::json!(attempt);
        assert!(serde_json::from_value::<Execution>(value).is_err());
    }
}
