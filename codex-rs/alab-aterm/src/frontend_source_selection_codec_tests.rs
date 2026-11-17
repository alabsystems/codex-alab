// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Private codec syntax only. No Controller, opaque SDK receipt, Native Go,
//! Source ticket, UI observation or release is constructed by these controls.
use super::*;
fn syntax() -> serde_json::Value {
    serde_json::json!({"schema":1,"request":"a".repeat(32),"operation":"b".repeat(32),
        "member":"c".repeat(32),"session":{"sid":"test","process_pid":1,"process_birth":"data","epoch":"d".repeat(32)},
        "service":"e".repeat(32),"source_operation":"f".repeat(32),"selection":"1".repeat(32),
        "frontier":"2".repeat(32),"state":"selection-observed","human":3})
}
#[test]
fn initial_source_native_reply_rejects_go_key_even_null() -> io::Result<()> {
    let baseline = serde_json::to_string(&syntax()).map_err(|_| unavailable())?;
    assert!(NativeReply::decode(&baseline, false, false).is_ok()); // Private syntax only.
    for go in [
        serde_json::Value::Null,
        serde_json::Value::String("a".repeat(32)),
    ] {
        let mut value = syntax();
        value["go"] = go;
        let body = serde_json::to_string(&value).map_err(|_| unavailable())?;
        assert!(NativeReply::decode(&body, false, false).is_err());
    }
    Ok(())
}
#[test]
fn source_commit_requires_nonnull_replacement_app_owner_and_rejects_extra_fields() -> io::Result<()>
{
    let mut baseline = syntax();
    baseline["go"] = serde_json::json!("a".repeat(32));
    baseline["app_owner"] = serde_json::json!("b".repeat(32));
    assert!(
        NativeReply::decode(
            &serde_json::to_string(&baseline).map_err(|_| unavailable())?,
            true,
            true
        )
        .is_ok()
    );
    for owner in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!(true)),
    ] {
        let mut value = syntax();
        value["go"] = serde_json::json!("a".repeat(32));
        if let Some(owner) = owner {
            value["app_owner"] = owner;
        }
        let body = serde_json::to_string(&value).map_err(|_| unavailable())?;
        assert!(NativeReply::decode(&body, true, true).is_err());
    }
    let mut value = syntax();
    value["current"] = serde_json::json!(true);
    let body = serde_json::to_string(&value).map_err(|_| unavailable())?;
    assert!(NativeReply::decode(&body, false, false).is_err());
    Ok(())
}
#[test]
fn native_source_replies_reject_duplicate_typed_fields() -> io::Result<()> {
    for (replacement, commit) in [(false, false), (true, false), (true, true)] {
        let mut value = syntax();
        if replacement {
            value["go"] = serde_json::json!("a".repeat(32));
        }
        if commit {
            value["app_owner"] = serde_json::json!("b".repeat(32));
        }
        let body = serde_json::to_string(&value).map_err(|_| unavailable())?;
        assert!(NativeReply::decode(&body, replacement, commit).is_ok());
        let duplicate = body.replacen('{', "{\"schema\":1,", 1);
        assert!(NativeReply::decode(&duplicate, replacement, commit).is_err());
    }
    Ok(())
}
