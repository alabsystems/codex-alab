// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native exec grants. Capabilities never implement Debug or appear in errors.

use crate::Client;
use crate::Error;
use crate::renewal::Identity;
use crate::rotation::Origin;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[path = "exec_tree_grant.rs"]
pub mod tree;

pub const ENV: &str = "ATERM_CODEX_EXEC_HANDOFF";

#[derive(Clone, PartialEq, Eq)]
pub struct Ticket {
    pub id: String,
    pub capability: String,
}

impl Ticket {
    pub fn from_process_env(value: &str, pid: u32) -> Option<Self> {
        let mut parts = value.split('.');
        if parts.next()?.parse::<u32>().ok()? != pid {
            return None;
        }
        let ticket = Self {
            id: parts.next()?.into(),
            capability: parts.next()?.into(),
        };
        (parts.next().is_none() && ticket.valid()).then_some(ticket)
    }
    pub fn process_env(&self, pid: u32) -> String {
        format!("{pid}.{}.{}", self.id, self.capability)
    }
    fn valid(&self) -> bool {
        hex(&self.id, 32) && hex(&self.capability, 64)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Armed,
    Loaded,
    Consumed,
}

#[derive(Clone)]
pub struct Grant {
    pub ticket: Ticket,
    pub stage: Stage,
    pub origin: Origin,
    pub destination: Identity,
    pub auth_path: PathBuf,
    pub checkpoint: serde_json::Value,
    pub deadline_us: u64,
}

fn bad() -> Error {
    Error::Malformed("invalid native exec grant".into())
}
fn hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn encode(value: &[u8]) -> String {
    value.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode(value: &str) -> Result<String, Error> {
    if !value.len().is_multiple_of(2) || value.len() > 256 * 1024 {
        return Err(bad());
    }
    let bytes = value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| bad())?;
            if !hex(text, 2) {
                return Err(bad());
            }
            u8::from_str_radix(text, 16).map_err(|_| bad())
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).map_err(|_| bad())
}
fn fields(line: &str) -> Result<BTreeMap<&str, &str>, Error> {
    let mut values = BTreeMap::new();
    for word in line
        .strip_prefix("OK codex-exec-handoff ")
        .ok_or_else(bad)?
        .split_ascii_whitespace()
    {
        let (key, value) = word.split_once('=').ok_or_else(bad)?;
        if values.insert(key, value).is_some() {
            return Err(bad());
        }
    }
    Ok(values)
}
fn checkpoint_limit(checkpoint: &serde_json::Value) -> Result<Option<&str>, Error> {
    checkpoint
        .get("limit")
        .map(|value| {
            value
                .as_str()
                .filter(|id| codex_alab_core::wall_kind::valid_limit_id(id))
                .ok_or_else(bad)
        })
        .transpose()
}

fn parse(line: &str) -> Result<Grant, Error> {
    let f = fields(line)?;
    if f.len() != 12 {
        return Err(bad());
    }
    let get = |key| f.get(key).copied().ok_or_else(bad);
    let stage = match get("state")? {
        "armed" => Stage::Armed,
        "loaded" => Stage::Loaded,
        "consumed" => Stage::Consumed,
        _ => return Err(bad()),
    };
    let ticket = Ticket {
        id: get("id")?.into(),
        capability: get("cap")?.into(),
    };
    let origin = Origin {
        thread: get("thread")?.into(),
        turn: get("turn")?.into(),
        identity: Identity {
            account: get("source_account")?.into(),
            user: get("source_user")?.into(),
        },
    };
    let destination = Identity {
        account: get("target_account")?.into(),
        user: get("target_user")?.into(),
    };
    let target = Origin {
        identity: destination.clone(),
        ..origin.clone()
    };
    let deadline_us = get("deadline_us")?.parse().map_err(|_| bad())?;
    let auth_path = PathBuf::from(decode(get("auth")?)?);
    let checkpoint: serde_json::Value =
        serde_json::from_str(&decode(get("checkpoint")?)?).map_err(|_| bad())?;
    checkpoint_limit(&checkpoint)?;
    if !ticket.valid()
        || !origin.valid()
        || !target.valid()
        || destination.account == origin.identity.account
        || deadline_us == 0
        || !auth_path.is_absolute()
        || checkpoint.get("thread").and_then(|v| v.as_str()) != Some(&origin.thread)
        || checkpoint.get("turn").and_then(|v| v.as_str()) != Some(&origin.turn)
    {
        return Err(bad());
    }
    Ok(Grant {
        ticket,
        stage,
        origin,
        destination,
        auth_path,
        checkpoint,
        deadline_us,
    })
}

