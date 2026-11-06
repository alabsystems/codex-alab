// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Owner-managed refresh on a dedicated authenticated connection. Unlike the
//! Autopilot client this never claims supervision and works with ALab UI off.

use crate::CALL_TIMEOUT;
use crate::conn;
use crate::conn::Conn;
use crate::conn::Framing;
use crate::locate::Endpoint;
use codex_alab_core::managed_auth as core;
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);

struct Host;

impl core::Host for Host {
    fn refresh<'a>(&'a self, request: &'a core::Request) -> core::RefreshFuture<'a> {
        Box::pin(async move {
            let endpoint = Endpoint::managed_auth_from_env().ok_or_else(core::unavailable)?;
            refresh_at(&endpoint, request).await
        })
    }
}

pub fn install() {
    if codex_alab_core::is_alab_binary() {
        core::install(Arc::new(Host));
    }
}

enum Outcome {
    Waiting,
    Ready(core::Revision),
}

fn parse(line: &str, request: &core::Request) -> io::Result<Outcome> {
    if line.len() > 1024 {
        return Err(core::unavailable());
    }
    let body = line
        .strip_prefix("OK codex-auth ")
        .ok_or_else(core::unavailable)?;
    // Before the native worker has read its owner, no real revision is known.
    // Correlate this distinct pending reply without inventing one.
    let (pending, body) = match body.strip_prefix("refresh-pending ") {
        Some(body) => (true, body),
        None => (false, body),
    };
    let mut fields = BTreeMap::new();
    for field in body.split_ascii_whitespace() {
        let (key, value) = field.split_once('=').ok_or_else(core::unavailable)?;
        if fields.insert(key, value).is_some() {
            return Err(core::unavailable());
        }
    }
    if fields.len() != if pending { 3 } else { 6 }
        || fields.get("slot").copied() != Some(request.slot.as_str())
        || fields.get("operation").copied() != Some(request.operation.as_str())
        || fields.get("expected_digest").copied() != Some(request.expected_digest.as_str())
    {
        return Err(core::unavailable());
    }
    if pending {
        return Ok(Outcome::Waiting);
    }
    let generation = fields
        .get("generation")
        .and_then(|value| crate::wire::canonical_u64(value))
        .filter(|generation| *generation > 0)
        .ok_or_else(core::unavailable)?;
    let digest = fields
        .get("digest")
        .filter(|digest| core::valid_hex(digest, 64))
        .ok_or_else(core::unavailable)?;
    match fields.get("state").copied() {
        Some("waiting") => Ok(Outcome::Waiting),
        Some("ready") => Ok(Outcome::Ready(core::Revision {
            generation,
            digest: (*digest).to_owned(),
        })),
        _ => Err(core::unavailable()),
    }
}

async fn refresh_at(endpoint: &Endpoint, request: &core::Request) -> io::Result<core::Revision> {
    if !crate::locate::valid_sid(&endpoint.sid)
        || !core::valid_slot(&request.slot)
        || !core::valid_hex(&request.operation, 32)
        || !core::valid_hex(&request.expected_digest, 64)
    {
        return Err(core::unavailable());
    }
    let opened = conn::open(&endpoint.sock, CALL_TIMEOUT)
        .await
        .map_err(|_| core::unavailable())?;
    let connection = Conn::spawn(opened, CALL_TIMEOUT);
    let line = format!(
        "@{} codex-auth refresh slot={} operation={} expected_digest={}",
        endpoint.sid, request.slot, request.operation, request.expected_digest
    );
    let result = tokio::time::timeout(WAIT, async {
        loop {
            let reply = connection
                .request(&line, Framing::Status)
                .await
                .map_err(|_| core::unavailable())?;
            match parse(&reply.status, request)? {
                Outcome::Ready(revision) => return Ok(revision),
                Outcome::Waiting => tokio::time::sleep(POLL).await,
            }
        }
    })
    .await
    .map_err(|_| core::unavailable())
    .and_then(std::convert::identity);
    connection.close();
    result
}

#[cfg(test)]
#[path = "managed_auth_tests.rs"]
mod tests;
