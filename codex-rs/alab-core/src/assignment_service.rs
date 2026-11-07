// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Typed native assignment operations. These carry no credential path or token.

use crate::assignment::Binding;
use crate::assignment::Generations;
use crate::managed_auth::valid_hex;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

fn unavailable() -> io::Error {
    io::Error::other("Native account assignment must be reconciled.")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Manual,
    Auto,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Waiting,
    Prepared,
    Installing,
    AwaitingAck,
    Applied,
    Cancelled,
    Refused,
    Reconciling,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Busy,
    Unavailable,
    Denied,
    StaleRevision,
    SourceChanged,
    TargetUnavailable,
    Policy,
    Cancelled,
    Superseded,
    NeedsReconciliation,
    UnsupportedConsumer,
    AuthenticationRequired,
}
impl Reason {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "busy" => Self::Busy,
            "unavailable" => Self::Unavailable,
            "denied" => Self::Denied,
            "stale_revision" => Self::StaleRevision,
            "source_changed" => Self::SourceChanged,
            "target_unavailable" => Self::TargetUnavailable,
            "policy" => Self::Policy,
            "cancelled" => Self::Cancelled,
            "superseded" => Self::Superseded,
            "needs_reconciliation" => Self::NeedsReconciliation,
            "unsupported_consumer" => Self::UnsupportedConsumer,
            "authentication_required" => Self::AuthenticationRequired,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub operation: String,
    pub phase: Phase,
    pub boundary: Option<String>,
    pub assignment_generation: Option<u64>,
    pub credential_generation: Option<u64>,
    pub target: Option<String>,
    pub mode: Option<Mode>,
    pub reason: Option<Reason>,
}
impl Status {
    pub fn validate(&self, operation: &str) -> io::Result<()> {
        let opaque_terminal = matches!(self.phase, Phase::Cancelled | Phase::Refused)
            && self.target.is_none()
            && self.boundary.is_none()
            && self.assignment_generation.is_none()
            && self.credential_generation.is_none();
        if self.operation != operation
            || !valid_hex(operation, 32)
            || self
                .boundary
                .as_ref()
                .is_some_and(|value| !valid_hex(value, 32))
            || self
                .target
                .as_ref()
                .is_some_and(|value| !valid_hex(value, 64))
            || self.assignment_generation == Some(0)
            || self.credential_generation == Some(0)
            || self.assignment_generation.is_some() != self.credential_generation.is_some()
            || (self.mode.is_none() && self.phase != Phase::Waiting && !opaque_terminal)
            || (self.phase == Phase::Prepared
                && (self.target.is_none()
                    || self.boundary.is_some()
                    || self.assignment_generation.is_some()))
            || (matches!(self.phase, Phase::AwaitingAck | Phase::Applied)
                && (self.boundary.is_none()
                    || self.assignment_generation.is_none()
                    || self.target.is_none()))
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    Prepare {
        operation: String,
        pool_revision: String,
        expected_assignment: u64,
        selection: Option<String>,
    },
    Commit {
        operation: String,
        boundary: String,
        source_credential_generation: u64,
        generations: Generations,
    },
    Poll {
        operation: String,
    },
    Cancel {
        operation: String,
    },
    Ack {
        operation: String,
        boundary: String,
        binding: Binding,
    },
}
impl Command {
    pub fn operation(&self) -> &str {
        match self {
            Self::Prepare { operation, .. }
            | Self::Commit { operation, .. }
            | Self::Poll { operation }
            | Self::Cancel { operation }
            | Self::Ack { operation, .. } => operation,
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        if !valid_hex(self.operation(), 32) {
            return Err(unavailable());
        }
        let valid = match self {
            Self::Prepare {
                pool_revision,
                selection,
                ..
            } => {
                valid_hex(pool_revision, 64)
                    && selection.as_ref().is_none_or(|value| valid_hex(value, 64))
            }
            Self::Commit {
                boundary,
                source_credential_generation,
                ..
            } => valid_hex(boundary, 32) && *source_credential_generation > 0,
            Self::Ack {
                boundary, binding, ..
            } => {
                valid_hex(boundary, 32)
                    && valid_hex(&binding.account, 64)
                    && valid_hex(&binding.user, 64)
                    && valid_hex(&binding.digest, 64)
                    && binding.assignment_generation > 0
                    && binding.credential_generation > 0
            }
            Self::Poll { .. } | Self::Cancel { .. } => true,
        };
        if valid { Ok(()) } else { Err(unavailable()) }
    }
}

/// Private prepared-target policy input, authenticated by the owning native transport.
/// This is not a credential source or permission to install a grant.
#[derive(Clone)]
pub struct Target {
    pub operation: String,
    pub account: String,
    pub user: String,
    pub source_grant: String,
    pub target_grant: String,
}
impl Target {
    pub fn validate(&self, operation: &str) -> io::Result<()> {
        if self.operation != operation
            || !valid_hex(operation, 32)
            || [
                &self.account,
                &self.user,
                &self.source_grant,
                &self.target_grant,
            ]
            .into_iter()
            .any(|value| !valid_hex(value, 64))
        {
            return Err(unavailable());
        }
        Ok(())
    }
}
impl std::fmt::Debug for Target {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PreparedTarget(<private>)")
    }
}

pub type TargetFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Target>> + Send + 'a>>;
pub type ReplyFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Status>> + Send + 'a>>;
pub trait Host: Send + Sync {
    fn command<'a>(&'a self, command: &'a Command) -> ReplyFuture<'a>;
    /// Older hosts cannot authorize a policy preview by returning public status alone.
    fn target<'a>(&'a self, _operation: &'a str) -> TargetFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
}
static HOST: OnceLock<Arc<dyn Host>> = OnceLock::new();
pub fn install(host: Arc<dyn Host>) {
    let _ = HOST.set(host);
}
pub fn available() -> bool {
    HOST.get().is_some()
}
pub async fn command(command: &Command) -> io::Result<Status> {
    command.validate()?;
    let reply = HOST.get().ok_or_else(unavailable)?.command(command).await?;
    reply.validate(command.operation())?;
    Ok(reply)
}

/// Fetch a private prepared-target preview; absence never falls back to a cached account label.
pub async fn target(operation: &str) -> io::Result<Target> {
    if !valid_hex(operation, 32) {
        return Err(unavailable());
    }
    let target = HOST
        .get()
        .ok_or_else(unavailable)?
        .target(operation)
        .await?;
    target.validate(operation)?;
    Ok(target)
}

/// Reconcile one immutable native operation without releasing its caller's
/// owned admission. Transport uncertainty only polls; exact mutation replay
/// requires a validated native phase proving that mutation is still pending.
pub async fn reconcile_command<F, Fut, W>(
    command: Command,
    mut exchange: F,
    mut waiting: W,
) -> io::Result<Status>
where
    F: FnMut(Command) -> Fut,
    Fut: Future<Output = io::Result<Status>>,
    W: FnMut(),
{
    command.validate()?;
    let mut next = command.clone();
    loop {
        let sent_ack = matches!(next, Command::Ack { .. });
        if let Ok(status) = exchange(next).await
            && status.validate(command.operation()).is_ok()
        {
            let replay = matches!(
                (&command, status.phase),
                (Command::Commit { .. }, Phase::Prepared)
            ) || (!sent_ack
                && matches!(
                    (&command, status.phase),
                    (Command::Ack { .. }, Phase::Applied)
                ));
            let terminal = matches!(status.phase, Phase::Cancelled | Phase::Refused)
                || matches!(
                    (&command, status.phase),
                    (Command::Prepare { .. }, Phase::Prepared)
                        | (Command::Commit { .. }, Phase::AwaitingAck | Phase::Applied)
                        | (Command::Ack { .. }, Phase::AwaitingAck)
                )
                || (sent_ack
                    && matches!(
                        (&command, status.phase),
                        (Command::Ack { .. }, Phase::Applied)
                    ));
            if terminal {
                return Ok(status);
            }
            next = if replay {
                command.clone()
            } else {
                Command::Poll {
                    operation: command.operation().into(),
                }
            };
        } else {
            next = Command::Poll {
                operation: command.operation().into(),
            };
        }
        waiting();
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod reconciliation_tests {
    use super::*;
    use crate::assignment::Domain;
    use crate::assignment::Intent;
    use crate::assignment::Selection;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn status(phase: Phase) -> Status {
        let installed = matches!(phase, Phase::AwaitingAck | Phase::Applied);
        Status {
            operation: "a".repeat(32),
            phase,
            boundary: installed.then(|| "b".repeat(32)),
            assignment_generation: installed.then_some(3),
            credential_generation: installed.then_some(7),
            target: Some("c".repeat(64)),
            mode: Some(Mode::Manual),
            reason: None,
        }
    }
    fn prepare() -> Command {
        Command::Prepare {
            operation: "a".repeat(32),
            pool_revision: "d".repeat(64),
            expected_assignment: 2,
            selection: Some("c".repeat(64)),
        }
    }
    fn binding() -> Binding {
        Binding {
            account: "c".repeat(64),
            user: "e".repeat(64),
            digest: "f".repeat(64),
            credential_generation: 7,
            assignment_generation: 3,
            generations: Generations {
                auth: 1,
                owner: 2,
                source: 3,
                login: 4,
            },
        }
    }
    async fn run(command: Command, replies: Vec<io::Result<Status>>) -> Vec<Command> {
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let domain = Arc::new(Domain::default());
        domain
            .request(Intent {
                operation: "a".repeat(32),
                thread: "12345678-1234-1234-1234-123456789abc".into(),
                selection: Selection::Manual {
                    account: "c".repeat(64),
                    revision: "d".repeat(64),
                },
            })
            .unwrap();
        let mut boundary = domain
            .retire(&"a".repeat(32))
            .await
            .unwrap()
            .wait_drained()
            .await;
        boundary.set_native_intent("a".repeat(32)).unwrap();
        let result = reconcile_command(
            command,
            |command| {
                assert!(
                    domain.admit_work(None).is_err(),
                    "transport uncertainty must keep the real domain closed"
                );
                assert!(boundary.protects(&domain, &"a".repeat(32), Some(&"a".repeat(32))));
                sent.lock().unwrap().push(command);
                let reply = replies
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unexpected mutation or poll");
                std::future::ready(reply)
            },
            || {},
        )
        .await
        .unwrap();
        assert!(replies.lock().unwrap().is_empty());
        assert_eq!(result.operation, "a".repeat(32));
        assert!(domain.admit_work(None).is_err());
        // The helper cannot reopen the gate or fabricate destination readiness.
        drop(boundary);
        assert!(domain.admit_work(None).is_err());
        Arc::try_unwrap(sent).unwrap().into_inner().unwrap()
    }
    fn lost() -> io::Result<Status> {
        Err(io::Error::other("fixture lost reply"))
    }

    #[tokio::test(start_paused = true)]
    async fn lost_prepare_reply_polls_the_same_operation_without_another_prepare() {
        let sent = run(
            prepare(),
            vec![
                lost(),
                Ok(status(Phase::Waiting)),
                Ok(status(Phase::Prepared)),
            ],
        )
        .await;
        assert!(matches!(
            sent.as_slice(),
            [
                Command::Prepare { .. },
                Command::Poll { .. },
                Command::Poll { .. }
            ]
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn commit_replay_requires_native_prepared_proof_and_retains_exact_body() {
        let command = Command::Commit {
            operation: "a".repeat(32),
            boundary: "b".repeat(32),
            source_credential_generation: 6,
            generations: Generations {
                auth: 1,
                owner: 2,
                source: 3,
                login: 4,
            },
        };
        let sent = run(
            command,
            vec![
                lost(),
                Ok(status(Phase::Installing)),
                Ok(status(Phase::Prepared)),
                Ok(status(Phase::AwaitingAck)),
            ],
        )
        .await;
        assert!(matches!(
            sent.as_slice(),
            [
                Command::Commit { .. },
                Command::Poll { .. },
                Command::Poll { .. },
                Command::Commit { .. }
            ]
        ));
        match (&sent[0], &sent[3]) {
            (
                Command::Commit {
                    operation: a,
                    boundary: b,
                    source_credential_generation: c,
                    generations: d,
                },
                Command::Commit {
                    operation: e,
                    boundary: f,
                    source_credential_generation: g,
                    generations: h,
                },
            ) => assert!(a == e && b == f && c == g && d == h),
            _ => unreachable!(),
        }
    }
    #[tokio::test(start_paused = true)]
    async fn lost_ack_reply_requires_exact_ack_receipt_after_poll_without_opening_gate() {
        let command = Command::Ack {
            operation: "a".repeat(32),
            boundary: "b".repeat(32),
            binding: binding(),
        };
        let sent = run(
            command,
            vec![
                lost(),
                Ok(status(Phase::Applied)),
                Ok(status(Phase::Applied)),
            ],
        )
        .await;
        assert!(matches!(
            sent.as_slice(),
            [
                Command::Ack { .. },
                Command::Poll { .. },
                Command::Ack { .. }
            ]
        ));
        match (&sent[0], &sent[2]) {
            (
                Command::Ack {
                    operation: a,
                    boundary: b,
                    binding: c,
                },
                Command::Ack {
                    operation: d,
                    boundary: e,
                    binding: f,
                },
            ) => assert!(a == d && b == e && c == f),
            _ => unreachable!(),
        }
    }
    #[tokio::test(start_paused = true)]
    async fn awaiting_ack_returns_to_owned_actor_for_fresh_source_instead_of_stale_body_replay() {
        let command = Command::Ack {
            operation: "a".repeat(32),
            boundary: "b".repeat(32),
            binding: binding(),
        };
        let sent = run(command, vec![lost(), Ok(status(Phase::AwaitingAck))]).await;
        assert!(matches!(
            sent.as_slice(),
            [Command::Ack { .. }, Command::Poll { .. }]
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn an_uncorrelated_reply_cannot_authorize_mutation_replay() {
        let mut wrong = status(Phase::Prepared);
        wrong.operation = "1".repeat(32);
        let sent = run(prepare(), vec![Ok(wrong), Ok(status(Phase::Prepared))]).await;
        assert!(matches!(
            sent.as_slice(),
            [Command::Prepare { .. }, Command::Poll { .. }]
        ));
    }
}