/// Whether this aterm serves exec contract 3. An answer that says it serves no such
/// contract is `Ok(false)` ([`crate::exec_route::not_served`]); no answer (a closed or
/// unanswered connection, a busy aterm, or any other refusal) is an error, which says
/// nothing about the contract.
pub async fn supported(client: &Client) -> Result<bool, Error> {
    served(
        client,
        "codex-exec-handoff info",
        "OK codex-exec-handoff contract=3",
    )
    .await
}

/// Whether `probe` is answered exactly `contract`: `Ok(false)` for an answer that says this
/// aterm serves no such contract, and an error for none, or for a refusal that does not say
/// so (an `ERR busy` or a main-thread refusal among them).
pub(crate) async fn served(client: &Client, probe: &str, contract: &str) -> Result<bool, Error> {
    let reply = client.on_self(probe).await?;
    let status = reply.status.trim();
    if status == contract {
        return Ok(true);
    }
    if crate::exec_route::not_served(status) {
        return Ok(false);
    }
    Err(crate::conn::refusal(status.to_owned()))
}

pub async fn arm(
    client: &Client,
    origin: &Origin,
    destination: &Identity,
    checkpoint: &serde_json::Value,
) -> Result<Grant, Error> {
    arm_using(client, origin, destination, checkpoint, true).await
}

/// Reconcile an unanswered arm on a fresh connection. Only the native host's
/// retained operation for this exact source can admit it without a new claim.
pub async fn reconcile_arm(
    client: &Client,
    origin: &Origin,
    destination: &Identity,
    checkpoint: &serde_json::Value,
) -> Result<Grant, Error> {
    if client.holds_claim() {
        return Err(bad());
    }
    arm_using(client, origin, destination, checkpoint, false).await
}

/// Recover a lost arm acknowledgement even if the provider wait has since ended. The
/// fresh connection can only match existing custody; give its ticket back if it is stale.
pub async fn reconcile_arm_if_current(
    client: &Client,
    origin: &Origin,
    destination: &Identity,
    checkpoint: &serde_json::Value,
    current: impl FnOnce() -> bool,
) -> Result<Option<Grant>, Error> {
    let grant = reconcile_arm(client, origin, destination, checkpoint).await?;
    if current() {
        Ok(Some(grant))
    } else {
        cancel(client, &grant.ticket).await?;
        Ok(None)
    }
}

async fn arm_using(
    client: &Client,
    origin: &Origin,
    destination: &Identity,
    checkpoint: &serde_json::Value,
    require_claim: bool,
) -> Result<Grant, Error> {
    let target = Origin {
        identity: destination.clone(),
        ..origin.clone()
    };
    if (require_claim && !client.holds_claim())
        || !origin.valid()
        || !target.valid()
        || target.identity.account == origin.identity.account
    {
        return Err(bad());
    }
    let json = serde_json::to_vec(checkpoint).map_err(|_| bad())?;
    if json.len() > 64 * 1024 {
        return Err(bad());
    }
    let mut request = format!(
        "codex-exec-handoff arm thread={} turn={} source_account={} source_user={} target_account={} target_user={} checkpoint={}",
        origin.thread,
        origin.turn,
        origin.identity.account,
        origin.identity.user,
        destination.account,
        destination.user,
        encode(&json)
    );
    let limit = checkpoint_limit(checkpoint)?;
    if limit.is_some() {
        client.harness_policy().await?;
    }
    crate::account_recovery::append_exec_limit(client, &mut request, limit)?;
    let reply = client.on_self(&request).await?;
    let grant = parse(&reply.status)?;
    if grant.stage != Stage::Armed
        || grant.origin != *origin
        || grant.destination != *destination
        || grant.checkpoint != *checkpoint
    {
        return Err(bad());
    }
    Ok(grant)
}
async fn call(client: &Client, ticket: &Ticket, verb: &str) -> Result<crate::conn::Reply, Error> {
    if !ticket.valid() {
        return Err(bad());
    }
    client
        .on_self(&format!(
            "codex-exec-handoff {verb} id={} cap={}",
            ticket.id, ticket.capability
        ))
        .await
}
pub async fn load(client: &Client, ticket: &Ticket) -> Result<Grant, Error> {
    let grant = parse(&call(client, ticket, "load").await?.status)?;
    if grant.ticket != *ticket || !matches!(grant.stage, Stage::Loaded | Stage::Consumed) {
        return Err(bad());
    }
    Ok(grant)
}
pub async fn consume(client: &Client, expected: &Grant) -> Result<(), Error> {
    let grant = parse(&call(client, &expected.ticket, "consume").await?.status)?;
    if grant.stage != Stage::Consumed
        || grant.ticket != expected.ticket
        || grant.origin != expected.origin
        || grant.destination != expected.destination
        || grant.checkpoint != expected.checkpoint
        || grant.auth_path != expected.auth_path
    {
        return Err(bad());
    }
    Ok(())
}
#[derive(Debug, PartialEq, Eq)]
pub enum Execution {
    Waiting,
    Ready,
}

