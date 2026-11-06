// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve both ends of account recovery while sharing the native continuation
//! lifecycle for interactive and headless callers. Each purpose still uses its
//! own host protocol and policy. This client adapter grants no execution authority.

pub use crate::exec_selection::Selection as ExecSelection;
pub use crate::exec_selection::prepare as prepare_exec;

use crate::Client;
use crate::Error;
use crate::renewal;
use crate::rotation;
use codex_alab_core::account_handoff::AccountHandoff;
use codex_alab_core::account_handoff::Purpose;

#[derive(Clone, PartialEq, Eq)]
pub struct Receipt {
    pub id: String,
    pub thread: String,
    pub wall: String,
    pub purpose: Purpose,
    pub identity: renewal::Identity,
    pub destination: renewal::Identity,
}

impl Receipt {
    /// Match recovery metadata to the original request and selected accounts.
    /// This does not replace the caller's live host and execution-admission checks.
    pub fn matches_handoff(&self, thread: &str, turn: &str, handoff: &AccountHandoff) -> bool {
        self.thread == thread
            && self.wall == turn
            && self.purpose == handoff.purpose
            && self.identity.account == handoff.account
            && self.identity.user == handoff.user
            && handoff.target().is_none_or(|target| {
                target
                    == (
                        self.destination.account.as_str(),
                        self.destination.user.as_str(),
                    )
            })
            && handoff.receipt.as_ref().is_none_or(|id| id == &self.id)
    }
}

/// Shared policy gate; a successful check alone does not authorize execution.
pub fn policy_allows(policy: &crate::policy::HarnessPolicy, purpose: Purpose) -> bool {
    policy.effective
        && match purpose {
            Purpose::Login => policy.get("logins") == Some("renew"),
            Purpose::WeeklyQuota => {
                policy.get("codex_rotation") == Some("1")
                    && matches!(policy.get("accounts"), Some("early" | "wall"))
            }
        }
        && policy.switch("relaunch")
        && policy.switch("resume_limits")
}

pub enum Outcome {
    NotNeeded,
    Waiting(Option<Receipt>),
    Ready(Receipt),
    Committed(Receipt),
    Consumed(Receipt),
    Retired(Receipt),
}

impl From<renewal::Receipt> for Receipt {
    fn from(value: renewal::Receipt) -> Self {
        Self {
            id: value.id,
            thread: value.thread,
            wall: value.wall,
            purpose: Purpose::Login,
            destination: value.identity.clone(),
            identity: value.identity,
        }
    }
}

impl From<rotation::Receipt> for Receipt {
    fn from(value: rotation::Receipt) -> Self {
        Self {
            id: value.id,
            thread: value.origin.thread,
            wall: value.origin.turn,
            purpose: Purpose::WeeklyQuota,
            identity: value.origin.identity,
            destination: value.destination,
        }
    }
}

impl From<renewal::Outcome> for Outcome {
    fn from(value: renewal::Outcome) -> Self {
        match value {
            renewal::Outcome::Waiting(value) => Self::Waiting(value.map(Into::into)),
            renewal::Outcome::Ready(value) => Self::Ready(value.into()),
            renewal::Outcome::Committed(value) => Self::Committed(value.into()),
            renewal::Outcome::Consumed(value) => Self::Consumed(value.into()),
            renewal::Outcome::Retired(value) => Self::Retired(value.into()),
        }
    }
}

impl From<rotation::Outcome> for Outcome {
    fn from(value: rotation::Outcome) -> Self {
        match value {
            rotation::Outcome::NotNeeded => Self::NotNeeded,
            rotation::Outcome::Waiting(value) => Self::Waiting(value.map(Into::into)),
            rotation::Outcome::Ready(value) => Self::Ready(value.into()),
            rotation::Outcome::Committed(value) => Self::Committed(value.into()),
            rotation::Outcome::Consumed(value) => Self::Consumed(value.into()),
            rotation::Outcome::Retired(value) => Self::Retired(value.into()),
        }
    }
}

pub async fn prepare(
    client: &Client,
    purpose: Purpose,
    thread: &str,
    wall: &str,
    identity: &renewal::Identity,
) -> Result<Outcome, Error> {
    match purpose {
        Purpose::Login => client
            .renewal_prepare(thread, wall, identity)
            .await
            .map(Into::into),
        Purpose::WeeklyQuota => client
            .rotation_prepare(&rotation::Origin {
                thread: thread.into(),
                turn: wall.into(),
                identity: identity.clone(),
            })
            .await
            .map(Into::into),
    }
}

pub async fn status(client: &Client, purpose: Purpose, id: &str) -> Result<Outcome, Error> {
    match purpose {
        Purpose::Login => client.renewal_status(id).await.map(Into::into),
        Purpose::WeeklyQuota => client.rotation_status(id).await.map(Into::into),
    }
}

