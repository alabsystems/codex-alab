// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exec preparation through the shared account adapter. A selection has no
//! receipt or capability and cannot authorize suspension or process replacement.

use crate::Client;
use crate::Error;
use crate::renewal::Identity;
use crate::rotation::Origin;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
pub enum Selection {
    Waiting,
    NotNeeded,
    Selected {
        origin: Origin,
        destination: Identity,
    },
}

fn malformed() -> Error {
    Error::Malformed("invalid exec account selection".into())
}

fn parse(line: &str, expected: &Origin) -> Result<Selection, Error> {
    let rest = line
        .strip_prefix("OK codex-exec-handoff ")
        .ok_or_else(malformed)?;
    let mut fields = BTreeMap::new();
    for word in rest.split_ascii_whitespace() {
        let (key, value) = word.split_once('=').ok_or_else(malformed)?;
        if fields.insert(key, value).is_some() {
            return Err(malformed());
        }
    }
    match fields.get("state").copied() {
        Some("waiting") if fields.len() == 1 => return Ok(Selection::Waiting),
        Some("not-needed") if fields.len() == 1 => return Ok(Selection::NotNeeded),
        Some("selected") if fields.len() == 7 => {}
        _ => return Err(malformed()),
    }
    let field = |key| {
        fields
            .get(key)
            .map(|s| (*s).to_owned())
            .ok_or_else(malformed)
    };
    let origin = Origin {
        thread: field("thread")?,
        turn: field("turn")?,
        identity: Identity {
            account: field("source_account")?,
            user: field("source_user")?,
        },
    };
    let destination = Identity {
        account: field("target_account")?,
        user: field("target_user")?,
    };
    let target = Origin {
        identity: destination.clone(),
        ..origin.clone()
    };
    if origin != *expected
        || !origin.valid()
        || !target.valid()
        || destination.account == origin.identity.account
    {
        return Err(malformed());
    }
    Ok(Selection::Selected {
        origin,
        destination,
    })
}

pub async fn prepare(client: &Client, origin: &Origin) -> Result<Selection, Error> {
    if !origin.valid() || !client.holds_claim() {
        return Err(Error::BadRequest(
            "exec account selection requires a valid native origin and self-claim".into(),
        ));
    }
    let reply = client
        .on_self(&format!(
            "codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
            origin.thread, origin.turn, origin.identity.account, origin.identity.user,
        ))
        .await?;
    parse(&reply.status, origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> Origin {
        Origin {
            thread: "00000000-0000-0000-0000-000000000001".into(),
            turn: "00000000-0000-0000-0000-000000000002".into(),
            identity: Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
        }
    }

    #[tokio::test]
    async fn exec_account_selection_preserves_origin_and_is_not_a_rotation_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        let origin = origin();
        assert!(prepare(&client, &origin).await.is_err());
        client.harness_policy().await.unwrap();
        client.claim().await.unwrap();
        let request = format!(
            "@{} codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
            host.sid(),
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user
        );
        let selected = format!(
            "OK codex-exec-handoff state=selected thread={} turn={} source_account={} source_user={} target_account={} target_user={}\n",
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user,
            "c".repeat(64),
            "d".repeat(64)
        );
        host.reply_once(&request, &selected);
        assert!(matches!(
            prepare(&client, &origin).await.unwrap(),
            Selection::Selected { .. }
        ));
        for altered in [
            selected.replace("codex-exec-handoff", "codex-rotation"),
            selected.replace("state=selected", "state=ready"),
            selected.replace(&origin.turn, "00000000-0000-0000-0000-000000000003"),
            selected.replace(&"c".repeat(64), &origin.identity.account),
            format!("{} cap={}\n", selected.trim(), "e".repeat(64)),
        ] {
            host.reply_once(&request, &altered);
            assert!(prepare(&client, &origin).await.is_err());
        }
    }
}