fn execution_reply(status: &str, deadline_us: u64) -> Result<Execution, Error> {
    permission_reply(status, deadline_us, "execute")
}

fn permission_reply(status: &str, deadline_us: u64, ready: &str) -> Result<Execution, Error> {
    let f = fields(status)?;
    if f.len() != 2 || f.get("deadline_us").and_then(|s| s.parse::<u64>().ok()) != Some(deadline_us)
    {
        return Err(bad());
    }
    match f.get("state").copied() {
        Some(value) if value == ready => Ok(Execution::Ready),
        Some("waiting") => Ok(Execution::Waiting),
        _ => Err(bad()),
    }
}

pub async fn reserve(client: &Client, grant: &Grant) -> Result<Execution, Error> {
    let reply = call(client, &grant.ticket, "reserve").await?;
    permission_reply(&reply.status, grant.deadline_us, "reserved")
}

pub async fn execute(client: &Client, grant: &Grant) -> Result<Execution, Error> {
    let reply = call(client, &grant.ticket, "execute").await?;
    execution_reply(&reply.status, grant.deadline_us)
}

pub async fn complete(client: &Client, grant: &Grant) -> Result<bool, Error> {
    let reply = call(client, &grant.ticket, "complete").await?;
    let f = fields(&reply.status)?;
    if f.len() != 1 {
        return Err(bad());
    }
    match f.get("state").copied() {
        Some("completed") => Ok(true),
        Some("waiting") => Ok(false),
        _ => Err(bad()),
    }
}

pub fn remaining(deadline_us: u64) -> std::io::Result<std::time::Duration> {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime initializes the valid stack-allocated timespec.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let current = (now.tv_sec as u64)
        .saturating_mul(1_000_000)
        .saturating_add(now.tv_nsec as u64 / 1000);
    deadline_us
        .checked_sub(current)
        .filter(|left| *left != 0)
        .map(std::time::Duration::from_micros)
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "native exec deadline elapsed")
        })
}
pub async fn cancel(client: &Client, ticket: &Ticket) -> Result<(), Error> {
    let reply = call(client, ticket, "cancel").await?;
    let f = fields(&reply.status)?;
    if f.len() != 1 || f.get("state") != Some(&"cancelled") {
        return Err(bad());
    }
    Ok(())
}

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;

