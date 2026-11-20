// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::process_responses_event;
use crate::common::ResponseEvent;
use serde_json::json;

#[test]
fn provider_interruption_is_distinct_from_a_completed_continuation() {
    let interrupted = json!({
        "type": "response.incomplete",
        "response": {
            "id": "interrupted",
            "incomplete_details": {"reason": "interrupted"}
        }
    });
    let completed = json!({
        "type": "response.completed",
        "response": {"id": "continuation", "end_turn": false}
    });
    for (wire, expected_interruption) in [(interrupted, true), (completed, false)] {
        let event = serde_json::from_value(wire).expect("valid response event");
        let Ok(Some(ResponseEvent::Completed {
            interrupted,
            end_turn,
            ..
        })) = process_responses_event(event)
        else {
            panic!("expected terminal response event")
        };
        assert_eq!(interrupted, expected_interruption);
        assert_eq!(end_turn, Some(false));
    }
}
