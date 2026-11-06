// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native admission for members of a source-bound exec tree.
//! Manifest data and a prior root admission grant no authority locally.

use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub thread: String,
    pub parent: Option<String>,
    pub turn: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    version: u32,
    digest: String,
    root: String,
    turn: String,
    members: Vec<Member>,
}

fn identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

impl Manifest {
    pub fn new(
        root: String,
        turn: String,
        digest: String,
        members: Vec<Member>,
    ) -> Result<Self, Error> {
        let manifest = Self {
            version: 1,
            root,
            turn,
            digest,
            members,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.version != 1
            || !hex(&self.digest, 64)
            || !identity(&self.root)
            || !identity(&self.turn)
        {
            return Err(bad());
        }
        let mut seen = HashSet::new();
        for member in &self.members {
            if !identity(&member.thread)
                || member.turn.as_ref().is_some_and(|turn| !identity(turn))
                || seen.contains(&member.thread)
            {
                return Err(bad());
            }
            if seen.is_empty() {
                if member.thread != self.root
                    || member.parent.is_some()
                    || member.turn.as_ref() != Some(&self.turn)
                {
                    return Err(bad());
                }
            } else if !member
                .parent
                .as_ref()
                .is_some_and(|parent| seen.contains(parent))
            {
                return Err(bad());
            }
            seen.insert(member.thread.clone());
        }
        if seen.is_empty() {
            return Err(bad());
        }
        Ok(())
    }

    fn matches(&self, grant: &Grant) -> Result<(), Error> {
        self.validate()?;
        if !grant.ticket.valid()
            || self.root != grant.origin.thread
            || self.turn != grant.origin.turn
        {
            return Err(bad());
        }
        Ok(())
    }
}

pub async fn supported(client: &Client) -> bool {
    client
        .on_self("codex-exec-handoff tree-info")
        .await
        .is_ok_and(|reply| reply.status.trim() == "OK codex-exec-handoff tree-contract=1")
}

/// Publish the exact source members and capsule digest before execute.
/// A retry may reconcile only the same manifest on the native operation.
pub async fn bind(client: &Client, grant: &Grant, manifest: &Manifest) -> Result<(), Error> {
    manifest.matches(grant)?;
    let json = serde_json::to_vec(manifest).map_err(|_| bad())?;
    let command = format!(
        "codex-exec-handoff bind-tree id={} cap={} manifest={}",
        grant.ticket.id,
        grant.ticket.capability,
        encode(&json)
    );
    if command.len() > 64 * 1024 {
        return Err(bad());
    }
    let reply = client.on_self(&command).await?;
    bound_reply(&reply.status, &manifest.digest)
}

fn bound_reply(reply: &str, digest: &str) -> Result<(), Error> {
    let values = fields(reply)?;
    if values.len() != 2
        || values.get("state") != Some(&"tree-bound")
        || values.get("digest") != Some(&digest)
    {
        return Err(bad());
    }
    Ok(())
}

/// Each call reaches native policy and account validation, including retries.
/// `None` requests an idle member's admission and creates no original turn.
pub async fn admit(
    client: &Client,
    grant: &Grant,
    manifest: &Manifest,
    thread: &str,
    turn: Option<&str>,
) -> Result<bool, Error> {
    manifest.matches(grant)?;
    if thread == manifest.root
        || !manifest
            .members
            .iter()
            .any(|member| member.thread == thread && member.turn.as_deref() == turn)
    {
        return Err(bad());
    }
    let turn = turn
        .map(|turn| encode(turn.as_bytes()))
        .unwrap_or_else(|| "-".into());
    let command = format!(
        "codex-exec-handoff admit-member id={} cap={} digest={} thread={} turn={}",
        grant.ticket.id, grant.ticket.capability, manifest.digest, thread, turn
    );
    let reply = client.on_self(&command).await?;
    admitted_reply(&reply.status, &manifest.digest, thread, &turn)
}

fn admitted_reply(reply: &str, digest: &str, thread: &str, turn: &str) -> Result<bool, Error> {
    let values = fields(reply)?;
    if values.len() != 5
        || values.get("state") != Some(&"member-admitted")
        || values.get("digest") != Some(&digest)
        || values.get("thread") != Some(&thread)
        || values.get("turn") != Some(&turn)
    {
        return Err(bad());
    }
    match values.get("fresh").copied() {
        Some("1") => Ok(true),
        Some("0") => Ok(false),
        _ => Err(bad()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest::new(
            "root".into(),
            "original".into(),
            "a".repeat(64),
            vec![
                Member {
                    thread: "root".into(),
                    parent: None,
                    turn: Some("original".into()),
                },
                Member {
                    thread: "child".into(),
                    parent: Some("root".into()),
                    turn: Some("turn".into()),
                },
                Member {
                    thread: "idle".into(),
                    parent: Some("child".into()),
                    turn: None,
                },
            ],
        )
        .unwrap()
    }

    #[test]
    fn manifest_preserves_idle_members_and_rejects_changed_lineage() {
        let original = manifest();
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Manifest = serde_json::from_str(&encoded).unwrap();
        assert!(decoded == original);
        let mut changed = original.clone();
        changed.members.swap(1, 2);
        assert!(changed.validate().is_err());
        let mut changed = original.clone();
        changed.members.push(changed.members[1].clone());
        assert!(changed.validate().is_err());
        let mut changed = original;
        changed.members[0].turn = None;
        assert!(changed.validate().is_err());
    }

    #[test]
    fn replies_bind_the_digest_member_turn_and_retry_status() {
        let digest = "a".repeat(64);
        assert!(
            bound_reply(
                &format!("OK codex-exec-handoff state=tree-bound digest={digest}"),
                &digest
            )
            .is_ok()
        );
        assert!(
            bound_reply(
                &format!("OK codex-exec-handoff state=tree-bound digest={digest} extra=1"),
                &digest
            )
            .is_err()
        );
        let reply = format!(
            "OK codex-exec-handoff state=member-admitted digest={digest} thread=child turn=7475726e fresh=1"
        );
        assert!(admitted_reply(&reply, &digest, "child", "7475726e").unwrap());
        assert!(admitted_reply(&reply, &digest, "other", "7475726e").is_err());
        assert!(admitted_reply(&reply, &digest, "child", "-").is_err());
        assert!(admitted_reply(&reply, &"b".repeat(64), "child", "7475726e").is_err());
        assert!(
            !admitted_reply(
                &reply.replace("fresh=1", "fresh=0"),
                &digest,
                "child",
                "7475726e"
            )
            .unwrap()
        );
    }
    #[tokio::test]
    async fn every_member_retry_reaches_native_and_idle_has_no_turn() {
        let dir = tempfile::tempdir().unwrap();
        let fake = crate::fake::FakeAterm::start(dir.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let client = Client::connect(fake.endpoint(), crate::Options::default())
            .await
            .unwrap();
        let manifest = manifest();
        let grant = Grant {
            ticket: Ticket {
                id: "1".repeat(32),
                capability: "2".repeat(64),
            },
            stage: Stage::Loaded,
            origin: crate::rotation::Origin {
                thread: "root".into(),
                turn: "original".into(),
                identity: Identity {
                    account: "a".repeat(64),
                    user: "b".repeat(64),
                },
            },
            destination: Identity {
                account: "c".repeat(64),
                user: "d".repeat(64),
            },
            auth_path: "/private/fixture/auth.json".into(),
            checkpoint: serde_json::json!({}),
            deadline_us: 123,
        };
        let prefix = format!("@{} codex-exec-handoff", fake.sid());
        fake.reply_once(
            &format!("{prefix} tree-info"),
            "OK codex-exec-handoff tree-contract=1\n",
        );
        assert!(supported(&client).await);
        let bind_command = format!(
            "{prefix} bind-tree id={} cap={} manifest={}",
            grant.ticket.id,
            grant.ticket.capability,
            encode(&serde_json::to_vec(&manifest).unwrap())
        );
        fake.reply_once(
            &bind_command,
            &format!(
                "OK codex-exec-handoff state=tree-bound digest={}\n",
                manifest.digest
            ),
        );
        bind(&client, &grant, &manifest).await.unwrap();
        let command = format!(
            "{prefix} admit-member id={} cap={} digest={} thread=child turn=7475726e",
            grant.ticket.id, grant.ticket.capability, manifest.digest
        );
        fake.reply_once(&command, &format!("OK codex-exec-handoff state=member-admitted digest={} thread=child turn=7475726e fresh=1\n", manifest.digest));
        assert!(
            admit(&client, &grant, &manifest, "child", Some("turn"))
                .await
                .unwrap()
        );
        fake.reply_once(&command, "ERR denied\n");
        assert!(
            admit(&client, &grant, &manifest, "child", Some("turn"))
                .await
                .is_err()
        );
        assert_eq!(
            fake.requests()
                .iter()
                .filter(|request| **request == command)
                .count(),
            2
        );
        let idle = format!(
            "{prefix} admit-member id={} cap={} digest={} thread=idle turn=-",
            grant.ticket.id, grant.ticket.capability, manifest.digest
        );
        fake.reply_once(&idle, &format!("OK codex-exec-handoff state=member-admitted digest={} thread=idle turn=- fresh=1\n", manifest.digest));
        assert!(
            admit(&client, &grant, &manifest, "idle", None)
                .await
                .unwrap()
        );
    }
}
