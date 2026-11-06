// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native enrollment operations, independent of session account assignment.

use crate::Client;
use crate::Error;
use crate::accounts::Session;
use crate::accounts::request_id;
use crate::accounts::valid_session;
use crate::accounts::valid_text;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub schema: u32,
    pub request: String,
    pub session: Session,
    pub revision: String,
    pub capabilities: Capabilities,
    pub targets: Vec<Target>,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub enroll: bool,
    pub create: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub account_id: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub operation: String,
    pub account_id: String,
    pub revision: String,
    pub phase: Phase,
    #[serde(deserialize_with = "nullable_generation")]
    pub generation: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Running,
    AuthenticationRequired,
    Refused,
    CancelRequested,
    Cancelled,
    Applied,
}

impl Phase {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::AuthenticationRequired | Self::Refused | Self::Cancelled | Self::Applied
        )
    }

    pub fn can_cancel(self) -> bool {
        matches!(self, Self::Running | Self::CancelRequested)
    }

    pub fn blocks_new_enrollment(self) -> bool {
        matches!(
            self,
            Self::Running | Self::AuthenticationRequired | Self::CancelRequested
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Enrollment running",
            Self::AuthenticationRequired => "Account authentication required",
            Self::Refused => "Enrollment refused",
            Self::CancelRequested => "Cancellation requested; waiting for native confirmation",
            Self::Cancelled => "Enrollment cancelled",
            Self::Applied => "Enrollment complete; this session's account has not changed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Transport admission is pending. A durable provider operation is not yet confirmed.
    Waiting,
    Enrollment {
        phase: Phase,
        generation: Option<u64>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Waiting {
    schema: u32,
    operation: String,
    session: Session,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    schema: u32,
    operation: String,
    session: Session,
    account_id: String,
    phase: Phase,
    #[serde(deserialize_with = "nullable_generation")]
    generation: Option<u64>,
}

fn nullable_generation<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    Option::<u64>::deserialize(deserializer)
}

fn hex(text: &str, width: usize) -> bool {
    codex_alab_core::managed_auth::valid_hex(text, width)
}

/// Correlation for one explicit enrollment intent; retain it through all retries.
pub fn new_operation_id() -> String {
    request_id()
}

fn invalid() -> Error {
    Error::Malformed("invalid native enrollment response".into())
}

fn generation_valid(phase: Phase, generation: Option<u64>) -> bool {
    if phase == Phase::Applied {
        generation.is_some_and(|generation| generation > 0)
    } else {
        generation.is_none()
    }
}

fn parse_status(line: &str, request: &str, sid: &str, birth: &str) -> Result<Status, Error> {
    if line.len() > 64 * 1024 {
        return Err(invalid());
    }
    let status: Status = serde_json::from_str(
        line.strip_prefix("OK codex-accounts enrollment-status ")
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())?;
    if status.schema != 1
        || status.request != request
        || !valid_session(&status.session, sid, birth)
        || !hex(&status.revision, 64)
        || status.targets.len() > 128
        || status.operations.len() > 256
    {
        return Err(invalid());
    }
    let mut targets = std::collections::BTreeSet::new();
    for target in &status.targets {
        if !hex(&target.account_id, 64)
            || !valid_text(&target.label, 128)
            || !targets.insert(&target.account_id)
        {
            return Err(invalid());
        }
    }
    let mut operations = std::collections::BTreeSet::new();
    for operation in &status.operations {
        if !hex(&operation.operation, 32)
            || !hex(&operation.account_id, 64)
            || !hex(&operation.revision, 64)
            || !operations.insert(&operation.operation)
            || !generation_valid(operation.phase, operation.generation)
        {
            return Err(invalid());
        }
    }
    Ok(status)
}

fn parse_reply(
    line: &str,
    operation: &str,
    account: &str,
    sid: &str,
    birth: &str,
) -> Result<Reply, Error> {
    if line.len() > 64 * 1024 {
        return Err(invalid());
    }
    if let Some(json) = line.strip_prefix("OK codex-accounts-change waiting ") {
        let reply: Waiting = serde_json::from_str(json).map_err(|_| invalid())?;
        if reply.schema != 1
            || reply.operation != operation
            || !valid_session(&reply.session, sid, birth)
        {
            return Err(invalid());
        }
        return Ok(Reply::Waiting);
    }
    let reply: Enrollment = serde_json::from_str(
        line.strip_prefix("OK codex-accounts-change enrollment ")
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())?;
    if reply.schema != 1
        || reply.operation != operation
        || reply.account_id != account
        || !valid_session(&reply.session, sid, birth)
        || !generation_valid(reply.phase, reply.generation)
    {
        return Err(invalid());
    }
    Ok(Reply::Enrollment {
        phase: reply.phase,
        generation: reply.generation,
    })
}

impl Client {
    pub async fn enrollment_status(&self, birth: &str) -> Result<Status, Error> {
        let request = request_id();
        self.enrollment_status_request(birth, &request).await
    }

    async fn enrollment_status_request(&self, birth: &str, request: &str) -> Result<Status, Error> {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let reply = self
                    .on_self(&format!(
                        "codex-accounts enrollment-status schema=1 request={request}"
                    ))
                    .await?;
                if reply.status.starts_with("ERR ") {
                    return Err(crate::conn::refusal(reply.status));
                }
                if crate::accounts::waiting(&reply.status, request)? {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
                return parse_status(&reply.status, request, self.endpoint_sid(), birth);
            }
        })
        .await
        .map_err(|_| Error::Unanswered)?
    }

    /// Caller supplies one retained operation ID; retries must reuse it.
    pub async fn enrollment_start(
        &self,
        birth: &str,
        operation: &str,
        revision: &str,
        account: &str,
    ) -> Result<Reply, Error> {
        if !hex(revision, 64) {
            return Err(Error::BadRequest("invalid enrollment revision".into()));
        }
        self.enrollment_command(
            birth,
            operation,
            account,
            &format!("enroll schema=1 operation={operation} revision={revision} account={account}"),
        )
        .await
    }

    pub async fn enrollment_poll(
        &self,
        birth: &str,
        operation: &str,
        account: &str,
    ) -> Result<Reply, Error> {
        self.enrollment_command(
            birth,
            operation,
            account,
            &format!("enrollment-poll schema=1 operation={operation}"),
        )
        .await
    }

    pub async fn enrollment_cancel(
        &self,
        birth: &str,
        operation: &str,
        account: &str,
    ) -> Result<Reply, Error> {
        self.enrollment_command(
            birth,
            operation,
            account,
            &format!("enrollment-cancel schema=1 operation={operation}"),
        )
        .await
    }

    async fn enrollment_command(
        &self,
        birth: &str,
        operation: &str,
        account: &str,
        command: &str,
    ) -> Result<Reply, Error> {
        if !hex(operation, 32) || !hex(account, 64) {
            return Err(Error::BadRequest("invalid enrollment reference".into()));
        }
        let reply = self
            .on_self(&format!("codex-accounts-change {command}"))
            .await?;
        if reply.status.starts_with("ERR ") {
            return Err(crate::conn::refusal(reply.status));
        }
        parse_reply(
            &reply.status,
            operation,
            account,
            self.endpoint_sid(),
            birth,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use serde_json::json;

    fn session() -> Value {
        json!({"sid":"s-one","process_pid":std::process::id(),"process_birth":"birth","epoch":"host"})
    }

    fn status() -> Value {
        json!({"schema":1,"request":"a".repeat(32),"session":session(),"revision":"b".repeat(64),
            "capabilities":{"enroll":true,"create":false},
            "targets":[{"account_id":"c".repeat(64),"label":"Work"}],
            "operations":[{"operation":"d".repeat(32),"account_id":"c".repeat(64),"revision":"e".repeat(64),"phase":"authentication_required","generation":null}]})
    }

    fn read_status(value: &Value) -> Result<Status, Error> {
        parse_status(
            &format!("OK codex-accounts enrollment-status {value}"),
            &"a".repeat(32),
            "s-one",
            "birth",
        )
    }

    #[test]
    fn discovery_recovers_exact_operations_and_refuses_unbound_or_malformed_records() {
        let value = status();
        assert_eq!(
            read_status(&value).unwrap().operations[0].operation,
            "d".repeat(32)
        );
        let mut removed_target = value.clone();
        removed_target["targets"] = json!([]);
        assert!(
            read_status(&removed_target).is_ok(),
            "a removed source row must not hide the caller's retained operation"
        );
        for (pointer, replacement) in [
            ("/schema", json!(2)),
            ("/request", json!("wrong")),
            ("/session/process_pid", json!(std::process::id() + 1)),
            ("/session/process_birth", json!("another")),
            ("/session/sid", json!("s-other")),
            ("/session/epoch", json!("")),
            ("/revision", json!("invalid")),
            ("/targets/0/account_id", json!("raw-label")),
            ("/targets/0/label", json!("bad\nlabel")),
            ("/operations/0/phase", json!("ready")),
            ("/operations/0/generation", json!(1)),
        ] {
            let mut bad = value.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(read_status(&bad).is_err(), "{pointer}");
        }
        let mut duplicate = value.clone();
        duplicate["operations"]
            .as_array_mut()
            .unwrap()
            .push(value["operations"][0].clone());
        assert!(read_status(&duplicate).is_err());
        let mut unknown = value.clone();
        unknown["credential_path"] = json!("forbidden");
        assert!(read_status(&unknown).is_err());
        let mut too_many = value.clone();
        too_many["targets"] = json!(vec![value["targets"][0].clone(); 129]);
        assert!(read_status(&too_many).is_err());
    }

    #[test]
    fn waiting_is_not_running_and_completion_requires_a_real_generation() {
        let op = "d".repeat(32);
        let account = "c".repeat(64);
        let mut value = json!({"schema":1,"operation":op,"session":session()});
        let read = |prefix: &str, value: &Value| {
            parse_reply(
                &format!("OK codex-accounts-change {prefix} {value}"),
                &op,
                &account,
                "s-one",
                "birth",
            )
        };
        assert_eq!(read("waiting", &value).unwrap(), Reply::Waiting);
        value["account_id"] = json!(account);
        assert!(read("waiting", &value).is_err());
        value["phase"] = json!("running");
        value["generation"] = Value::Null;
        assert!(read("waiting", &value).is_err());
        assert_eq!(
            read("enrollment", &value).unwrap(),
            Reply::Enrollment {
                phase: Phase::Running,
                generation: None
            }
        );
        value["phase"] = json!("applied");
        assert!(read("enrollment", &value).is_err());
        value["generation"] = json!(0);
        assert!(read("enrollment", &value).is_err());
        value["generation"] = json!(7);
        assert_eq!(
            read("enrollment", &value).unwrap(),
            Reply::Enrollment {
                phase: Phase::Applied,
                generation: Some(7)
            }
        );
        value["operation"] = json!("f".repeat(32));
        assert!(read("enrollment", &value).is_err());
    }

    #[tokio::test]
    async fn native_discovery_reuses_correlation_and_operation_poll_never_restarts_enrollment() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        let request = "a".repeat(32);
        let command = format!(
            "@{} codex-accounts enrollment-status schema=1 request={request}",
            host.sid()
        );
        host.reply_once(
            &command,
            &format!("OK codex-accounts state=waiting request={request}\n"),
        );
        let mut value = status();
        value["session"]["sid"] = json!(host.sid());
        let task =
            tokio::spawn(async move { client.enrollment_status_request("birth", &request).await });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !host.requests().contains(&command) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        host.reply_once(
            &command,
            &format!("OK codex-accounts enrollment-status {value}\n"),
        );
        assert_eq!(
            task.await.unwrap().unwrap().operations[0].operation,
            "d".repeat(32)
        );
        assert_eq!(
            host.requests()
                .iter()
                .filter(|line| *line == &command)
                .count(),
            2
        );

        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        let op = "d".repeat(32);
        let account = "c".repeat(64);
        let poll = format!(
            "@{} codex-accounts-change enrollment-poll schema=1 operation={op}",
            host.sid()
        );
        let waiting = json!({"schema":1,"operation":op,"session":value["session"]});
        host.reply_once(
            &poll,
            &format!("OK codex-accounts-change waiting {waiting}\n"),
        );
        assert_eq!(
            client
                .enrollment_poll("birth", &op, &account)
                .await
                .unwrap(),
            Reply::Waiting
        );
        let mut complete = waiting;
        complete["account_id"] = json!(account);
        complete["phase"] = json!("applied");
        complete["generation"] = json!(9);
        host.reply_once(
            &poll,
            &format!("OK codex-accounts-change enrollment {complete}\n"),
        );
        assert_eq!(
            client
                .enrollment_poll("birth", &op, &account)
                .await
                .unwrap(),
            Reply::Enrollment {
                phase: Phase::Applied,
                generation: Some(9)
            }
        );
        assert!(
            !host
                .requests()
                .iter()
                .any(|line| line.contains(" enroll ") || line.contains("harness claim"))
        );
        assert!(matches!(
            client
                .enrollment_start("birth", "bad", &"b".repeat(64), &account)
                .await,
            Err(Error::BadRequest(_))
        ));
    }
}
