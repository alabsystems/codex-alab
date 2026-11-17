// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower local custody/observation controls, not callback/SDK grant fixtures.
use super::*;

#[test]
fn original_write_success_is_unconfirmed_and_cannot_resubmit() {
    let phase = Arc::new(AtomicU8::new(QUEUED));
    begin(&phase).unwrap().observed(&Ok(()));
    assert_eq!(phase.load(Ordering::Acquire), WRITTEN);
    assert!(begin(&phase).is_err());
    // Neither WRITTEN nor UNKNOWN is a Source terminal/accepted state.
}

#[test]
fn dropped_or_failed_actual_attempt_stays_uncertain_without_retry() {
    let phase = Arc::new(AtomicU8::new(QUEUED));
    drop(begin(&phase).unwrap());
    assert_eq!(phase.load(Ordering::Acquire), UNKNOWN);
    assert!(begin(&phase).is_err());
    let phase = Arc::new(AtomicU8::new(QUEUED));
    begin(&phase)
        .unwrap()
        .observed(&Err(io::Error::other("actual writer failed")));
    assert_eq!(phase.load(Ordering::Acquire), UNKNOWN);
    assert!(begin(&phase).is_err());
}

#[test]
fn saved_human_response_is_closed_data_without_a_send_constructor() {
    let data = HumanReplyData {
        schema: 1,
        thread_id: "00000000-0000-0000-0000-000000000001".into(),
        request_id: AppServerRequestId::Integer(7),
        result: serde_json::json!({"answers":{"q":["original answer"]}}),
        observation: HumanReplyState::Unconfirmed {},
    };
    let mut wire = serde_json::to_value(&data).unwrap();
    wire["observation"]["accepted"] = serde_json::json!(true);
    assert!(serde_json::from_value::<HumanReplyData>(wire).is_err());
    let value: HumanReplyData =
        serde_json::from_value(serde_json::to_value(&data).unwrap()).unwrap();
    assert!(value.validate_data().is_ok());
    assert_eq!(value.request_id_data(), &AppServerRequestId::Integer(7));
}
