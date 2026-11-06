// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Local enrollment view messages carry references and safe metadata, never credentials.

#[cfg(unix)]
use codex_alab_aterm::accounts::Session;
#[cfg(unix)]
use codex_alab_aterm::enrollment::Operation;
#[cfg(unix)]
use codex_alab_aterm::enrollment::Reply;
#[cfg(unix)]
use codex_alab_aterm::enrollment::Status;

#[derive(Default)]
pub struct View {
    pub busy: bool,
}

#[derive(Clone, Debug)]
#[cfg(unix)]
pub struct Active {
    pub operation: String,
    pub account_id: String,
    pub revision: String,
}

#[cfg(unix)]
impl From<Operation> for Active {
    fn from(operation: Operation) -> Self {
        Self {
            operation: operation.operation,
            account_id: operation.account_id,
            revision: operation.revision,
        }
    }
}

#[derive(Clone, Debug)]
#[cfg(unix)]
pub enum Action {
    Discover {
        label: Option<String>,
    },
    Enroll {
        account_id: String,
        revision: String,
        session: Session,
    },
    Start(Active),
    Observe(Operation),
    Poll(Active),
    Cancel(Active),
}

#[derive(Debug)]
#[cfg(unix)]
pub enum Event {
    Action(Action),
    Status {
        status: Status,
        label: Option<String>,
    },
    Progress {
        active: Active,
        reply: Reply,
    },
    Failed(&'static str),
}

#[cfg(not(unix))]
#[derive(Debug)]
pub enum Event {}

// This retains correlation only. Native admission and the durable book own authority.
#[cfg(unix)]
type IntentKey = (String, u32, String, String);
#[cfg(unix)]
type Intents = std::sync::Mutex<std::collections::BTreeMap<IntentKey, Active>>;
#[cfg(unix)]
fn intents() -> &'static Intents {
    static INTENTS: std::sync::OnceLock<Intents> = std::sync::OnceLock::new();
    INTENTS.get_or_init(Default::default)
}

