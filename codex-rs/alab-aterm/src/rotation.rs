// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Cross-account quota handoff. The host selects a destination from its own
//! roster; this protocol never accepts a credential path or restart command.
//! Same-account credential renewal remains a separate protocol.

use crate::Client;
use crate::Error;
use crate::renewal::Identity;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
pub struct Origin {
    pub thread: String,
    pub turn: String,
    pub identity: Identity,
}

impl Origin {
    pub fn valid(&self) -> bool {
        uuid(&self.thread) && uuid(&self.turn) && identity(&self.identity)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Receipt {
    pub id: String,
    pub origin: Origin,
    pub destination: Identity,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The source is not at a weekly wall; ordinary quota recovery still owns it.
    NotNeeded,
    Waiting(Option<Receipt>),
    Ready(Receipt),
    Committed(Receipt),
    Consumed(Receipt),
    Retired(Receipt),
}

impl Outcome {
    pub fn receipt(&self) -> Option<&Receipt> {
        match self {
            Self::NotNeeded => None,
            Self::Waiting(receipt) => receipt.as_ref(),
            Self::Ready(receipt)
            | Self::Committed(receipt)
            | Self::Consumed(receipt)
            | Self::Retired(receipt) => Some(receipt),
        }
    }
}

fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn identity(value: &Identity) -> bool {
    hex(&value.account, 64) && hex(&value.user, 64)
}

fn malformed() -> Error {
    // Never echo an untrusted reply; it may contain credentials or raw identities.
    Error::Malformed("invalid cross-account rotation reply".into())
}

fn parse(line: &str) -> Result<Outcome, Error> {
    let rest = line
        .strip_prefix("OK codex-rotation ")
        .ok_or_else(malformed)?;
    let mut fields = BTreeMap::new();
    for word in rest.split_ascii_whitespace() {
        let (key, value) = word.split_once('=').ok_or_else(malformed)?;
        if fields.insert(key, value).is_some() {
            return Err(malformed());
        }
    }
    let state = fields.get("state").copied().ok_or_else(malformed)?;
    if state == "not-needed" && fields.len() == 1 {
        return Ok(Outcome::NotNeeded);
    }
    if state == "waiting" && fields.len() == 1 {
        return Ok(Outcome::Waiting(None));
    }
    if fields.len() != 8 {
        return Err(malformed());
    }
    let get = |name| fields.get(name).copied().ok_or_else(malformed);
    let receipt = Receipt {
        id: get("id")?.into(),
        origin: Origin {
            thread: get("thread")?.into(),
            turn: get("turn")?.into(),
            identity: Identity {
                account: get("source_account")?.into(),
                user: get("source_user")?.into(),
            },
        },
        destination: Identity {
            account: get("target_account")?.into(),
            user: get("target_user")?.into(),
        },
    };
    if !hex(&receipt.id, 32)
        || !receipt.origin.valid()
        || !identity(&receipt.destination)
        || receipt.origin.identity.account == receipt.destination.account
    {
        return Err(malformed());
    }
    match state {
        "waiting" => Ok(Outcome::Waiting(Some(receipt))),
        "ready" => Ok(Outcome::Ready(receipt)),
        "committed" => Ok(Outcome::Committed(receipt)),
        "consumed" => Ok(Outcome::Consumed(receipt)),
        "retired" => Ok(Outcome::Retired(receipt)),
        _ => Err(malformed()),
    }
}

impl Client {
    async fn rotation_call(&self, request: &str) -> Result<Outcome, Error> {
        if !self.holds_claim() {
            return Err(Error::BadRequest(
                "rotation requires the native self-claim".into(),
            ));
        }
        let reply = self.on_self(request).await.map_err(|_| malformed())?;
        parse(&reply.status)
    }

    pub async fn rotation_prepare(&self, origin: &Origin) -> Result<Outcome, Error> {
        if !origin.valid() {
            return Err(Error::BadRequest("invalid rotation origin".into()));
        }
        let outcome = self
            .rotation_call(&format!(
                "codex-rotation prepare thread={} turn={} source_account={} source_user={}",
                origin.thread, origin.turn, origin.identity.account, origin.identity.user,
            ))
            .await?;
        if outcome
            .receipt()
            .is_some_and(|receipt| receipt.origin != *origin)
        {
            return Err(malformed());
        }
        Ok(outcome)
    }

    pub async fn rotation_status(&self, id: &str) -> Result<Outcome, Error> {
        self.rotation_id("status", id, None).await
    }

    pub async fn rotation_commit(&self, id: &str) -> Result<Outcome, Error> {
        self.rotation_id("commit", id, None).await
    }

    pub async fn rotation_consume(&self, id: &str, turn: &str) -> Result<Outcome, Error> {
        if !uuid(turn) {
            return Err(Error::BadRequest("invalid rotation accepted turn".into()));
        }
        self.rotation_id("consume", id, Some(turn)).await
    }

    async fn rotation_id(
        &self,
        verb: &str,
        id: &str,
        turn: Option<&str>,
    ) -> Result<Outcome, Error> {
        if !hex(id, 32) {
            return Err(Error::BadRequest("invalid rotation receipt".into()));
        }
        let suffix = turn.map(|turn| format!(" turn={turn}")).unwrap_or_default();
        let outcome = self
            .rotation_call(&format!("codex-rotation {verb} id={id}{suffix}"))
            .await?;
        if outcome.receipt().is_none_or(|receipt| receipt.id != id) {
            return Err(malformed());
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(state: &str) -> String {
        format!(
            "OK codex-rotation state={state} id={} thread=00000000-0000-0000-0000-000000000001 turn=00000000-0000-0000-0000-000000000002 source_account={} source_user={} target_account={} target_user={}",
            "1".repeat(32),
            "a".repeat(64),
            "b".repeat(64),
            "c".repeat(64),
            "d".repeat(64),
        )
    }

    #[test]
    fn every_phase_retains_both_identities_and_the_failed_native_turn() {
        for phase in ["waiting", "ready", "committed", "consumed", "retired"] {
            assert!(
                parse(&reply(phase)).is_ok_and(|outcome| outcome.receipt().is_some_and(|r| r
                    .origin
                    .identity
                    .account
                    == "a".repeat(64)
                    && r.destination.account == "c".repeat(64)
                    && r.origin.turn.ends_with('2')))
            );
        }
        assert!(matches!(
            parse("OK codex-rotation state=waiting"),
            Ok(Outcome::Waiting(None))
        ));
        assert!(matches!(
            parse("OK codex-rotation state=not-needed"),
            Ok(Outcome::NotNeeded)
        ));
        assert!(parse(&reply("not-needed")).is_err());
    }

    #[test]
    fn renewal_or_unbound_and_ambiguous_replies_cannot_be_rotation_receipts() {
        let good = reply("ready");
        for bad in [
            good.replace("codex-rotation", "codex-renewal"),
            good.replace(&"c".repeat(64), &"a".repeat(64)),
            good.replace(&"b".repeat(64), "raw-user"),
            good.replace("state=ready", "state=unknown"),
            good.replace("00000000-0000-0000-0000-000000000002", "invalid"),
            format!("{good} state=committed"),
            format!("{good} path=/credentials"),
        ] {
            assert!(parse(&bad).is_err());
        }
        let failure = parse("ERR secret-token-value").err().map(|e| e.to_string());
        assert!(failure.is_some_and(|error| !error.contains("secret-token-value")));
    }

    #[tokio::test]
    async fn rotation_calls_without_native_custody_never_reach_the_host() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let host = crate::fake::FakeAterm::start(dir.path(), crate::fake::Mode::Current)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let before = host.requests();
        let origin = Origin {
            thread: "00000000-0000-0000-0000-000000000001".into(),
            turn: "00000000-0000-0000-0000-000000000002".into(),
            identity: Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
        };
        assert!(matches!(
            client.rotation_prepare(&origin).await,
            Err(Error::BadRequest(_))
        ));
        let id = "1".repeat(32);
        assert!(matches!(
            client.rotation_status(&id).await,
            Err(Error::BadRequest(_))
        ));
        assert!(matches!(
            client.rotation_commit(&id).await,
            Err(Error::BadRequest(_))
        ));
        assert!(matches!(
            client.rotation_consume(&id, &origin.turn).await,
            Err(Error::BadRequest(_))
        ));
        assert_eq!(host.requests(), before);
    }
}
