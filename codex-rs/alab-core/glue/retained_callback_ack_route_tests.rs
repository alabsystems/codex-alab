// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Source identity and closed DATA policy only, no Native/Core/configured
//! resource is fabricated. Actual route execution needs the paired fixture.
use super::*;
#[test]
fn live_cleanup_cannot_replace_original_resource_with_same_looking_data() {
    let original = Arc::new("same source data");
    let alias = original.clone();
    let replacement = Arc::new("same source data");
    assert!(same_original(&original, alias.as_ref()));
    assert!(!same_original(&original, replacement.as_ref()));
}
#[test]
fn configured_endpoint_codec_has_no_fabricated_core_and_refuses_pending() {
    let descriptor = serde_json::json!({"epoch":"1".repeat(32),"ordinal":1,"operation":"2".repeat(32),"ids":[7],"proof":"3".repeat(64)});
    let base = serde_json::json!({"descriptor":descriptor,"projection":{"frontier":"4".repeat(32),"snapshot":"5".repeat(32),"runtime":"6".repeat(32),"version":0},"content":{"state":"current","facts":[{"requestId":7,"threadId":"actual old thread","outcome":{"state":"response"}}]},"proof":"7".repeat(64)});
    let closed = serde_json::from_value::<data::ConfiguredObservation>(base.clone());
    assert!(closed.is_ok_and(|value| value.valid()));
    let mut pending = base.clone();
    pending["content"]["facts"][0]["outcome"] = serde_json::json!({"state":"pending"});
    assert!(
        !serde_json::from_value::<data::ConfiguredObservation>(pending)
            .is_ok_and(|value| value.valid())
    );
    let mut fake_core = base;
    fake_core["projection"]["threadId"] = serde_json::json!("invented");
    assert!(serde_json::from_value::<data::ConfiguredObservation>(fake_core).is_err());
}
#[test]
fn closed_subset_never_omits_pending_unknown_or_missing_endpoint_data() {
    let facts = vec![
        data::Fact {
            request_id: RequestId::Integer(1),
            thread_id: Some("original background Core".into()),
            outcome: data::Outcome::Response {},
        },
        data::Fact {
            request_id: RequestId::Integer(2),
            thread_id: Some("original background Core".into()),
            outcome: data::Outcome::Pending {},
        },
        data::Fact {
            request_id: RequestId::Integer(3),
            thread_id: Some("original background Core".into()),
            outcome: data::Outcome::Unknown {},
        },
    ];
    let closed = select_closed(&facts, &[RequestId::Integer(1)]);
    assert!(closed.is_ok_and(|selected| selected.len() == 1
        && selected[0].thread_id.as_deref() == Some("original background Core")));
    for ids in [
        vec![RequestId::Integer(1), RequestId::Integer(2)],
        vec![RequestId::Integer(3)],
        vec![RequestId::Integer(4)],
    ] {
        assert!(select_closed(&facts, &ids).is_err());
    }
}
