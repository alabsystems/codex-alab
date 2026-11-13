// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Read-only native subscription pool metadata. This protocol carries no credentials.

use crate::Client;
use crate::Error;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: u32,
    pub observed_at: i64,
    pub request: String,
    pub session: Session,
    pub revision: String,
    pub assignment: Assignment,
    pub accounts: Vec<Account>,
    pub capabilities: Capabilities,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub sid: String,
    pub process_pid: u32,
    pub process_birth: String,
    pub epoch: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub account_id: Option<String>,
    pub mode: Mode,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub id: String,
    pub label: String,
    pub masked_email: Option<String>,
    pub condition: Condition,
    pub enrolled: bool,
    pub held: bool,
    pub eligible: bool,
    pub quota: Quota,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Quota {
    pub stale: bool,
    pub observed_at: Option<i64>,
    pub five_hour_used_percent: Option<f64>,
    pub five_hour_reset_at: Option<i64>,
    pub weekly_used_percent: Option<f64>,
    pub weekly_reset_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Condition {
    NotEnrolled,
    SignedOut,
    Limited,
    Low,
    Usable,
    Unseen,
}

impl Condition {
    pub fn label(&self) -> &'static str {
        match self {
            Self::NotEnrolled => "not enrolled",
            Self::SignedOut => "signed out",
            Self::Limited => "limited",
            Self::Low => "low",
            Self::Usable => "usable",
            Self::Unseen => "unseen",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub add: bool,
    pub select: bool,
    pub auto: bool,
}

impl Account {
    /// Whether the pool's record lets a session move to this row now. The
    /// host's own target rule refuses a row whose login is absent or signed
    /// out, one held for another machine, one standing at a wall and a low one.
    fn can_take_a_session(&self) -> bool {
        self.enrolled
            && !self.held
            && matches!(self.condition, Condition::Usable | Condition::Unseen)
    }
}

impl Snapshot {
    /// Whether the pool shows an account other than this session's own that a
    /// session could move to now. Cached host records, not permission: the
    /// host reads each candidate afresh before it selects one, so this can
    /// only say that asking for a move is not known to be futile.
    ///
    /// Where the host names no row as this session's, any one able row may be
    /// the session's own, so two are required.
    pub fn another_account_can_run(&self) -> bool {
        let able = self.accounts.iter().filter(|row| row.can_take_a_session());
        match self.assignment.account_id.as_deref() {
            Some(own) => able.filter(|row| row.id != own).count() >= 1,
            None => able.count() >= 2,
        }
    }
}

pub(crate) fn valid_text(text: &str, max: usize) -> bool {
    !text.is_empty() && text.chars().count() <= max && !text.chars().any(char::is_control)
}

pub(crate) fn valid_session(session: &Session, sid: &str, birth: &str) -> bool {
    session.sid == sid
        && session.process_pid == std::process::id()
        && session.process_birth == birth
        && valid_text(&session.epoch, 128)
}

pub(crate) fn request_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}",
        Sha256::digest(format!(
            "{}:{}:{:?}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
        ))
    )[..32]
        .to_owned()
}

fn parse(line: &str, request: &str, sid: &str, birth: &str) -> Result<Snapshot, Error> {
    let invalid = || Error::Malformed("invalid native accounts snapshot".into());
    if line.len() > 64 * 1024 {
        return Err(invalid());
    }
    let snapshot: Snapshot = serde_json::from_str(
        line.strip_prefix("OK codex-accounts ")
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())?;
    if snapshot.schema != 1
        || !(0..=253_402_300_799).contains(&snapshot.observed_at)
        || snapshot.request != request
        || !valid_session(&snapshot.session, sid, birth)
        || !codex_alab_core::managed_auth::valid_hex(&snapshot.revision, 64)
        || snapshot.accounts.len() > 128
        || snapshot.capabilities.add
        || snapshot.capabilities.select
        || snapshot.capabilities.auto
    {
        return Err(invalid());
    }
    let mut ids = std::collections::BTreeSet::new();
    for account in &snapshot.accounts {
        let quota = &account.quota;
        if !valid_text(&account.id, 256)
            || !ids.insert(&account.id)
            || !valid_text(&account.label, 128)
            || account.eligible
            || account
                .masked_email
                .as_ref()
                .is_some_and(|text| !valid_text(text, 256))
            || [quota.five_hour_used_percent, quota.weekly_used_percent]
                .into_iter()
                .flatten()
                .any(|n| !n.is_finite() || !(0.0..=100.0).contains(&n))
            || [
                quota.observed_at,
                quota.five_hour_reset_at,
                quota.weekly_reset_at,
            ]
            .into_iter()
            .flatten()
            .any(|n| !(0..=253_402_300_799).contains(&n))
        {
            return Err(invalid());
        }
    }
    if snapshot
        .assignment
        .account_id
        .as_ref()
        .is_some_and(|id| !ids.contains(id))
    {
        return Err(invalid());
    }
    Ok(snapshot)
}