/// Immutable launch words plus fixed native metadata. No shell fallback,
/// process-global environment mutation or stdio reassignment is involved.
pub struct PreparedExec {
    path: CString,
    argv: Vec<CString>,
    env: Vec<CString>,
    capsule: Option<crate::exec_capsule::Capsule>,
}
impl PreparedExec {
    pub fn new(grant: &Grant) -> std::io::Result<Self> {
        let launch = codex_alab_core::session::shared()
            .launch()
            .ok_or_else(|| std::io::Error::other("launch snapshot unavailable"))?;
        let path = launch
            .current_exe
            .as_ref()
            .ok_or_else(|| std::io::Error::other("launch image unavailable"))?;
        let metadata = std::fs::metadata(path)?;
        if launch.exe_id != Some((metadata.dev(), metadata.ino())) {
            return Err(std::io::Error::other("launch image changed"));
        }
        let c = |bytes: &[u8]| {
            CString::new(bytes).map_err(|_| std::io::Error::other("invalid exec string"))
        };
        let argv = launch
            .args
            .iter()
            .map(|arg| c(arg.as_bytes()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut vars = launch.vars.clone();
        vars.retain(|(key, _)| {
            key != "CODEX_AUTH_PATH" && key != ENV && key != crate::exec_capsule::ENV
        });
        vars.push((
            "CODEX_AUTH_PATH".into(),
            grant.auth_path.as_os_str().to_owned(),
        ));
        vars.push((
            ENV.into(),
            grant.ticket.process_env(std::process::id()).into(),
        ));
        let env = vars
            .into_iter()
            .map(|(key, value)| {
                let mut bytes = key.as_bytes().to_vec();
                bytes.push(b'=');
                bytes.extend_from_slice(value.as_bytes());
                c(&bytes)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            path: c(path.as_os_str().as_bytes())?,
            argv,
            env,
            capsule: None,
        })
    }
    pub fn execute(self, deadline_us: u64) -> std::io::Error {
        if let Err(error) = remaining(deadline_us) {
            return error;
        }
        let mut argv = self.argv.iter().map(|v| v.as_ptr()).collect::<Vec<_>>();
        argv.push(std::ptr::null());
        let mut env = self.env.iter().map(|v| v.as_ptr()).collect::<Vec<_>>();
        env.push(std::ptr::null());
        let _fd_custody = match codex_alab_core::native_fd_custody::executing() {
            Ok(guard) => guard,
            Err(error) => return error,
        };
        if let Err(error) = remaining(deadline_us) {
            return error;
        }
        // On every failure the actual capsule drops before FD exclusion ends,
        // including a failed restoration of its CLOEXEC flag.
        let capsule = self.capsule;
        if let Some(capsule) = &capsule
            && let Err(error) = capsule.inherit()
        {
            return error;
        }
        // SAFETY: all pointers name live, NUL-terminated strings and both
        // arrays end with null. execve replaces this process atomically.
        unsafe {
            libc::execve(self.path.as_ptr(), argv.as_ptr(), env.as_ptr());
        }
        let error = std::io::Error::last_os_error();
        if let Some(capsule) = &capsule {
            let _ = capsule.stop_inheriting();
        }
        error
    }

    pub fn with_capsule(mut self, capsule: crate::exec_capsule::Capsule) -> std::io::Result<Self> {
        let prefix = format!("{}=", crate::exec_capsule::ENV);
        self.env
            .retain(|entry| !entry.as_bytes().starts_with(prefix.as_bytes()));
        self.env.push(
            CString::new(format!(
                "{}={}",
                crate::exec_capsule::ENV,
                capsule.reference()
            ))
            .map_err(|_| std::io::Error::other("invalid exec capsule reference"))?,
        );
        self.capsule = Some(capsule);
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_is_not_execution_and_cannot_extend_the_native_deadline() {
        assert_eq!(
            execution_reply("OK codex-exec-handoff state=waiting deadline_us=17", 17).unwrap(),
            Execution::Waiting
        );
        assert_eq!(
            execution_reply("OK codex-exec-handoff state=execute deadline_us=17", 17).unwrap(),
            Execution::Ready
        );
        assert!(execution_reply("OK codex-exec-handoff state=waiting deadline_us=18", 17).is_err());
        assert!(
            execution_reply("OK codex-exec-handoff state=consumed deadline_us=17", 17).is_err()
        );
        assert!(remaining(0).is_err());
        assert_eq!(
            permission_reply(
                "OK codex-exec-handoff state=reserved deadline_us=17",
                17,
                "reserved"
            )
            .unwrap(),
            Execution::Ready
        );
        assert!(
            execution_reply("OK codex-exec-handoff state=reserved deadline_us=17", 17).is_err()
        );
        assert!(
            permission_reply(
                "OK codex-exec-handoff state=execute deadline_us=17",
                17,
                "reserved"
            )
            .is_err()
        );
    }

    fn reply(state: &str) -> String {
        let thread = "00000000-0000-0000-0000-000000000001";
        let turn = "00000000-0000-0000-0000-000000000002";
        let checkpoint = serde_json::json!({"thread": thread, "turn": turn, "schema": null});
        format!(
            "OK codex-exec-handoff state={state} id={} cap={} deadline_us=123 thread={thread} turn={turn} source_account={} source_user={} target_account={} target_user={} auth={} checkpoint={}\n",
            "1".repeat(32),
            "2".repeat(64),
            "a".repeat(64),
            "b".repeat(64),
            "c".repeat(64),
            "d".repeat(64),
            encode(b"/private/accounts/new/auth.json"),
            encode(checkpoint.to_string().as_bytes())
        )
    }

    #[tokio::test]
    async fn named_exec_prepare_arm_and_reconciliation_keep_one_bucket() {
        use crate::exec_selection::Asked;
        use crate::exec_selection::Selection;
        use crate::exec_selection::ask;
        use codex_alab_core::wall_kind::WallKind;
        let dir = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(dir.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        host.set_policy_key("codex_rotation", "1");
        host.set_policy_key("accounts", "wall");
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        client.harness_policy().await.unwrap();
        client.claim().await.unwrap();
        let ordinary_reply = reply("armed");
        let ordinary = parse(&ordinary_reply).unwrap();
        let origin = &ordinary.origin;
        let target = &ordinary.destination;
        let limit = "codex_other";
        let model = WallKind::Limit(limit.into());
        let mut checkpoint = ordinary.checkpoint.clone();
        checkpoint["limit"] = limit.into();
        let args = format!(
            "thread={} turn={} source_account={} source_user={}",
            origin.thread, origin.turn, origin.identity.account, origin.identity.user
        );
        let dest = format!(
            "target_account={} target_user={}",
            target.account, target.user
        );
        let prefix = format!("@{} codex-exec-handoff", host.sid());
        let prepare = format!("{prefix} prepare {args} limit={limit}");
        let checkpoint_hex = encode(checkpoint.to_string().as_bytes());
        let armed = format!("{prefix} arm {args} {dest} checkpoint={checkpoint_hex} limit={limit}");
        let named_reply = ordinary_reply.replace(
            &encode(ordinary.checkpoint.to_string().as_bytes()),
            &checkpoint_hex,
        );
        let moves = || {
            host.requests()
                .into_iter()
                .filter(|line| {
                    line.contains("codex-exec-handoff prepare ")
                        || line.contains("codex-exec-handoff arm ")
                })
                .collect::<Vec<_>>()
        };

        // A client that already holds the ordinary claim checks both current grants.
        for (general, exec) in [("0", "0"), ("1", "0"), ("0", "1"), ("1", "yes")] {
            host.set_policy_key("codex_rotation_limits", general);
            host.set_policy_key("codex_exec_rotation_limits", exec);
            assert!(matches!(ask(&client, origin, &model).await, Asked::Unasked));
            assert!(arm(&client, origin, target, &checkpoint).await.is_err());
            assert!(
                reconcile_arm(&client, origin, target, &checkpoint)
                    .await
                    .is_err()
            );
            assert!(moves().is_empty());
        }
        host.set_policy_key("codex_rotation_limits", "1");
        host.set_policy_key("codex_exec_rotation_limits", "1");
        assert!(matches!(
            ask(&client, origin, &WallKind::Limit("Bad id".into())).await,
            Asked::Unasked
        ));
        for invalid in [serde_json::Value::Null, "codex".into(), "bad id".into()] {
            let mut bad = checkpoint.clone();
            bad["limit"] = invalid;
            assert!(arm(&client, origin, target, &bad).await.is_err());
        }
        assert!(moves().is_empty());
        host.reply_once(
            &prepare,
            &format!("OK codex-exec-handoff state=selected {args} {dest}\n"),
        );
        let Asked::Answered(Ok(Selection::Selected {
            origin: selected,
            destination,
        })) = ask(&client, origin, &model).await
        else {
            panic!("named destination not selected")
        };
        assert!(selected == *origin && destination == *target);
        host.reply_once(&armed, &named_reply);
        let grant = arm(&client, &selected, &destination, &checkpoint)
            .await
            .unwrap();
        assert_eq!(grant.checkpoint, checkpoint);

        let reconnected = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        assert!(!reconnected.holds_claim());
        host.reply_once(&armed, &named_reply);
        let grant = reconcile_arm(&reconnected, origin, target, &checkpoint)
            .await
            .unwrap();
        assert_eq!(grant.checkpoint, checkpoint);
        host.reply_once(&armed, &ordinary_reply);
        assert!(
            reconcile_arm(&reconnected, origin, target, &checkpoint)
                .await
                .is_err()
        );
        let cancel_request = format!(
            "{prefix} cancel id={} cap={}",
            grant.ticket.id, grant.ticket.capability
        );
        host.reply_once(&armed, &named_reply);
        host.reply_once(&cancel_request, "OK codex-exec-handoff state=cancelled\n");
        assert!(
            reconcile_arm_if_current(&reconnected, origin, target, &checkpoint, || false)
                .await
                .unwrap()
                .is_none()
        );
        assert!(host.requests().iter().any(|line| line == &cancel_request));
        assert!(
            reconcile_arm(&client, origin, target, &checkpoint)
                .await
                .is_err()
        );
        assert_eq!(
            moves(),
            [prepare, armed.clone(), armed.clone(), armed.clone(), armed]
        );

        let ticket = &grant.ticket;
        let command = |verb| format!("{prefix} {verb} id={} cap={}", ticket.id, ticket.capability);
        host.reply_once(
            &command("load"),
            &named_reply.replace("state=armed", "state=loaded"),
        );
        let loaded = load(&reconnected, ticket).await.unwrap();
        assert_eq!(loaded.checkpoint, checkpoint);
        host.reply_once(
            &command("consume"),
            &ordinary_reply.replace("state=armed", "state=consumed"),
        );
        assert!(consume(&reconnected, &loaded).await.is_err());
        host.reply_once(
            &command("consume"),
            &named_reply.replace("state=armed", "state=consumed"),
        );
        assert!(consume(&reconnected, &loaded).await.is_ok());
    }

    #[test]
    fn native_exec_grant_keeps_checkpoint_and_ticket_bound_to_origin() {
        for state in ["armed", "loaded", "consumed"] {
            let grant = parse(&reply(state)).unwrap();
            assert_eq!(
                grant.checkpoint["thread"].as_str(),
                Some(grant.origin.thread.as_str())
            );
            assert_eq!(
                grant.auth_path,
                PathBuf::from("/private/accounts/new/auth.json")
            );
            let env = grant.ticket.process_env(4242);
            assert!(Ticket::from_process_env(&env, 4242).is_some());
            assert!(Ticket::from_process_env(&env, 4243).is_none());
        }
        let original = reply("armed");
        for altered in [
            original.replace("codex-exec-handoff", "native-act"),
            original.replace("state=armed", "state=selected"),
            original.replace(&"c".repeat(64), &"a".repeat(64)),
            original.replace(
                "turn=00000000-0000-0000-0000-000000000002",
                "turn=00000000-0000-0000-0000-000000000003",
            ),
            format!("{} extra=1", original.trim()),
        ] {
            assert!(parse(&altered).is_err());
        }
    }
    #[test]
    fn direct_exec_preserves_pid_stdio_argv_and_exit_status() {
        use std::io::Write;
        use std::process::Command;
        use std::process::Stdio;
        let root = tempfile::tempdir().unwrap();
        let auth = root.path().join("auth.json");
        std::fs::write(&auth, "{}").unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "exec_grant::tests::direct_exec_child",
                "--ignored",
                "--nocapture",
            ])
            .env_remove(ENV)
            .env("CODEX_EXEC_TEST_CHILD", "1")
            .env("CODEX_EXEC_TEST_AUTH", &auth)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"inherited input")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(17));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains(&format!("before-exec:{pid}")), "{stdout}");
        assert!(
            stdout.contains(&format!("after-exec:{pid}:inherited input")),
            "{stdout}"
        );
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("after-exec-stderr")
        );
    }

    #[test]
    #[ignore = "child entry driven by direct_exec_preserves_pid_stdio_argv_and_exit_status"]
    fn direct_exec_child() {
        use std::io::Read;
        use std::io::Write;
        assert_eq!(std::env::var("CODEX_EXEC_TEST_CHILD").unwrap(), "1");
        if std::env::var(ENV)
            .ok()
            .and_then(|v| Ticket::from_process_env(&v, std::process::id()))
            .is_some()
        {
            assert_eq!(
                std::env::var_os("CODEX_AUTH_PATH"),
                std::env::var_os("CODEX_EXEC_TEST_AUTH")
            );
            assert_eq!(
                std::env::args().nth(2).as_deref(),
                Some("exec_grant::tests::direct_exec_child")
            );
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).unwrap();
            println!("after-exec:{}:{input}", std::process::id());
            eprintln!("after-exec-stderr");
            std::process::exit(17);
        }
        let path = std::env::current_exe().unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        codex_alab_core::session::shared().set_launch(codex_alab_core::session::Launch {
            args: std::env::args_os().collect(),
            vars: std::env::vars_os().collect(),
            current_exe: Some(path),
            exe_id: Some((meta.dev(), meta.ino())),
            pid: std::process::id(),
            ..Default::default()
        });
        let mut grant = parse(&reply("armed")).unwrap();
        grant.auth_path = std::env::var_os("CODEX_EXEC_TEST_AUTH").unwrap().into();
        println!("before-exec:{}", std::process::id());
        std::io::stdout().flush().unwrap();
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: writable initialized timespec used only by this child.
        assert_eq!(
            unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) },
            0
        );
        let deadline = u64::try_from(now.tv_sec).unwrap() * 1_000_000 + 60_000_000;
        panic!(
            "exec returned: {}",
            PreparedExec::new(&grant).unwrap().execute(deadline)
        );
    }
}
