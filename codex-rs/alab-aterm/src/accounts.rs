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

/// aterm's `codex-accounts` snapshot, of either schema. Schema 1 says only what the pool
/// holds: no row is this session's, none is eligible, no capability is offered, and `pool` is
/// `None`. Schema 2 also says what `restart account=` may ask for this session: the row aterm
/// judges it to run on from the login its launch carried (`assignment.account_id`), the pool's
/// own choice, pin and rotation (`pool`), the rows a move would not be refused on record
/// (`eligible`), and whether the restart would be carried now (`capabilities`). It is aterm's
/// judgement, not a proof: the relaunch still proves the row it starts on.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub schema: u32,
    pub observed_at: i64,
    pub request: String,
    pub session: Session,
    pub revision: String,
    pub assignment: Assignment,
    /// Schema 2's pool view; `None` for schema 1.
    pub pool: Option<Pool>,
    pub accounts: Vec<Account>,
    pub capabilities: Capabilities,
}

/// Schema 1 as the wire carries it: `assignment` also carries `mode`, always `unknown`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireOne {
    schema: u32,
    observed_at: i64,
    request: String,
    session: Session,
    revision: String,
    assignment: AssignmentOne,
    accounts: Vec<Account>,
    capabilities: Capabilities,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignmentOne {
    account_id: Option<String>,
    mode: ModeOne,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ModeOne {
    Unknown,
}

/// Schema 2 as the wire carries it: `assignment` has no `mode`, and `pool` is required.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTwo {
    schema: u32,
    observed_at: i64,
    request: String,
    session: Session,
    revision: String,
    assignment: Assignment,
    pool: Pool,
    accounts: Vec<Account>,
    capabilities: Capabilities,
}

/// The snapshot schema a request asks for; a reply of the other is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Schema {
    One,
    Two,
}

impl Schema {
    fn number(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub sid: String,
    pub process_pid: u32,
    pub process_birth: String,
    pub epoch: String,
}

/// The row aterm names as this session's: always `None` in schema 1; in schema 2 the row the
/// session's launch login names in the roster, where aterm could judge one.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub account_id: Option<String>,
}

/// Schema 2's pool: the row a new launch would get (`choice_id`), whether the pool is pinned
/// to its active row, and the pool's rotation mode.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pool {
    pub choice_id: Option<String>,
    pub pinned: bool,
    pub rotation: Rotation,
}

/// `[harness] accounts` as aterm reports it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Rotation {
    Off,
    Wall,
    Early,
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
    /// Where the host names no row as this session's (schema 1 never does;
    /// schema 2 does where aterm could judge the session's launch login), any
    /// one able row may be the session's own, so two are required.
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

