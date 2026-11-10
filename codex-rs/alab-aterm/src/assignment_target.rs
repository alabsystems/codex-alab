// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private policy metadata, pinned to the native instance which prepared it.

use super::*;
use codex_alab_core::assignment_service::Target;
use codex_alab_core::assignment_service::TargetPolicy;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preview {
    schema: u32,
    operation: String,
    session: Session,
    pool_revision: String,
    expected_assignment: u64,
    target_ref: String,
    auth_mode: String,
    account: String,
    user: String,
    source_grant: String,
    target_grant: String,
    source_provider: String,
    target_provider: String,
    source_fedramp: bool,
    target_fedramp: bool,
    target_refresh_endpoint: String,
}

fn parse(line: &str, operation: &str, sid: &str, birth: &str, epoch: &str) -> io::Result<Target> {
    if line.len() > 16 * 1024 || epoch.is_empty() {
        return Err(unavailable());
    }
    let body = line
        .strip_prefix("OK codex-accounts-change assignment-target ")
        .ok_or_else(unavailable)?;
    let preview: Preview = serde_json::from_str(body).map_err(|_| unavailable())?;
    if preview.schema != 1
        || preview.operation != operation
        || preview.auth_mode != "chatgpt"
        || !valid_session(&preview.session, sid, birth)
        || preview.session.epoch != epoch
    {
        return Err(unavailable());
    }
    let target = Target {
        operation: preview.operation,
        pool_revision: preview.pool_revision,
        expected_assignment: preview.expected_assignment,
        target_ref: preview.target_ref,
        account: preview.account,
        user: preview.user,
        source_grant: preview.source_grant,
        target_grant: preview.target_grant,
        policy: TargetPolicy {
            source_provider: preview.source_provider,
            target_provider: preview.target_provider,
            source_fedramp: preview.source_fedramp,
            target_fedramp: preview.target_fedramp,
            target_refresh_endpoint: preview.target_refresh_endpoint,
        },
    };
    target.validate(operation)?;
    Ok(target)
}

/// The epoch must come from this host's actual prepared-operation channel.
/// This read installs no Host and cannot authorize credential access or assignment.
pub async fn target_at(
    endpoint: &Endpoint,
    operation: &str,
    birth: &str,
    epoch: &str,
) -> io::Result<Target> {
    if !crate::locate::valid_sid(&endpoint.sid)
        || !codex_alab_core::managed_auth::valid_hex(operation, 32)
        || birth.is_empty()
        || birth.len() > 128
        || birth.chars().any(char::is_control)
        || epoch.is_empty()
        || epoch.len() > 256
        || epoch.chars().any(char::is_control)
    {
        return Err(unavailable());
    }
    let request = format!(
        "@{} codex-accounts-change assignment-target schema=1 operation={operation}",
        endpoint.sid
    );
    let opened = conn::open(&endpoint.sock, CALL_TIMEOUT)
        .await
        .map_err(|_| unavailable())?;
    let connection = Conn::spawn(opened, CALL_TIMEOUT);
    let result = connection
        .request(&request, Framing::Status)
        .await
        .map_err(|_| unavailable())
        .and_then(|reply| parse(&reply.status, operation, &endpoint.sid, birth, epoch));
    connection.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn preview() -> Value {
        json!({
            "schema":1,"operation":"a".repeat(32),
            "session":{"sid":"fixture","process_pid":std::process::id(),"process_birth":"birth","epoch":"original"},
            "pool_revision":"b".repeat(64),"expected_assignment":7,"target_ref":"c".repeat(64),
            "auth_mode":"chatgpt","account":"d".repeat(64),"user":"e".repeat(64),
            "source_grant":"1".repeat(64),"target_grant":"2".repeat(64),
            "source_provider":"3".repeat(64),"target_provider":"3".repeat(64),
            "source_fedramp":false,"target_fedramp":false,
            "target_refresh_endpoint":"https://auth.example/oauth/token"
        })
    }
    fn line(value: &Value) -> String {
        format!("OK codex-accounts-change assignment-target {value}")
    }
    fn read(value: &Value) -> io::Result<Target> {
        parse(
            &line(value),
            &"a".repeat(32),
            "fixture",
            "birth",
            "original",
        )
    }

    #[test]
    fn preview_requires_complete_private_prepared_metadata() {
        let original = preview();
        let target = read(&original).unwrap();
        assert_eq!(target.expected_assignment, 7);
        assert_eq!(target.target_ref, "c".repeat(64));
        let mut startup = original.clone();
        startup["expected_assignment"] = json!(0);
        assert_eq!(read(&startup).unwrap().expected_assignment, 0);
        for key in original.as_object().unwrap().keys() {
            let mut changed = original.clone();
            changed.as_object_mut().unwrap().remove(key);
            assert!(read(&changed).is_err(), "missing {key}");
        }
        for (pointer, value) in [
            ("/unknown", json!(true)),
            ("/schema", json!(2)),
            ("/operation", json!("9".repeat(32))),
            ("/expected_assignment", json!(-1)),
            ("/auth_mode", json!("api_key")),
            ("/target_provider", json!("4".repeat(64))),
            ("/target_fedramp", json!(true)),
            ("/source_fedramp", json!("false")),
            ("/session/epoch", json!("replacement")),
            ("/session/process_pid", json!(0)),
            ("/session/process_birth", json!("other")),
            ("/session/sid", json!("other")),
        ] {
            let mut changed = original.clone();
            if pointer == "/unknown" {
                changed["unknown"] = value;
            } else {
                *changed.pointer_mut(pointer).unwrap() = value;
            }
            assert!(read(&changed).is_err(), "changed {pointer}");
        }
    }

    #[test]
    fn preview_refuses_duplicate_fields_and_oversized_frames() {
        let original = line(&preview());
        let duplicate = original.replacen("{", "{\"schema\":1,", 1);
        assert!(parse(&duplicate, &"a".repeat(32), "fixture", "birth", "original").is_err());
        let large = format!("{original}{}", " ".repeat(16 * 1024));
        assert!(parse(&large, &"a".repeat(32), "fixture", "birth", "original").is_err());
    }
}
