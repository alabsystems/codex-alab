// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Closed transport data checks only; these fixtures issue no controller role.
use super::*;

#[test]
fn startup_transport_rejects_public_endpoints_and_handoff_claims() {
    let value = serde_json::json!({"state":"ready","endpoint":"ws://127.0.0.1:43210","credential":"a".repeat(64)});
    let backend = ready(serde_json::from_value(value.clone()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(backend.address(), "127.0.0.1:43210".parse().unwrap());
    assert_eq!(backend.credential(), "a".repeat(64));
    assert!(!format!("{backend:?}").contains(&"a".repeat(64)));
    for endpoint in [
        "ws://0.0.0.0:43210",
        "ws://localhost:43210",
        "ws://127.0.0.1:0",
        "ws://127.0.0.1:43210/path",
        "wss://127.0.0.1:43210",
    ] {
        let mut changed = value.clone();
        changed["endpoint"] = serde_json::json!(endpoint);
        assert!(ready(serde_json::from_value(changed).unwrap()).is_err());
    }
    let mut changed = value;
    changed["handoff"] = serde_json::json!(true);
    assert!(serde_json::from_value::<State>(changed).is_err());
}

#[test]
fn stop_observations_cannot_claim_all_source_work_or_goal_readiness() {
    assert!(
        !stopped(serde_json::from_value(serde_json::json!({"state":"stopping"})).unwrap()).unwrap()
    );
    assert!(
        stopped(
            serde_json::from_value(
                serde_json::json!({"state":"stopped","receipt_scope":"captured-native-root"})
            )
            .unwrap()
        )
        .unwrap()
    );
    assert!(
        stopped(
            serde_json::from_value(
                serde_json::json!({"state":"stopped","receipt_scope":"all-source-work"})
            )
            .unwrap()
        )
        .is_err()
    );
    assert!(serde_json::from_value::<StopState>(serde_json::json!({"state":"stopped","receipt_scope":"captured-native-root","next_goal_ready":true})).is_err());
}
