// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! DATA-only codec/checkpoint controls. No Source capture or Native grant.
use super::*;
#[test]
fn checkpoint_before_send_preserves_same_operation_and_exact_request_ids()
-> Result<(), TypedRequestError> {
    let operation = CallbackCaptureOperation::new(
        "aabbccddeeff00112233445566778899",
        vec![
            RequestId::Integer(17),
            RequestId::String("human-form".into()),
        ],
    )?;
    let before = operation.checkpoint()?;
    let restored = CallbackCaptureOperation::restore_checkpoint(before.clone())?;
    assert_eq!(restored.checkpoint()?, before);
    Ok(())
}
#[test]
fn checkpoint_rejects_descriptor_substitution_and_duplicate_request_ids() {
    let base = serde_json::json!({"schema":1,"service":"aabbccddeeff00112233445566778899","operation":"00000000000000000000000000000001","ids":[17],"descriptor":{"epoch":"00000000000000000000000000000002","ordinal":1,"operation":"00000000000000000000000000000001","ids":[18],"proof":"0".repeat(64)}});
    assert!(CallbackCaptureOperation::restore_checkpoint(base).is_err());
    assert!(
        CallbackCaptureOperation::new(
            "aabbccddeeff00112233445566778899",
            vec![RequestId::Integer(1), RequestId::Integer(1)]
        )
        .is_err()
    );
}
#[test]
fn closed_outcome_codec_rejects_unknown_state_extra_reason_and_fields() {
    for value in [
        serde_json::json!({"state":"ready"}),
        serde_json::json!({"state":"response","reason":"assumed"}),
        serde_json::json!({"state":"pending","consumed":true}),
    ] {
        assert!(serde_json::from_value::<Outcome>(value).is_err());
    }
}
