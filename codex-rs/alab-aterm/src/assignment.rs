// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Authenticated healthy assignment transport. Errors never include wire bodies.

use crate::CALL_TIMEOUT;
use crate::Endpoint;
use crate::accounts::Session;
use crate::accounts::valid_session;
use crate::conn::Conn;
use crate::conn::Framing;
use crate::conn::{self};
use codex_alab_core::assignment_service::Command;
use codex_alab_core::assignment_service::Mode;
use codex_alab_core::assignment_service::Phase;
use codex_alab_core::assignment_service::Reason;
use codex_alab_core::assignment_service::Status;
use serde::Deserialize;
use serde_json::Value;
use std::io;

#[path = "assignment_target.rs"]
mod target;
pub use target::target_at;

fn unavailable() -> io::Error {
    io::Error::other("Native account assignment must be reconciled.")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    schema: u32,
    operation: String,
    native_intent: String,
    session: Session,
    phase: String,
    boundary: Value,
    assignment_generation: Value,
    credential_generation: Value,
    target_ref: Value,
    selection_mode: Value,
    reason: Value,
}
fn text(value: Value) -> io::Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value)),
        _ => Err(unavailable()),
    }
}
fn generation(value: Value) -> io::Result<Option<u64>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .filter(|value| *value > 0)
        .map(Some)
        .ok_or_else(unavailable)
}
fn parse(line: &str, operation: &str, sid: &str, birth: &str) -> io::Result<Status> {
    if line.len() > 16 * 1024 {
        return Err(unavailable());
    }
    let body = line
        .strip_prefix("OK codex-accounts-change assignment ")
        .ok_or_else(unavailable)?;
    let reply: Reply = serde_json::from_str(body).map_err(|_| unavailable())?;
    if reply.schema != 1
        || reply.operation != operation
        || reply.native_intent != operation
        || !valid_session(&reply.session, sid, birth)
    {
        return Err(unavailable());
    }
    let phase = match reply.phase.as_str() {
        "waiting" => Phase::Waiting,
        "prepared" => Phase::Prepared,
        "installing" => Phase::Installing,
        "awaiting_ack" => Phase::AwaitingAck,
        "applied" => Phase::Applied,
        "cancelled" => Phase::Cancelled,
        "refused" => Phase::Refused,
        "reconciling" => Phase::Reconciling,
        _ => return Err(unavailable()),
    };
    let mode = match text(reply.selection_mode)?.as_deref() {
        None => None,
        Some("manual") => Some(Mode::Manual),
        Some("auto") => Some(Mode::Auto),
        _ => return Err(unavailable()),
    };
    let reason = text(reply.reason)?
        .map(|reason| Reason::parse(&reason).ok_or_else(unavailable))
        .transpose()?;
    let status = Status {
        operation: reply.operation,
        phase,
        boundary: text(reply.boundary)?,
        assignment_generation: generation(reply.assignment_generation)?,
        credential_generation: generation(reply.credential_generation)?,
        target: text(reply.target_ref)?,
        mode,
        reason,
    };
    status.validate(operation).map_err(|_| unavailable())?;
    Ok(status)
}

fn wire(command: &Command) -> io::Result<String> {
    command.validate().map_err(|_| unavailable())?;
    let operation = command.operation();
    Ok(match command {
        Command::Prepare {
            pool_revision,
            expected_assignment,
            selection,
            ..
        } => format!(
            "assignment-prepare schema=1 operation={operation} pool_revision={pool_revision} expected_assignment={expected_assignment} selection={}",
            selection.as_deref().unwrap_or("auto")
        ),
        Command::Commit {
            boundary,
            source_credential_generation,
            generations,
            ..
        } => format!(
            "assignment-commit schema=1 operation={operation} boundary={boundary} source_credential_generation={source_credential_generation} auth_generation={} owner_generation={} source_generation={} login_generation={}",
            generations.auth, generations.owner, generations.source, generations.login
        ),
        Command::Poll { .. } => format!("assignment-poll schema=1 operation={operation}"),
        Command::Cancel { .. } => format!("assignment-cancel schema=1 operation={operation}"),
        Command::Ack {
            boundary, binding, ..
        } => format!(
            "assignment-ack schema=1 operation={operation} boundary={boundary} assignment_generation={} credential_generation={} digest={} account={} user={} auth_generation={} owner_generation={} source_generation={} login_generation={}",
            binding.assignment_generation,
            binding.credential_generation,
            binding.digest,
            binding.account,
            binding.user,
            binding.generations.auth,
            binding.generations.owner,
            binding.generations.source,
            binding.generations.login
        ),
    })
}