impl Client {
    /// Account custody is independent of the optional presentation and supervision.
    pub async fn connect_accounts_self() -> Result<Option<Self>, Error> {
        match crate::Endpoint::managed_auth_from_env() {
            Some(endpoint) => Self::connect(endpoint, crate::Options::default())
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    /// Ask the authenticated hosting session. No supervision claim or mutation is sent.
    pub async fn accounts_snapshot(&self, process_birth: &str) -> Result<Snapshot, Error> {
        let request = request_id();
        self.accounts_request(process_birth, &request).await
    }

    async fn accounts_request(
        &self,
        process_birth: &str,
        request: &str,
    ) -> Result<Snapshot, Error> {
        let command = format!("codex-accounts snapshot schema=1 request={request}");
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let reply = self.on_self(&command).await?;
                if reply.status.starts_with("ERR ") {
                    return Err(crate::conn::refusal(reply.status));
                }
                if waiting(&reply.status, request)? {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
                return parse(&reply.status, request, self.endpoint_sid(), process_birth);
            }
        })
        .await
        .map_err(|_| Error::Unanswered)?
    }
}

pub(crate) fn waiting(line: &str, request: &str) -> Result<bool, Error> {
    if !line.starts_with("OK codex-accounts state=") {
        return Ok(false);
    }
    if line == format!("OK codex-accounts state=waiting request={request}") {
        Ok(true)
    } else {
        Err(Error::Malformed("invalid accounts wait correlation".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use serde_json::json;

    fn fixture() -> Value {
        json!({"schema":1,"observed_at":1800000000,"request":"a".repeat(32),
            "session":{"sid":"s-abc","process_pid":std::process::id(),
                "process_birth":"Fri Nov 6 09:00:00 2026","epoch":"host-epoch"},
            "revision":"b".repeat(64),"assignment":{"account_id":null,"mode":"unknown"},
            "accounts":[{"id":"safe-account","label":"Work","masked_email":"a***@example.test",
                "condition":"usable","enrolled":true,"held":false,"eligible":false,
                "quota":{"stale":true,"observed_at":null,"five_hour_used_percent":null,
                    "five_hour_reset_at":null,"weekly_used_percent":null,"weekly_reset_at":null}}],
            "capabilities":{"add":false,"select":false,"auto":false}})
    }

    fn read(value: &Value) -> Result<Snapshot, Error> {
        parse(
            &format!("OK codex-accounts {value}"),
            &"a".repeat(32),
            "s-abc",
            "Fri Nov 6 09:00:00 2026",
        )
    }

    #[test]
    fn accounts_snapshot_is_bound_and_rejects_unknown_authority_or_malformed_metadata() {
        assert_eq!(read(&fixture()).unwrap().accounts[0].label, "Work");
        for pointer in [
            "/schema",
            "/request",
            "/session/sid",
            "/session/process_pid",
            "/session/process_birth",
            "/observed_at",
            "/revision",
            "/accounts/0/condition",
            "/assignment/mode",
            "/accounts/0/quota/stale",
        ] {
            let mut changed = fixture();
            *changed.pointer_mut(pointer).unwrap() = json!("invalid");
            assert!(read(&changed).is_err(), "{pointer}");
        }
        for pointer in ["/session/process_birth", "/session/epoch"] {
            let mut changed = fixture();
            *changed.pointer_mut(pointer).unwrap() = json!("");
            assert!(read(&changed).is_err());
        }
        for (pointer, value) in [
            ("/session/process_pid", json!(std::process::id() + 1)),
            ("/accounts/0/label", json!("unsafe\nlabel")),
            ("/accounts/0/label", json!("a".repeat(129))),
            ("/accounts/0/quota/weekly_used_percent", json!(101)),
            ("/accounts/0/quota/weekly_reset_at", json!(i64::MAX)),
            ("/capabilities/select", json!(true)),
            ("/assignment/account_id", json!("missing")),
        ] {
            let mut changed = fixture();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(read(&changed).is_err(), "{pointer}");
        }
        let mut changed = fixture();
        changed["access_token"] = json!("must never enter a view");
        assert!(read(&changed).is_err());
        let mut changed = fixture();
        let duplicate = changed["accounts"][0].clone();
        changed["accounts"].as_array_mut().unwrap().push(duplicate);
        assert!(read(&changed).is_err());
        let oversized = format!("OK codex-accounts {}", " ".repeat(65536));
        assert!(parse(&oversized, "", "", "").is_err());
    }

    /// A two-row pool: `mine` and `other`, each a condition and whether its
    /// login is enrolled here, with `own` the row the host names as the session's.
    fn pool(own: Option<&str>, mine: (&str, bool), other: (&str, bool)) -> Snapshot {
        let mut value = fixture();
        let row = value["accounts"][0].clone();
        let rows = [("mine", mine), ("other", other)].map(|(id, (condition, enrolled))| {
            let mut row = row.clone();
            row["id"] = json!(id);
            row["condition"] = json!(condition);
            row["enrolled"] = json!(enrolled);
            row
        });
        value["accounts"] = json!(rows);
        value["assignment"]["account_id"] = json!(own);
        read(&value).unwrap()
    }

    #[test]
    fn accounts_pool_shows_another_account_only_beyond_the_sessions_own() {
        // One enrolled row: whichever account the session runs on, no other can take it.
        let alone = read(&fixture()).unwrap();
        assert_eq!(alone.accounts.len(), 1);
        assert!(!alone.another_account_can_run());
        let empty = {
            let mut value = fixture();
            value["accounts"] = json!([]);
            read(&value).unwrap()
        };
        assert!(!empty.another_account_can_run());

        // The host names no row as the session's: one able row may be its own.
        assert!(pool(None, ("usable", true), ("unseen", true)).another_account_can_run());
        for other in [
            ("limited", true),
            ("low", true),
            ("signed-out", true),
            ("not-enrolled", false),
            ("usable", false),
        ] {
            assert!(
                !pool(None, ("usable", true), other).another_account_can_run(),
                "{other:?}"
            );
            assert!(
                !pool(None, other, ("unseen", true)).another_account_can_run(),
                "{other:?} first"
            );
        }
        let mut held = pool(None, ("usable", true), ("usable", true));
        held.accounts[1].held = true;
        assert!(!held.another_account_can_run(), "held for another machine");

        // The host names the session's row: one other able row is enough, and
        // the session's own never counts.
        assert!(pool(Some("mine"), ("limited", true), ("unseen", true)).another_account_can_run());
        assert!(pool(Some("mine"), ("usable", true), ("usable", true)).another_account_can_run());
        assert!(!pool(Some("mine"), ("usable", true), ("limited", true)).another_account_can_run());
        assert!(
            !pool(Some("other"), ("limited", true), ("usable", true)).another_account_can_run()
        );
    }

    #[test]
    fn accounts_wait_reuses_exact_request_and_refuses_stale_correlation() {
        let request = "a".repeat(32);
        assert!(
            waiting(
                &format!("OK codex-accounts state=waiting request={request}"),
                &request
            )
            .unwrap()
        );
        assert!(
            waiting(
                &format!("OK codex-accounts state=waiting request={}", "b".repeat(32)),
                &request
            )
            .is_err()
        );
        assert!(!waiting("OK codex-accounts {}", &request).unwrap());
    }

    #[test]
    fn accounts_shared_protocol_fixture_uses_the_real_parser() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../alab/design/accounts-protocol-fixtures.json"
        ))
        .unwrap();
        let cases = corpus["cases"].as_array().unwrap();
        let binding = &corpus["bindings"];
        let mut base = cases[0]["snapshot"].clone();
        base["session"]["process_pid"] = json!(std::process::id());
        for case in cases {
            let mut value = case
                .get("snapshot")
                .cloned()
                .unwrap_or_else(|| base.clone());
            value["session"]["process_pid"] = json!(std::process::id());
            if let Some(replace) = case.get("replace") {
                let path = replace["path"].as_str().unwrap();
                if let Some(target) = value.pointer_mut(path) {
                    *target = replace["value"].clone();
                    if path == "/session/process_pid" {
                        *target = json!(std::process::id() + 1);
                    }
                } else {
                    value[path.strip_prefix('/').unwrap()] = replace["value"].clone();
                }
            }
            if case.pointer("/generate/kind").and_then(Value::as_str)
                == Some("repeat-account-with-distinct-ids")
            {
                let row = value["accounts"][0].clone();
                value["accounts"] = Value::Array(
                    (0..129)
                        .map(|n| {
                            let mut row = row.clone();
                            row["id"] = json!(format!("account-{n}"));
                            row
                        })
                        .collect(),
                );
            }
            let mut line = case
                .get("line")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("OK codex-accounts {value}"));
            if case.pointer("/generate/kind").and_then(Value::as_str) == Some("append-ascii-spaces")
            {
                line.extend(std::iter::repeat_n(' ', 65537 - line.len()));
            }
            let request = binding["request"].as_str().unwrap();
            let result = waiting(&line, request).and_then(|pending| {
                if pending {
                    Ok(None)
                } else {
                    parse(
                        &line,
                        request,
                        binding["sid"].as_str().unwrap(),
                        binding["process_birth"].as_str().unwrap(),
                    )
                    .map(Some)
                }
            });
            match case["expect"].as_str().unwrap() {
                "snapshot" => assert!(
                    matches!(result, Ok(Some(_))),
                    "{}: {result:?}",
                    case["name"]
                ),
                "waiting" => assert!(matches!(result, Ok(None)), "{}", case["name"]),
                _ => assert!(result.is_err(), "{}", case["name"]),
            }
        }
        assert_eq!(cases.len(), 22);
    }

    #[tokio::test]
    async fn accounts_async_native_read_never_claims_supervision_and_polls_same_job() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        let request = "a".repeat(32);
        let command = format!(
            "@{} codex-accounts snapshot schema=1 request={request}",
            host.sid()
        );
        host.reply_once(
            &command,
            &format!("OK codex-accounts state=waiting request={request}\n"),
        );
        let mut snapshot = fixture();
        snapshot["session"]["sid"] = json!(host.sid());
        let read = tokio::spawn(async move {
            client
                .accounts_request("Fri Nov 6 09:00:00 2026", &request)
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !host.requests().contains(&command) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        host.reply_once(&command, &format!("OK codex-accounts {snapshot}\n"));
        assert!(read.await.unwrap().is_ok());
        assert_eq!(
            host.requests()
                .iter()
                .filter(|line| *line == &command)
                .count(),
            2
        );
        assert!(
            !host
                .requests()
                .iter()
                .any(|line| line.contains("harness claim"))
        );
    }

    #[tokio::test]
    async fn accounts_native_refusal_remains_typed_for_sanitized_ui() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        let request = "a".repeat(32);
        let command = format!(
            "@{} codex-accounts snapshot schema=1 request={request}",
            host.sid()
        );
        for refusal in [
            "ERR codex-accounts unsupported",
            "ERR codex-accounts denied",
        ] {
            host.reply_once(&command, &format!("{refusal}\n"));
            assert!(
                matches!(client.accounts_request("birth", &request).await, Err(Error::Refused(line)) if line == refusal)
            );
        }
    }
}
