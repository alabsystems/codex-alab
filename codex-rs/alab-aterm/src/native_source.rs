// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private source receipts on a dedicated authenticated connection. No source
//! path, native reply, or owner diagnostic is returned in an error.

use crate::CALL_TIMEOUT;
use crate::accounts::Session;
use crate::accounts::valid_session;
use crate::conn::Conn;
use crate::conn::Framing;
use crate::conn::{self};
use crate::locate::Endpoint;
use codex_alab_core::managed_auth::Revision;
use codex_alab_core::managed_auth::unavailable;
use codex_alab_core::managed_auth::valid_hex;
use codex_alab_core::native_source::Source;
use serde::Deserialize;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

const MAX_FRAME: usize = 16 * 1024;
const WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Waiting {
    schema: u32,
    operation: String,
    session: Session,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ready {
    schema: u32,
    operation: String,
    session: Session,
    slot: String,
    path: String,
    account: String,
    user: String,
    credential_generation: u64,
    digest: String,
    assignment_generation: u64,
    // Value deliberately requires the field even for an explicit bootstrap null.
    native_intent: serde_json::Value,
}

enum Outcome {
    Waiting,
    Ready(Source),
}

fn parse(line: &str, operation: &str, sid: &str, birth: &str) -> io::Result<Outcome> {
    if line.len() > MAX_FRAME {
        return Err(unavailable());
    }
    if let Some(body) = line.strip_prefix("OK codex-auth source-waiting ") {
        let reply: Waiting = serde_json::from_str(body).map_err(|_| unavailable())?;
        if reply.schema != 1
            || reply.operation != operation
            || !valid_session(&reply.session, sid, birth)
        {
            return Err(unavailable());
        }
        return Ok(Outcome::Waiting);
    }
    let body = line
        .strip_prefix("OK codex-auth source ")
        .ok_or_else(unavailable)?;
    let reply: Ready = serde_json::from_str(body).map_err(|_| unavailable())?;
    Ok(Outcome::Ready(reply.into_source(operation, sid, birth)?))
}

impl Ready {
    pub(crate) fn session(&self) -> &Session {
        &self.session
    }
    pub(crate) fn into_source(self, operation: &str, sid: &str, birth: &str) -> io::Result<Source> {
        let reply = self;
        if reply.schema != 1
            || reply.operation != operation
            || !valid_session(&reply.session, sid, birth)
            || reply.path.chars().any(char::is_control)
            || reply.path.split('/').any(|part| matches!(part, "." | ".."))
        {
            return Err(unavailable());
        }
        let native_intent = match reply.native_intent {
            serde_json::Value::Null if reply.assignment_generation == 0 => None,
            serde_json::Value::String(intent)
                if reply.assignment_generation > 0 && valid_hex(&intent, 32) =>
            {
                Some(intent)
            }
            _ => return Err(unavailable()),
        };
        let source = Source {
            operation: reply.operation,
            slot: reply.slot,
            path: PathBuf::from(reply.path),
            account: reply.account,
            user: reply.user,
            revision: Revision {
                generation: reply.credential_generation,
                digest: reply.digest,
            },
            assignment_generation: reply.assignment_generation,
            native_intent,
        };
        source.validate()?;
        Ok(source)
    }
}

/// Read only the caller's native source. The caller supplies a kernel-derived
/// birth, and the source adapter verifies the actual file under its own lock.
/// This request neither claims supervision nor assigns an account.
pub async fn source_at(
    endpoint: &Endpoint,
    operation: &str,
    process_birth: &str,
) -> io::Result<Source> {
    if !crate::locate::valid_sid(&endpoint.sid)
        || !valid_hex(operation, 32)
        || process_birth.is_empty()
        || process_birth.len() > 128
        || process_birth.chars().any(char::is_control)
    {
        return Err(unavailable());
    }
    let opened = conn::open(&endpoint.sock, CALL_TIMEOUT)
        .await
        .map_err(|_| unavailable())?;
    let connection = Conn::spawn(opened, CALL_TIMEOUT);
    let command = format!("@{} codex-auth source operation={operation}", endpoint.sid);
    let result = tokio::time::timeout(WAIT, async {
        loop {
            let reply = connection
                .request(&command, Framing::Status)
                .await
                .map_err(|_| unavailable())?;
            match parse(&reply.status, operation, &endpoint.sid, process_birth)? {
                Outcome::Ready(source) => return Ok(source),
                Outcome::Waiting => tokio::time::sleep(POLL).await,
            }
        }
    })
    .await
    .map_err(|_| unavailable())
    .and_then(std::convert::identity);
    connection.close();
    result
}

#[cfg(test)]
#[path = "native_source_tests.rs"]
mod tests;