pub async fn command_at(endpoint: &Endpoint, command: &Command, birth: &str) -> io::Result<Status> {
    if !crate::locate::valid_sid(&endpoint.sid)
        || birth.is_empty()
        || birth.len() > 128
        || birth.chars().any(char::is_control)
    {
        return Err(unavailable());
    }
    let request = format!("@{} codex-accounts-change {}", endpoint.sid, wire(command)?);
    let opened = conn::open(&endpoint.sock, CALL_TIMEOUT)
        .await
        .map_err(|_| unavailable())?;
    let connection = Conn::spawn(opened, CALL_TIMEOUT);
    let result = connection
        .request(&request, Framing::Status)
        .await
        .map_err(|_| unavailable())
        .and_then(|reply| parse(&reply.status, command.operation(), &endpoint.sid, birth));
    connection.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const BIRTH: &str = "Fri Nov 6 12:34:56 2026";
    fn value(operation: &str) -> Value {
        json!({"schema":1,"operation":operation,"native_intent":operation,
            "session":{"sid":"fixture","process_pid":std::process::id(),"process_birth":BIRTH,"epoch":"host"},
            "phase":"prepared","boundary":null,"assignment_generation":null,"credential_generation":null,
            "target_ref":"b".repeat(64),"selection_mode":"manual","reason":null})
    }
    fn line(value: &Value) -> String {
        format!("OK codex-accounts-change assignment {value}")
    }
    #[test]
    fn assignment_status_rejects_missing_unknown_unbound_and_impossible_fields() {
        let operation = "a".repeat(32);
        let original = value(&operation);
        assert_eq!(
            parse(&line(&original), &operation, "fixture", BIRTH)
                .unwrap()
                .phase,
            Phase::Prepared
        );
        for key in original.as_object().unwrap().keys() {
            let mut malformed = original.clone();
            malformed.as_object_mut().unwrap().remove(key);
            assert!(
                parse(&line(&malformed), &operation, "fixture", BIRTH).is_err(),
                "missing {key}"
            );
        }
        for (pointer, replacement) in [
            ("/unknown", json!(true)),
            ("/schema", json!(2)),
            ("/operation", json!("c".repeat(32))),
            ("/native_intent", json!("c".repeat(32))),
            ("/session/process_pid", json!(0)),
            ("/session/process_birth", json!("other")),
            ("/phase", json!("done")),
            ("/phase", json!("applied")),
            ("/assignment_generation", json!(1)),
            ("/credential_generation", json!(0)),
            ("/target_ref", Value::Null),
            ("/selection_mode", Value::Null),
            ("/selection_mode", json!("global")),
            ("/reason", json!("private-token-canary")),
        ] {
            let mut malformed = original.clone();
            if pointer == "/unknown" {
                malformed["unknown"] = replacement;
            } else {
                *malformed.pointer_mut(pointer).unwrap() = replacement;
            }
            let error = parse(&line(&malformed), &operation, "fixture", BIRTH)
                .unwrap_err()
                .to_string();
            assert!(!error.contains("private-token-canary"));
        }
        assert!(
            parse(
                &format!("{}{}", line(&original), " ".repeat(16 * 1024)),
                &operation,
                "fixture",
                BIRTH
            )
            .is_err()
        );
        let duplicate = line(&original).replacen("{", "{\"schema\":1,", 1);
        assert!(parse(&duplicate, &operation, "fixture", BIRTH).is_err());
        assert!(parse(&line(&original), &operation, "foreign", BIRTH).is_err());
        let mut waiting = original.clone();
        waiting["phase"] = json!("waiting");
        waiting["selection_mode"] = Value::Null;
        waiting["target_ref"] = Value::Null;
        assert_eq!(
            parse(&line(&waiting), &operation, "fixture", BIRTH)
                .unwrap()
                .phase,
            Phase::Waiting
        );
        waiting["phase"] = json!("cancelled");
        assert_eq!(
            parse(&line(&waiting), &operation, "fixture", BIRTH)
                .unwrap()
                .phase,
            Phase::Cancelled
        );
        waiting["target_ref"] = json!("b".repeat(64));
        assert!(parse(&line(&waiting), &operation, "fixture", BIRTH).is_err());
        let mut applied = original;
        applied["phase"] = json!("applied");
        applied["boundary"] = json!("c".repeat(32));
        applied["assignment_generation"] = json!(2);
        applied["credential_generation"] = json!(3);
        assert_eq!(
            parse(&line(&applied), &operation, "fixture", BIRTH)
                .unwrap()
                .phase,
            Phase::Applied
        );
    }
    #[tokio::test]
    async fn assignment_transport_uses_authenticated_same_operation_without_supervision()
    -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let fake = crate::fake::FakeAterm::start(dir.path(), crate::fake::Mode::Current).await?;
        fake.set_effective(false, "presentation off");
        let command = Command::Prepare {
            operation: "a".repeat(32),
            pool_revision: "d".repeat(64),
            expected_assignment: 0,
            selection: Some("b".repeat(64)),
        };
        let request = format!("@{} codex-accounts-change {}", fake.sid(), wire(&command)?);
        let mut reply = value(command.operation());
        reply["session"]["sid"] = json!(fake.sid());
        fake.reply_once(&request, &format!("{}\n", line(&reply)));
        let status = command_at(&fake.endpoint(), &command, BIRTH).await?;
        assert_eq!(status.phase, Phase::Prepared);
        assert!(
            fake.requests()
                .iter()
                .all(|request| !request.contains("supervisor")
                    && !request.contains("harness policy"))
        );
        Ok(())
    }
}
