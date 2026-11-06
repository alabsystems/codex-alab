// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Identity-bound renewal of the login this native session already uses. The
//! host derives every path and restart command; callers supply no launch data.

use crate::Client;
use crate::Error;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    pub account: String,
    pub user: String,
}

impl Identity {
    pub fn from_native(account: &str, user: &str) -> Option<Self> {
        if account.is_empty() || user.is_empty() {
            return None;
        }
        let digest = |domain: &[u8], value: &str| {
            let mut hash = Sha256::new();
            hash.update(domain);
            hash.update(value.as_bytes());
            format!("{:x}", hash.finalize())
        };
        Some(Self {
            account: digest(b"aterm/codex-renewal/account/v1\0", account),
            user: digest(b"aterm/codex-renewal/user/v1\0", user),
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Receipt {
    pub id: String,
    pub thread: String,
    pub wall: String,
    pub identity: Identity,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Outcome {
    Waiting(Option<Receipt>),
    Ready(Receipt),
    Committed(Receipt),
    Consumed(Receipt),
    Retired(Receipt),
}

impl Outcome {
    pub fn receipt(&self) -> Option<&Receipt> {
        match self {
            Self::Waiting(receipt) => receipt.as_ref(),
            Self::Ready(receipt)
            | Self::Committed(receipt)
            | Self::Consumed(receipt)
            | Self::Retired(receipt) => Some(receipt),
        }
    }
}

fn malformed() -> Error {
    Error::Malformed("invalid same-account renewal reply".into())
}

fn identifier(value: &str, bytes: usize) -> bool {
    value.len() == bytes
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn valid_id(value: &str) -> bool {
    identifier(value, 32)
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}

fn parse(line: &str) -> Result<Outcome, Error> {
    let rest = line
        .strip_prefix("OK codex-renewal ")
        .ok_or_else(malformed)?;
    let mut fields = BTreeMap::new();
    for token in rest.split_ascii_whitespace() {
        let (key, value) = token.split_once('=').ok_or_else(malformed)?;
        if fields.insert(key, value).is_some() {
            return Err(malformed());
        }
    }
    match fields.get("state").copied() {
        Some("waiting") if fields.len() == 1 => Ok(Outcome::Waiting(None)),
        Some(state @ ("waiting" | "ready" | "committed" | "consumed" | "retired"))
            if fields.len() == 6 =>
        {
            let get = |key| fields.get(key).copied().ok_or_else(malformed);
            let id = get("id")?;
            let thread = get("thread")?;
            let wall = get("wall")?;
            if !valid_id(id) || !uuid(thread) || !uuid(wall) {
                return Err(malformed());
            }
            let account = get("account")?;
            let user = get("user")?;
            if !identifier(account, 64) || !identifier(user, 64) {
                return Err(malformed());
            }
            let receipt = Receipt {
                id: id.into(),
                thread: thread.into(),
                wall: wall.into(),
                identity: Identity {
                    account: account.into(),
                    user: user.into(),
                },
            };
            Ok(match state {
                "waiting" => Outcome::Waiting(Some(receipt)),
                "ready" => Outcome::Ready(receipt),
                "committed" => Outcome::Committed(receipt),
                "consumed" => Outcome::Consumed(receipt),
                _ => Outcome::Retired(receipt),
            })
        }
        _ => Err(malformed()),
    }
}

impl Client {
    async fn renewal_call(&self, request: &str) -> Result<Outcome, Error> {
        if !self.holds_claim() {
            return Err(Error::BadRequest(
                "renewal requires the native self-claim".into(),
            ));
        }
        let reply = self
            .on_self(request)
            .await
            .map_err(|_| Error::Malformed("same-account renewal exchange unavailable".into()))?;
        // Never propagate server text: a malformed peer must not leak an identity.
        parse(&reply.status)
    }

    pub async fn renewal_prepare(
        &self,
        thread: &str,
        wall: &str,
        identity: &Identity,
    ) -> Result<Outcome, Error> {
        if !uuid(thread) || !uuid(wall) {
            return Err(Error::BadRequest("invalid renewal native origin".into()));
        }
        if !identifier(&identity.account, 64) || !identifier(&identity.user, 64) {
            return Err(Error::BadRequest("invalid renewal identity proof".into()));
        }
        let outcome = self
            .renewal_call(&format!(
                "codex-renewal prepare thread={thread} wall={wall} account={} user={}",
                identity.account, identity.user
            ))
            .await?;
        if outcome.receipt().is_some_and(|receipt| {
            receipt.thread != thread || receipt.wall != wall || receipt.identity != *identity
        }) {
            return Err(malformed());
        }
        Ok(outcome)
    }

    pub async fn renewal_status(&self, id: &str) -> Result<Outcome, Error> {
        self.renewal_id("status", id, None).await
    }

    pub async fn renewal_commit(&self, id: &str) -> Result<Outcome, Error> {
        self.renewal_id("commit", id, None).await
    }

    pub async fn renewal_consume(&self, id: &str, turn: &str) -> Result<Outcome, Error> {
        if !uuid(turn) {
            return Err(Error::BadRequest("invalid renewal accepted turn".into()));
        }
        self.renewal_id("consume", id, Some(turn)).await
    }

    async fn renewal_id(&self, verb: &str, id: &str, turn: Option<&str>) -> Result<Outcome, Error> {
        if !valid_id(id) {
            return Err(Error::BadRequest("invalid renewal receipt".into()));
        }
        let suffix = turn.map(|turn| format!(" turn={turn}")).unwrap_or_default();
        let outcome = self
            .renewal_call(&format!("codex-renewal {verb} id={id}{suffix}"))
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

    #[test]
    fn strict_receipts_reject_ambiguous_or_unbound_replies() {
        let line = format!(
            "OK codex-renewal state=committed id=0123456789abcdef0123456789abcdef thread=00000000-0000-0000-0000-000000000001 wall=00000000-0000-0000-0000-000000000002 account={} user={}",
            "a".repeat(64),
            "b".repeat(64)
        );
        assert!(
            matches!(parse(&line), Ok(Outcome::Committed(receipt)) if receipt.identity.account == "a".repeat(64))
        );
        for invalid in [
            format!("{line} state=ready"),
            line.replace(&"a".repeat(64), "ff"),
            line.replace(&"b".repeat(64), "0A"),
            line.replace("state=committed", "state=unknown"),
            "OK state=committed".into(),
            "ERR sensitive payload".into(),
        ] {
            assert!(parse(&invalid).is_err());
        }
        assert!(matches!(
            parse("OK codex-renewal state=waiting"),
            Ok(Outcome::Waiting(None))
        ));
        for state in ["waiting", "ready", "committed", "consumed", "retired"] {
            let phase = line.replace("state=committed", &format!("state={state}"));
            assert!(parse(&phase).is_ok_and(|outcome| outcome.receipt().is_some()));
        }
        for state in ["ready", "committed", "consumed", "retired"] {
            assert!(parse(&format!("OK codex-renewal state={state}")).is_err());
        }
    }

    #[test]
    fn identity_proof_is_domain_separated_and_requires_known_identity() {
        let identity = Identity::from_native("same", "same").unwrap();
        assert_ne!(identity.account, identity.user);
        assert_eq!(
            identity.account,
            "4d9a5fa2af08763c11df9b82406d3b53813e1d0e957f9f682bb451234e54ce46"
        );
        assert_eq!(
            identity.user,
            "002317753bb35b3537fca8bdfaa8c0912d9c3912d607e1ead5afca2482c08814"
        );
        assert!(identifier(&identity.account, 64));
        assert!(Identity::from_native("", "user").is_none());
    }
}
