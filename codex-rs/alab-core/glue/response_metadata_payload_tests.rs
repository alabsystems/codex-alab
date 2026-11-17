// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn ordinary_typed_serialization_has_identical_bytes_and_no_metadata()
-> Result<(), serde_json::Error> {
    let original = crate::ThreadArchiveResponse {};
    let expected = serde_json::to_vec(&original)?;
    let payload = ClientResponsePayload::ThreadArchive(original);
    assert_eq!(serde_json::to_vec(&payload)?, expected);
    Ok(())
}

#[test]
fn typed_response_remains_typed_for_analytics_and_metadata_is_namespaced()
-> Result<(), serde_json::Error> {
    let payload = ClientResponsePayload::ThreadArchive(crate::ThreadArchiveResponse {});
    let wrapped = decorate(
        payload,
        "alab.observation",
        serde_json::json!({"token":"data"}),
    )?;
    let value = serde_json::to_value(&wrapped)?;
    assert_eq!(
        value,
        serde_json::json!({"_meta":{"alab.observation":{"token":"data"}}})
    );
    assert!(matches!(
        wrapped.into_client_response(RequestId::Integer(7)),
        Some(ClientResponse::ThreadArchive {
            request_id: RequestId::Integer(7),
            ..
        })
    ));
    Ok(())
}

#[test]
fn multiple_owners_merge_without_overwrite_and_duplicate_is_rejected()
-> Result<(), serde_json::Error> {
    let payload = ClientResponsePayload::ThreadArchive(crate::ThreadArchiveResponse {});
    let wrapped = decorate(payload, "alab.first", serde_json::json!(1))?;
    let wrapped = decorate(wrapped, "alab.second", serde_json::json!(2))?;
    assert_eq!(
        serde_json::to_value(&wrapped)?,
        serde_json::json!({"_meta":{"alab.first":1,"alab.second":2}})
    );
    assert!(decorate(wrapped, "alab.first", serde_json::json!(3)).is_err());
    assert!(acknowledgment("foreign.key", serde_json::json!(true)).is_err());
    Ok(())
}

#[test]
fn acknowledgment_has_no_fake_typed_upstream_response() -> Result<(), serde_json::Error> {
    let payload = acknowledgment(
        "alab.retainedProjection",
        serde_json::json!({"snapshot":"data"}),
    )?;
    assert_eq!(
        serde_json::to_value(&payload)?,
        serde_json::json!({"_meta":{"alab.retainedProjection":{"snapshot":"data"}}})
    );
    assert!(
        payload
            .into_client_response(RequestId::String("actual-client-request".into()))
            .is_none()
    );
    Ok(())
}

#[test]
fn actual_serialization_merge_preserves_unowned_meta_and_rejects_collisions()
-> Result<(), Box<dyn std::error::Error>> {
    let entries = BTreeMap::from([(
        "alab.retainedProjection".into(),
        serde_json::json!({"snapshot":"data"}),
    )]);
    let original =
        serde_json::json!({"thread":{"id":"same"},"_meta":{"foreign.owner":{"untouched":true}}});
    let actual = merge_metadata(original, &entries)?;
    assert_eq!(actual["thread"], serde_json::json!({"id":"same"}));
    assert_eq!(
        actual["_meta"]["foreign.owner"],
        serde_json::json!({"untouched":true})
    );
    assert_eq!(
        actual["_meta"]["alab.retainedProjection"],
        serde_json::json!({"snapshot":"data"})
    );
    assert!(
        merge_metadata(
            serde_json::json!({"_meta":{"alab.retainedProjection":null}}),
            &entries
        )
        .is_err()
    );
    assert!(merge_metadata(serde_json::json!({"_meta":false}), &entries).is_err());
    assert!(merge_metadata(serde_json::json!([]), &entries).is_err());
    Ok(())
}
