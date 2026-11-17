// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Protocol DATA controls, not Native/SDK/currentness or UI readiness grants.
use super::*;
use serde_json::json;

#[test]
fn exact_ready_and_closed_failure_are_distinct_terminal_data() {
    for (outcome, expected) in [
        (json!({"state":"ready"}), RetainedFrontierOutcome::Ready {}),
        (
            json!({"state":"failed","reason":"unavailable"}),
            RetainedFrontierOutcome::Failed {
                reason: RetainedFrontierFailure::Unavailable,
            },
        ),
    ] {
        let value = json!({"frontier":"11111111111111111111111111111111",
            "snapshot":"22222222222222222222222222222222","threadId":"projection-data-only",
            "outcome":outcome});
        let decoded: RetainedFrontierNotification = serde_json::from_value(value.clone())
            .unwrap_or_else(|error| panic!("exact private codec fixture: {error}"));
        assert_eq!(decoded.outcome, expected);
        assert_eq!(
            serde_json::to_value(decoded).unwrap_or_else(|error| panic!("codec: {error}")),
            value
        );
    }
}

#[test]
fn old_missing_outcome_unknown_state_and_private_error_payloads_are_rejected() {
    let base = json!({"frontier":"11111111111111111111111111111111",
        "snapshot":"22222222222222222222222222222222","threadId":"projection-data-only"});
    assert!(serde_json::from_value::<RetainedFrontierNotification>(base.clone()).is_err());
    for outcome in [
        json!({"state":"unknown"}),
        json!({"state":"failed"}),
        json!({"state":"failed","reason":"raw-error"}),
        json!({"state":"failed","reason":"unavailable","message":"must-not-export"}),
        json!({"state":"ready","reason":"unavailable"}),
        serde_json::Value::Null,
    ] {
        let mut value = base.clone();
        value["outcome"] = outcome;
        assert!(serde_json::from_value::<RetainedFrontierNotification>(value).is_err());
    }
}
