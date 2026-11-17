// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
#[test]
fn configured_tail_requires_real_slot_correlation_and_closed_terminal_outcome() {
    for missing in ["runtime", "version", "outcome"] {
        let mut value = serde_json::json!({"frontier":"a".repeat(32),"snapshot":"b".repeat(32),
            "runtime":"c".repeat(32),"version":0,"outcome":{"state":"ready"}});
        if let Some(object) = value.as_object_mut() {
            object.remove(missing);
        }
        assert!(serde_json::from_value::<RetainedConfiguredNotification>(value).is_err());
    }
    let mut fake_core = serde_json::json!({"frontier":"a".repeat(32),"snapshot":"b".repeat(32),
        "runtime":"c".repeat(32),"version":0,"outcome":{"state":"ready"}});
    fake_core["threadId"] = serde_json::json!("invented-first-thread");
    assert!(serde_json::from_value::<RetainedConfiguredNotification>(fake_core).is_err());
}