#[cfg(unix)]
pub fn retained(session: &Session) -> Vec<Active> {
    intents()
        .lock()
        .map(|intents| {
            intents
                .iter()
                .filter(|((sid, pid, birth, _), _)| {
                    sid == &session.sid
                        && pid == &session.process_pid
                        && birth == &session.process_birth
                })
                .map(|(_, active)| active.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(unix)]
pub fn retry_action(active: Active, status: &Status) -> Action {
    if active.revision == status.revision
        && status
            .targets
            .iter()
            .any(|target| target.account_id == active.account_id)
    {
        Action::Start(active)
    } else {
        Action::Poll(active)
    }
}

/// Record before the first send, and never discard an uncertain operation on view close.
#[cfg(unix)]
pub fn admit_intent(
    session: &Session,
    account_id: String,
    revision: String,
) -> Result<Action, &'static str> {
    let mut intents = intents()
        .lock()
        .map_err(|_| "Enrollment requests are unavailable in this session.")?;
    let key = (
        session.sid.clone(),
        session.process_pid,
        session.process_birth.clone(),
        account_id.clone(),
    );
    if let Some(active) = intents.get(&key) {
        return Ok(Action::Poll(active.clone()));
    }
    if intents.len() >= 256 {
        return Err("Too many pending enrollment requests. Check their status first.");
    }
    let active = Active {
        operation: codex_alab_aterm::enrollment::new_operation_id(),
        account_id,
        revision,
    };
    intents.insert(key, active.clone());
    Ok(Action::Start(active))
}

/// A terminal observation retires correlation only for this exact authenticated caller/intent.
#[cfg(unix)]
pub fn reconcile(session: &Session, operations: &[Operation]) {
    let Ok(mut intents) = intents().lock() else {
        return;
    };
    for operation in operations {
        if !matches!(
            operation.phase,
            codex_alab_aterm::enrollment::Phase::Applied
                | codex_alab_aterm::enrollment::Phase::Refused
                | codex_alab_aterm::enrollment::Phase::Cancelled
        ) {
            continue;
        }
        let key = (
            session.sid.clone(),
            session.process_pid,
            session.process_birth.clone(),
            operation.account_id.clone(),
        );
        if intents.get(&key).is_some_and(|active| {
            active.operation == operation.operation && active.revision == operation.revision
        }) {
            intents.remove(&key);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn uncertain_intent_survives_view_drop_but_never_transfers_to_a_new_process_birth() {
        let mut session = Session {
            sid: "s-intent-fixture".into(),
            process_pid: std::process::id(),
            process_birth: "first birth".into(),
            epoch: "first host".into(),
        };
        let Action::Start(original) =
            admit_intent(&session, "c".repeat(64), "b".repeat(64)).unwrap()
        else {
            panic!("new intent");
        };
        let pane = std::sync::Arc::new(std::sync::Mutex::new(View::default()));
        drop(pane);
        session.epoch = "replacement host".into();
        let Action::Poll(retry) = admit_intent(&session, "c".repeat(64), "f".repeat(64)).unwrap()
        else {
            panic!("must retain original intent");
        };
        assert_eq!(retry.operation, original.operation);
        assert_eq!(retry.revision, original.revision);
        assert_eq!(retained(&session).len(), 1);
        session.process_birth = "new process birth".into();
        assert!(retained(&session).is_empty());
        let Action::Start(new) = admit_intent(&session, "c".repeat(64), "b".repeat(64)).unwrap()
        else {
            panic!("new caller intent");
        };
        assert_ne!(new.operation, original.operation);
    }

    #[test]
    fn terminal_reconciliation_allows_a_new_explicit_intent_but_stale_reply_cannot_retire_it() {
        let session = Session {
            sid: "s-terminal-fixture".into(),
            process_pid: std::process::id(),
            process_birth: "birth".into(),
            epoch: "epoch".into(),
        };
        let Action::Start(first) = admit_intent(&session, "a".repeat(64), "b".repeat(64)).unwrap()
        else {
            panic!("first");
        };
        let mut record = Operation {
            operation: first.operation.clone(),
            account_id: first.account_id.clone(),
            revision: first.revision.clone(),
            phase: codex_alab_aterm::enrollment::Phase::AuthenticationRequired,
            generation: None,
        };
        reconcile(&session, &[record.clone()]);
        assert_eq!(retained(&session)[0].operation, first.operation);
        record.phase = codex_alab_aterm::enrollment::Phase::Applied;
        record.generation = Some(1);
        reconcile(&session, &[record.clone()]);
        assert!(retained(&session).is_empty());
        let Action::Start(second) = admit_intent(&session, "a".repeat(64), "c".repeat(64)).unwrap()
        else {
            panic!("new explicit intent");
        };
        reconcile(&session, &[record]);
        assert_eq!(retained(&session)[0].operation, second.operation);
        assert_ne!(first.operation, second.operation);
    }

    #[test]
    fn removed_target_or_changed_source_can_only_check_the_original_request() {
        let session = Session {
            sid: "s-retry".into(),
            process_pid: std::process::id(),
            process_birth: "birth".into(),
            epoch: "host".into(),
        };
        let active = Active {
            operation: "a".repeat(32),
            account_id: "b".repeat(64),
            revision: "c".repeat(64),
        };
        let mut status = Status {
            schema: 1,
            request: "d".repeat(32),
            session,
            revision: active.revision.clone(),
            capabilities: codex_alab_aterm::enrollment::Capabilities {
                enroll: true,
                create: false,
            },
            targets: vec![],
            operations: vec![],
        };
        assert!(matches!(
            retry_action(active.clone(), &status),
            Action::Poll(_)
        ));
        status.targets.push(codex_alab_aterm::enrollment::Target {
            account_id: active.account_id.clone(),
            label: "Work".into(),
        });
        assert!(matches!(
            retry_action(active.clone(), &status),
            Action::Start(_)
        ));
        status.revision = "e".repeat(64);
        assert!(matches!(retry_action(active, &status), Action::Poll(_)));
    }
}