/// The snapshot `line` carries for `request` of `schema`, bound to this process of `sid`. A
/// reply of the other schema is refused, as is any field either schema does not carry. Schema
/// 1 offers nothing: every capability and every row's `eligible` must be false. Schema 2 may
/// offer `select` and `auto`, never `add`; `auto` only where `select` holds and the pool's
/// choice is a row other than the session's own.
fn parse(
    line: &str,
    schema: Schema,
    request: &str,
    sid: &str,
    birth: &str,
) -> Result<Snapshot, Error> {
    let invalid = || Error::Malformed("invalid native accounts snapshot".into());
    if line.len() > 64 * 1024 {
        return Err(invalid());
    }
    let body = line
        .strip_prefix("OK codex-accounts ")
        .ok_or_else(invalid)?;
    let snapshot = match schema {
        Schema::One => {
            let wire: WireOne = serde_json::from_str(body).map_err(|_| invalid())?;
            let ModeOne::Unknown = wire.assignment.mode;
            Snapshot {
                schema: wire.schema,
                observed_at: wire.observed_at,
                request: wire.request,
                session: wire.session,
                revision: wire.revision,
                assignment: Assignment {
                    account_id: wire.assignment.account_id,
                },
                pool: None,
                accounts: wire.accounts,
                capabilities: wire.capabilities,
            }
        }
        Schema::Two => {
            let wire: WireTwo = serde_json::from_str(body).map_err(|_| invalid())?;
            Snapshot {
                schema: wire.schema,
                observed_at: wire.observed_at,
                request: wire.request,
                session: wire.session,
                revision: wire.revision,
                assignment: wire.assignment,
                pool: Some(wire.pool),
                accounts: wire.accounts,
                capabilities: wire.capabilities,
            }
        }
    };
    let offers = schema == Schema::Two;
    let capabilities = &snapshot.capabilities;
    if snapshot.schema != schema.number()
        || !(0..=253_402_300_799).contains(&snapshot.observed_at)
        || snapshot.request != request
        || !valid_session(&snapshot.session, sid, birth)
        || !codex_alab_core::managed_auth::valid_hex(&snapshot.revision, 64)
        || snapshot.accounts.len() > 128
        || capabilities.add
        || (!offers && (capabilities.select || capabilities.auto))
        || (capabilities.auto && !capabilities.select)
    {
        return Err(invalid());
    }
    let mut ids = std::collections::BTreeSet::new();
    for account in &snapshot.accounts {
        let quota = &account.quota;
        if !valid_text(&account.id, 256)
            || !ids.insert(&account.id)
            || !valid_text(&account.label, 128)
            || (!offers && account.eligible)
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
    let own = snapshot.assignment.account_id.as_ref();
    let choice = snapshot
        .pool
        .as_ref()
        .and_then(|pool| pool.choice_id.as_ref());
    if own.is_some_and(|id| !ids.contains(id))
        || choice.is_some_and(|id| !ids.contains(id))
        || (capabilities.auto && (choice.is_none() || choice == own))
    {
        return Err(invalid());
    }
    Ok(snapshot)
}

/// What an aterm that answers no schema 2 snapshot says to one, and only that: the line on
/// which [`Client::accounts_snapshot_for_move`] asks schema 1 instead.
const SCHEMA_TWO_UNKNOWN: &str = "ERR codex-accounts invalid request";

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

    /// Ask the authenticated hosting session for its schema 1 snapshot, which offers nothing.
    /// No supervision claim or mutation is sent.
    pub async fn accounts_snapshot(&self, process_birth: &str) -> Result<Snapshot, Error> {
        let request = request_id();
        self.accounts_request(process_birth, Schema::One, &request)
            .await
    }

    /// Ask the hosting session for its schema 2 snapshot: the row aterm judges this session
    /// to run on, the pool's choice, and what `restart account=` may ask now. An aterm that
    /// answers no schema 2 says exactly `ERR codex-accounts invalid request`; on that line,
    /// and on no other, schema 1 is asked once, whose snapshot names no row as this session's
    /// and offers no move. No supervision claim or mutation is sent.
    pub async fn accounts_snapshot_for_move(&self, process_birth: &str) -> Result<Snapshot, Error> {
        match self
            .accounts_request(process_birth, Schema::Two, &request_id())
            .await
        {
            Err(Error::Refused(line)) if line == SCHEMA_TWO_UNKNOWN => {
                self.accounts_request(process_birth, Schema::One, &request_id())
                    .await
            }
            answer => answer,
        }
    }

    async fn accounts_request(
        &self,
        process_birth: &str,
        schema: Schema,
        request: &str,
    ) -> Result<Snapshot, Error> {
        let command = format!(
            "codex-accounts snapshot schema={} request={request}",
            schema.number()
        );
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
                return parse(
                    &reply.status,
                    schema,
                    request,
                    self.endpoint_sid(),
                    process_birth,
                );
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
        read_as(value, Schema::One)
    }

    fn read_as(value: &Value, schema: Schema) -> Result<Snapshot, Error> {
        parse(
            &format!("OK codex-accounts {value}"),
            schema,
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
        assert!(parse(&oversized, Schema::One, "", "", "").is_err());
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
                        Schema::One,
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

    /// aterm's schema 2 reply, verbatim from its wire (aterm branch s51fb4f/codex-account-move,
    /// `codex_accounts_control::snapshot_in`), with the abbreviated revision and epoch filled.
    const SCHEMA_TWO: &str = r#"OK codex-accounts {"accounts":[{"condition":"unseen","eligible":false,"enrolled":true,"held":false,"id":"4e262b3b4b5afd6f1aad178108c7a27f56c1f3684c89d40695bbbf2752aac4cd","label":"main","masked_email":null,"quota":{"five_hour_reset_at":null,"five_hour_used_percent":null,"observed_at":null,"stale":true,"weekly_reset_at":null,"weekly_used_percent":null}},{"condition":"unseen","eligible":true,"enrolled":true,"held":false,"id":"52f9c7fe9a2c1f06a11b8fa847d0036264d06e08b18fbf2808f9187c14dce3b1","label":"work","masked_email":"p***@example.com","quota":{"five_hour_reset_at":null,"five_hour_used_percent":null,"observed_at":null,"stale":true,"weekly_reset_at":null,"weekly_used_percent":null}}],"assignment":{"account_id":"4e262b3b4b5afd6f1aad178108c7a27f56c1f3684c89d40695bbbf2752aac4cd"},"capabilities":{"add":false,"auto":false,"select":true},"observed_at":1792700000,"pool":{"choice_id":"4e262b3b4b5afd6f1aad178108c7a27f56c1f3684c89d40695bbbf2752aac4cd","pinned":false,"rotation":"early"},"request":"0123456789abcdef0123456789abcdef","revision":"REVISION","schema":2,"session":{"epoch":"EPOCH","process_birth":"birth","process_pid":42,"sid":"bound-session"}}"#;

    const TWO_REQUEST: &str = "0123456789abcdef0123456789abcdef";
    const MAIN: &str = "4e262b3b4b5afd6f1aad178108c7a27f56c1f3684c89d40695bbbf2752aac4cd";
    const WORK: &str = "52f9c7fe9a2c1f06a11b8fa847d0036264d06e08b18fbf2808f9187c14dce3b1";

    /// The schema 2 example as JSON, bound to this process.
    fn fixture_two() -> Value {
        let line = SCHEMA_TWO
            .replace("REVISION", &"c".repeat(64))
            .replace("EPOCH", "host-epoch");
        let mut value: Value =
            serde_json::from_str(line.strip_prefix("OK codex-accounts ").unwrap()).unwrap();
        value["session"]["process_pid"] = json!(std::process::id());
        value
    }

    fn read_two(value: &Value) -> Result<Snapshot, Error> {
        parse(
            &format!("OK codex-accounts {value}"),
            Schema::Two,
            TWO_REQUEST,
            "bound-session",
            "birth",
        )
    }

    #[test]
    fn accounts_schema_two_parses_aterms_reply_and_names_the_row_and_the_pools_choice() {
        let snapshot = read_two(&fixture_two()).unwrap();
        assert_eq!(snapshot.schema, 2);
        assert_eq!(snapshot.assignment.account_id.as_deref(), Some(MAIN));
        assert_eq!(
            snapshot.pool,
            Some(Pool {
                choice_id: Some(MAIN.into()),
                pinned: false,
                rotation: Rotation::Early,
            })
        );
        assert!(snapshot.capabilities.select && !snapshot.capabilities.auto);
        assert!(!snapshot.accounts[0].eligible && snapshot.accounts[1].eligible);
        // The same reply, as a schema 1 request would read it: refused.
        assert!(
            parse(
                &format!("OK codex-accounts {}", fixture_two()),
                Schema::One,
                TWO_REQUEST,
                "bound-session",
                "birth",
            )
            .is_err()
        );
        // And a schema 1 reply to a schema 2 request.
        let mut one = fixture();
        one["request"] = json!(TWO_REQUEST);
        one["session"]["sid"] = json!("bound-session");
        one["session"]["process_birth"] = json!("birth");
        assert!(
            parse(
                &format!("OK codex-accounts {one}"),
                Schema::One,
                TWO_REQUEST,
                "bound-session",
                "birth"
            )
            .is_ok()
        );
        assert!(
            read_two(&one).is_err(),
            "a schema 1 reply to a schema 2 request"
        );
    }

    #[test]
    fn accounts_schema_two_refuses_what_its_wire_does_not_carry() {
        let refused = |change: &dyn Fn(&mut Value), why: &str| {
            let mut value = fixture_two();
            change(&mut value);
            assert!(read_two(&value).is_err(), "{why}");
        };
        refused(&|v| v["capabilities"]["add"] = json!(true), "add");
        refused(&|v| v["schema"] = json!(1), "schema 1 in a schema 2 shape");
        refused(&|v| v["schema"] = json!(3), "schema 3");
        refused(
            &|v| v["assignment"]["mode"] = json!("unknown"),
            "schema 1's mode",
        );
        refused(
            &|v| v["pool"]["extra"] = json!(true),
            "an unknown pool field",
        );
        refused(
            &|v| v["access_token"] = json!("x"),
            "an unknown top-level field",
        );
        refused(
            &|v| {
                v.as_object_mut().unwrap().remove("pool");
            },
            "no pool",
        );
        refused(
            &|v| v["pool"]["choice_id"] = json!("missing"),
            "a choice naming no row",
        );
        refused(
            &|v| v["assignment"]["account_id"] = json!("missing"),
            "an assignment naming no row",
        );
        for word in ["auto", "on", "", "Wall"] {
            refused(&|v| v["pool"]["rotation"] = json!(word), word);
        }
        refused(
            &|v| v["pool"]["pinned"] = json!("no"),
            "a pin that is no bool",
        );
        refused(
            &|v| v["accounts"][1]["eligible"] = json!(null),
            "eligible not a bool",
        );
        refused(
            &|v| {
                v["capabilities"]["select"] = json!(false);
                v["capabilities"]["auto"] = json!(true);
                v["pool"]["choice_id"] = json!(WORK);
            },
            "auto without select",
        );
        refused(
            &|v| v["capabilities"]["auto"] = json!(true),
            "auto onto the session's own row",
        );
        refused(
            &|v| {
                v["capabilities"]["auto"] = json!(true);
                v["pool"]["choice_id"] = json!(null);
            },
            "auto with no choice",
        );
        // What schema 2 may carry: any eligible and select, auto onto another row, no choice.
        let mut value = fixture_two();
        value["capabilities"]["auto"] = json!(true);
        value["pool"]["choice_id"] = json!(WORK);
        value["pool"]["pinned"] = json!(true);
        value["pool"]["rotation"] = json!("off");
        value["accounts"][0]["eligible"] = json!(true);
        assert!(read_two(&value).is_ok());
        let mut value = fixture_two();
        value["assignment"]["account_id"] = json!(null);
        value["pool"]["choice_id"] = json!(null);
        value["capabilities"]["select"] = json!(false);
        assert!(read_two(&value).is_ok());

        // Schema 1 still offers nothing.
        for pointer in [
            "/capabilities/select",
            "/capabilities/auto",
            "/accounts/0/eligible",
        ] {
            let mut changed = fixture();
            *changed.pointer_mut(pointer).unwrap() = json!(true);
            assert!(read(&changed).is_err(), "{pointer}");
        }
        let mut changed = fixture();
        changed["pool"] = json!({"choice_id": null, "pinned": false, "rotation": "wall"});
        assert!(read(&changed).is_err(), "schema 1 carries no pool");
    }

    #[test]
    fn accounts_schema_two_counts_one_spare_beside_the_row_it_names() {
        // A session walled on `main` with exactly one other able row, `work`.
        let mut value = fixture_two();
        value["accounts"][0]["condition"] = json!("limited");
        value["accounts"][1]["condition"] = json!("usable");
        let two = read_two(&value).unwrap();
        assert!(
            two.another_account_can_run(),
            "schema 2 names main as its own"
        );
        // The same pool where no row is named (schema 1 never names one): `work` may be the
        // session's own, so it is not known to be another.
        let mut unnamed = two;
        unnamed.assignment.account_id = None;
        assert!(!unnamed.another_account_can_run());
        // The named row's own ability never counts.
        let mut value = fixture_two();
        value["accounts"][1]["condition"] = json!("limited");
        assert!(!read_two(&value).unwrap().another_account_can_run());
    }

    /// The snapshot lines a fake host was sent, without the session selector and request id.
    fn snapshot_asks(host: &crate::fake::FakeAterm) -> Vec<String> {
        host.requests()
            .iter()
            .filter_map(|line| {
                let rest = line.split_once(' ')?.1;
                let (head, _) = rest.rsplit_once(" request=")?;
                head.starts_with("codex-accounts snapshot ")
                    .then(|| head.to_owned())
            })
            .collect()
    }

    #[tokio::test]
    async fn accounts_snapshot_for_move_asks_schema_two_and_falls_back_to_schema_one_once() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        let rows = json!([fixture()["accounts"][0].clone()]);
        host.set_account_pool(Some(("birth", Some("safe-account"), rows)));
        host.set_account_moves(crate::fake::FakeAccountMoves {
            choice: Some("safe-account".into()),
            pinned: true,
            rotation: "early".into(),
            select: true,
            auto: false,
        });

        // A new host: one schema 2 request.
        let snapshot = client.accounts_snapshot_for_move("birth").await.unwrap();
        assert_eq!(snapshot.schema, 2);
        assert!(snapshot.capabilities.select);
        assert_eq!(
            snapshot.assignment.account_id.as_deref(),
            Some("safe-account")
        );
        assert_eq!(snapshot_asks(&host), ["codex-accounts snapshot schema=2"]);

        // An aterm before schema 2: schema 2, then schema 1 once, which offers nothing.
        host.answer_account_schema_two(false);
        let snapshot = client.accounts_snapshot_for_move("birth").await.unwrap();
        assert_eq!(snapshot.schema, 1);
        assert_eq!(snapshot.pool, None);
        assert_eq!(
            snapshot.capabilities,
            Capabilities {
                add: false,
                select: false,
                auto: false
            }
        );
        assert!(snapshot.accounts.iter().all(|row| !row.eligible));
        assert_eq!(
            snapshot_asks(&host)[1..],
            [
                "codex-accounts snapshot schema=2",
                "codex-accounts snapshot schema=1"
            ]
        );

        // Any other refusal is not an aterm before schema 2: nothing more is asked.
        host.answer_account_schema_two(true);
        let before = snapshot_asks(&host).len();
        host.set_account_pool(None);
        assert!(matches!(
            client.accounts_snapshot_for_move("birth").await,
            Err(Error::Refused(line)) if line.starts_with("ERR unknown")
        ));
        assert_eq!(snapshot_asks(&host).len(), before + 1);

        // The schema 1 read asks exactly the schema 1 line, of a new host too.
        host.set_account_pool(Some(("birth", None, json!([]))));
        let before = snapshot_asks(&host).len();
        let snapshot = client.accounts_snapshot("birth").await.unwrap();
        assert_eq!(snapshot.schema, 1);
        assert_eq!(
            snapshot_asks(&host)[before..],
            ["codex-accounts snapshot schema=1"]
        );
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
                .accounts_request("Fri Nov 6 09:00:00 2026", Schema::One, &request)
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
                matches!(client.accounts_request("birth", Schema::One, &request).await, Err(Error::Refused(line)) if line == refusal)
            );
        }
    }
}