pub async fn commit(client: &Client, purpose: Purpose, id: &str) -> Result<Outcome, Error> {
    match purpose {
        Purpose::Login => client.renewal_commit(id).await.map(Into::into),
        Purpose::WeeklyQuota => client.rotation_commit(id).await.map(Into::into),
    }
}

pub async fn consume(
    client: &Client,
    purpose: Purpose,
    id: &str,
    turn: &str,
) -> Result<Outcome, Error> {
    match purpose {
        Purpose::Login => client.renewal_consume(id, turn).await.map(Into::into),
        Purpose::WeeklyQuota => client.rotation_consume(id, turn).await.map(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_handoff_cannot_change_its_origin_destination_or_receipt() {
        let receipt = Receipt {
            id: "1".repeat(32),
            thread: "thread".into(),
            wall: "turn".into(),
            purpose: Purpose::WeeklyQuota,
            identity: renewal::Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
            destination: renewal::Identity {
                account: "c".repeat(64),
                user: "d".repeat(64),
            },
        };
        let handoff = AccountHandoff {
            purpose: Purpose::WeeklyQuota,
            openai_scope: true,
            local: false,
            account: receipt.identity.account.clone(),
            user: receipt.identity.user.clone(),
            destination: Some(codex_alab_core::account_handoff::Destination {
                account: receipt.destination.account.clone(),
                user: receipt.destination.user.clone(),
            }),
            receipt: Some(receipt.id.clone()),
            turn: None,
            accepted: false,
            starts: 1,
            goal_id: None,
        };
        assert!(receipt.matches_handoff("thread", "turn", &handoff));
        assert!(!receipt.matches_handoff("other-thread", "turn", &handoff));
        assert!(!receipt.matches_handoff("thread", "other-turn", &handoff));
        for field in 0..5 {
            let mut changed = handoff.clone();
            match field {
                0 => changed.account = "e".repeat(64),
                1 => changed.user = "e".repeat(64),
                2 => changed.destination.as_mut().unwrap().account = "e".repeat(64),
                3 => changed.receipt = Some("2".repeat(32)),
                _ => changed.purpose = Purpose::Login,
            }
            assert!(!receipt.matches_handoff("thread", "turn", &changed));
        }
    }

    #[tokio::test]
    async fn shared_recovery_dispatch_preserves_scope_and_rejects_protocol_substitution() {
        let directory = tempfile::tempdir().expect("scratch host");
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .expect("host");
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .expect("client");
        client.harness_policy().await.expect("policy");
        client.claim().await.expect("claim");
        let identity = renewal::Identity {
            account: "a".repeat(64),
            user: "b".repeat(64),
        };
        let thread = "00000000-0000-0000-0000-000000000001";
        let wall = "00000000-0000-0000-0000-000000000002";
        let id = "1".repeat(32);
        for purpose in [Purpose::Login, Purpose::WeeklyQuota] {
            let (protocol, command, reply) = match purpose {
                Purpose::Login => (
                    "codex-renewal",
                    format!(
                        "codex-renewal prepare thread={thread} wall={wall} account={} user={}",
                        identity.account, identity.user
                    ),
                    format!(
                        "OK codex-renewal state=ready id={id} thread={thread} wall={wall} account={} user={}\n",
                        identity.account, identity.user
                    ),
                ),
                Purpose::WeeklyQuota => (
                    "codex-rotation",
                    format!(
                        "codex-rotation prepare thread={thread} turn={wall} source_account={} source_user={}",
                        identity.account, identity.user
                    ),
                    format!(
                        "OK codex-rotation state=ready id={id} thread={thread} turn={wall} source_account={} source_user={} target_account={} target_user={}\n",
                        identity.account,
                        identity.user,
                        "c".repeat(64),
                        "d".repeat(64)
                    ),
                ),
            };
            host.reply_once(&format!("@{} {command}", host.sid()), &reply);
            let Outcome::Ready(receipt) = prepare(&client, purpose, thread, wall, &identity)
                .await
                .expect("prepared")
            else {
                panic!("ready receipt")
            };
            assert!(receipt.purpose == purpose && receipt.identity == identity);
            assert_eq!(
                (&*receipt.id, &*receipt.thread, &*receipt.wall),
                (&*id, thread, wall)
            );
            assert_eq!(
                receipt.destination.account,
                if purpose == Purpose::Login {
                    identity.account.clone()
                } else {
                    "c".repeat(64)
                }
            );
            let other = if purpose == Purpose::Login {
                "codex-rotation"
            } else {
                "codex-renewal"
            };
            host.reply_once(
                &format!("@{} {protocol} status id={id}", host.sid()),
                &reply.replace(protocol, other),
            );
            assert!(status(&client, purpose, &id).await.is_err());
        }
    }
}
