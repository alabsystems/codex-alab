// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Frozen policy DATA only. No source/Native reader or role is constructed.
use super::*;
use codex_config::CloudConfigBundleSnapshot;
#[tokio::test]
async fn actual_loaded_and_failed_snapshot_outcomes_survive_frozen_loader_without_empty_fallback()
-> Result<(), Box<dyn std::error::Error>> {
    let absent = CloudConfigBundleSnapshot {
        bundle: Ok(None),
        binding: None,
        config_server: None,
    };
    let data = ManagedCloudData::from_actual(&absent);
    let data: ManagedCloudData = serde_json::from_slice(&serde_json::to_vec(&data)?)?;
    assert_eq!(data.frozen_loader().get().await?, None);
    for (code, status) in [
        (CloudConfigBundleLoadErrorCode::Auth, Some(401)),
        (CloudConfigBundleLoadErrorCode::Timeout, None),
        (CloudConfigBundleLoadErrorCode::RequestFailed, Some(503)),
        (CloudConfigBundleLoadErrorCode::InvalidBundle, None),
        (CloudConfigBundleLoadErrorCode::Internal, None),
    ] {
        let actual = CloudConfigBundleSnapshot {
            bundle: Err(CloudConfigBundleLoadError::new(
                code,
                status,
                "original managed loader failure",
            )),
            binding: None,
            config_server: None,
        };
        let data: ManagedCloudData = serde_json::from_slice(&serde_json::to_vec(
            &ManagedCloudData::from_actual(&actual),
        )?)?;
        let result = data.frozen_loader().get().await;
        let error = match result {
            Err(error) => error,
            Ok(_) => return Err("failure became an empty policy bundle".into()),
        };
        assert_eq!(error.code(), code);
        assert_eq!(error.status_code(), status);
        assert_eq!(error.to_string(), "original managed loader failure");
        assert!(data.successful_parts().is_err());
    }
    Ok(())
}
#[test]
fn snapshot_requires_explicit_closed_outcome_and_nullable_fields() {
    for raw in [
        serde_json::json!({"outcome":{"state":"loaded"},"configServer":null}),
        serde_json::json!({"outcome":{"state":"loaded","bundle":null}}),
        serde_json::json!({"outcome":{"state":"failed","code":"auth","message":"failed"},"configServer":null}),
        serde_json::json!({"outcome":{"state":"loaded","bundle":null,"nativePermission":true},"configServer":null}),
    ] {
        assert!(serde_json::from_value::<ManagedCloudData>(raw).is_err());
    }
}
