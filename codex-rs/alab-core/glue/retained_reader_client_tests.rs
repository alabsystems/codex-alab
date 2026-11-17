// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Closed response codec only; these values are not a Native/backend witness.
use super::*;
#[test]
fn managed_snapshot_correlation_and_foreign_metadata_preserve_original_config_result()
-> Result<(), Box<dyn std::error::Error>> {
    let selected = metadata::Selection {
        service: "a".repeat(32),
        operation: "b".repeat(32),
        kind: metadata::ReadKind::Managed,
    };
    let valid = serde_json::json!({"config":{},"origins":{},"_meta":{"foreign.owner":{"kept":null},
        "alab.retainedReaderSnapshot":{"schema":1,"service":selected.service,"operation":selected.operation,"kind":"managed",
            "account":{"authenticated":false,"chatgptAuth":false,"managedChatgpt":false,"fedramp":false,"account":null,"user":null},
            "cloud":{"outcome":{"state":"loaded","bundle":null},"configServer":null}}}});
    let actual = decode_snapshot(valid.clone(), &selected)?;
    assert!(actual.response.origins.is_empty());
    assert!(
        actual
            .snapshot
            .cloud
            .as_ref()
            .ok_or("managed data missing")?
            .successful_parts()?
            .0
            .is_none()
    );
    for (field, value) in [
        ("schema", serde_json::json!(2)),
        ("service", serde_json::json!("c".repeat(32))),
        ("operation", serde_json::json!("d".repeat(32))),
        ("ready", serde_json::json!(true)),
    ] {
        let mut raw = valid.clone();
        raw["_meta"][metadata::RESULT_KEY][field] = value;
        assert!(decode_snapshot(raw, &selected).is_err());
    }
    Ok(())
}

#[test]
fn account_result_cannot_substitute_a_managed_policy_result_or_a_native_permission()
-> Result<(), Box<dyn std::error::Error>> {
    let selected = metadata::Selection {
        service: "a".repeat(32),
        operation: "b".repeat(32),
        kind: metadata::ReadKind::Account,
    };
    let valid = serde_json::json!({"config":{},"origins":{},"_meta":{
        "alab.retainedReaderSnapshot":{"schema":1,"service":selected.service,"operation":selected.operation,
            "kind":"account","cloud":null,"account":{"authenticated":false,"chatgptAuth":false,
                "managedChatgpt":false,"fedramp":false,"account":null,"user":null}}}});
    let actual = decode_snapshot(valid.clone(), &selected)?;
    assert_eq!(
        actual.snapshot.account.preference_user().as_deref(),
        Some("local")
    );
    for field in ["nativePermission", "ready", "readerRetired"] {
        let mut raw = valid.clone();
        raw["_meta"][metadata::RESULT_KEY]["account"][field] = serde_json::json!(true);
        assert!(decode_snapshot(raw, &selected).is_err());
    }
    let mut raw = valid.clone();
    raw["_meta"][metadata::RESULT_KEY]["kind"] = serde_json::json!("managed");
    assert!(decode_snapshot(raw, &selected).is_err());
    let mut raw = valid;
    raw["_meta"][metadata::RESULT_KEY]["account"]["managedChatgpt"] = serde_json::json!(true);
    assert!(decode_snapshot(raw, &selected).is_err());
    Ok(())
}
