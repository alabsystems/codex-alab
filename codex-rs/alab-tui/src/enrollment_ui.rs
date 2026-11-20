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
#[cfg(unix)]
use codex_alab_aterm::enrollment::Target;

#[derive(Default)]
pub struct View {
    pub busy: bool,
    #[cfg(unix)]
    selected: Option<Active>,
    #[cfg(unix)]
    pub conflicts: Conflicts,
}

#[cfg(unix)]
impl View {
    pub fn loading() -> Self {
        Self {
            busy: true,
            ..Self::default()
        }
    }

    pub fn select(&mut self, active: &Active) -> bool {
        if self
            .selected
            .as_ref()
            .is_some_and(|previous| !previous.same_binding(active))
        {
            return false;
        }
        self.selected.get_or_insert_with(|| active.clone());
        true
    }

    pub fn selected(&self) -> Option<&Active> {
        self.selected.as_ref()
    }

    pub fn rediscover(&mut self) {
        self.selected = None;
    }

    pub fn expects(&self, active: Option<&Active>) -> bool {
        match (self.selected.as_ref(), active) {
            (Some(selected), Some(active)) => selected.same_binding(active),
            (None, None) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
#[cfg(unix)]
pub struct Active {
    pub operation: String,
    pub account_id: String,
    pub revision: String,
    /// Presentation only, from the native target at the revision that admitted this operation.
    pub label: Option<String>,
}

#[cfg(unix)]
impl Active {
    pub fn same_binding(&self, other: &Self) -> bool {
        self.operation == other.operation
            && self.account_id == other.account_id
            && self.revision == other.revision
    }
}

/// A native cancellation names only the operation nonce. Never act on contradictory bindings.
#[derive(Clone, Default)]
#[cfg(unix)]
pub struct Conflicts(Vec<(Active, String)>);

#[cfg(unix)]
impl Conflicts {
    pub fn new(status: &Status, retained: &[Active]) -> Self {
        Self(
            retained
                .iter()
                .filter_map(|active| {
                    let record = status
                        .operations
                        .iter()
                        .find(|op| op.operation == active.operation)?;
                    (record.revision != active.revision || record.account_id != active.account_id)
                        .then(|| (active.clone(), record.account_id.clone()))
                })
                .collect(),
        )
    }

    pub fn affects(&self, account_id: &str) -> bool {
        self.0
            .iter()
            .any(|(original, reported)| original.account_id == account_id || reported == account_id)
    }

    pub fn original(&self, operation: &str) -> Option<&Active> {
        self.0
            .iter()
            .find(|(active, _)| active.operation == operation)
            .map(|(active, _)| active)
    }

    pub fn permits(&self, action: &Action) -> bool {
        match action {
            Action::OpenAccounts | Action::Discover { .. } => true,
            Action::Enroll { target, .. } => !self.affects(&target.account_id),
            Action::Start(active) => {
                !self.affects(&active.account_id) && self.original(&active.operation).is_none()
            }
            Action::Cancel(active) => self.original(&active.operation).is_none(),
            Action::Observe(operation, _) => self.original(&operation.operation).is_none(),
            Action::Poll(active) => self
                .original(&active.operation)
                .is_none_or(|old| old.same_binding(active)),
        }
    }
}

#[cfg(unix)]
impl From<Operation> for Active {
    fn from(operation: Operation) -> Self {
        Self {
            operation: operation.operation,
            account_id: operation.account_id,
            revision: operation.revision,
            label: None,
        }
    }
}

#[derive(Clone, Debug)]
#[cfg(unix)]
pub enum Action {
    OpenAccounts,
    Discover {
        label: Option<String>,
    },
    Enroll {
        target: Target,
        revision: String,
        session: Session,
    },
    Start(Active),
    Observe(Operation, Option<String>),
    Poll(Active),
    Cancel(Active),
}

#[cfg(unix)]
impl Action {
    pub fn request_operation(&self) -> Option<&Active> {
        match self {
            Self::Start(active) | Self::Poll(active) | Self::Cancel(active) => Some(active),
            _ => None,
        }
    }
}

#[derive(Debug)]
#[cfg(unix)]
pub enum Event {
    Action(Action),
    Status {
        status: Status,
        label: Option<String>,
        display_accounts: Vec<codex_alab_aterm::accounts::Account>,
    },
    Progress {
        active: Active,
        reply: Reply,
    },
    Failed(&'static str, Option<Active>),
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

/// A retained admission preserves its original display label across roster changes. An operation
/// discovered without that admission can only borrow a unique label from its own roster revision.
#[cfg(unix)]
pub fn operation_label<'a>(
    operation: &Operation,
    status: &'a Status,
    retained: &'a [Active],
) -> Option<&'a str> {
    if let Some(active) = retained
        .iter()
        .find(|active| active.operation == operation.operation)
    {
        if active.account_id != operation.account_id || active.revision != operation.revision {
            return None;
        }
        if let Some(label) = active.label.as_deref() {
            return Some(label);
        }
    }
    if status.revision != operation.revision {
        return None;
    }
    let target = status
        .targets
        .iter()
        .find(|target| target.account_id == operation.account_id)?;
    let ambiguous = status
        .targets
        .iter()
        .any(|other| other.account_id != target.account_id && other.label == target.label);
    (!ambiguous).then_some(target.label.as_str())
}

#[cfg(unix)]
pub fn retry_action(active: Active, status: &Status) -> Action {
    if let Some(operation) = status
        .operations
        .iter()
        .find(|op| op.operation == active.operation)
    {
        // Only an observation of the same immutable admission can resolve this intent.
        return if operation.account_id == active.account_id && operation.revision == active.revision
        {
            Action::Observe(operation.clone(), active.label)
        } else {
            Action::Poll(active)
        };
    }
    if status.capabilities.enroll
        && active.revision == status.revision
        && !status
            .operations
            .iter()
            .any(|op| op.account_id == active.account_id && op.phase.blocks_new_enrollment())
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

/// Both a named command and a picker selection resolve an account by the same native status.
#[cfg(unix)]
impl Conflicts {
    pub fn target_action(
        &self,
        target: &Target,
        status: &Status,
        retained: &[Active],
    ) -> Option<Action> {
        if self.affects(&target.account_id) {
            return None;
        }
        if let Some(active) = retained
            .iter()
            .find(|active| active.account_id == target.account_id)
        {
            return Some(retry_action(active.clone(), status));
        }
        if let Some(operation) = status
            .operations
            .iter()
            .rev()
            .find(|op| op.account_id == target.account_id && op.phase.blocks_new_enrollment())
        {
            return Some(Action::Observe(
                operation.clone(),
                operation_label(operation, status, retained).map(str::to_owned),
            ));
        }
        status.capabilities.enroll.then(|| Action::Enroll {
            target: target.clone(),
            revision: status.revision.clone(),
            session: status.session.clone(),
        })
    }
}

/// Record before the first send, and never discard an uncertain operation on view close.
#[cfg(unix)]
pub fn admit_intent(
    session: &Session,
    target: Target,
    revision: String,
) -> Result<Action, &'static str> {
    let mut intents = intents()
        .lock()
        .map_err(|_| "Enrollment requests are unavailable in this session.")?;
    let key = (
        session.sid.clone(),
        session.process_pid,
        session.process_birth.clone(),
        target.account_id.clone(),
    );
    if let Some(active) = intents.get(&key) {
        return Ok(Action::Poll(active.clone()));
    }
    if intents.len() >= 256 {
        return Err("Too many pending enrollment requests. Check their status first.");
    }
    let active = Active {
        operation: codex_alab_aterm::enrollment::new_operation_id(),
        account_id: target.account_id,
        revision,
        label: Some(target.label),
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

    fn target(id: &str) -> Target {
        Target {
            account_id: id.repeat(64),
            label: "Work".into(),
        }
    }

    #[test]
    fn uncertain_intent_survives_view_drop_but_never_transfers_to_a_new_process_birth() {
        let mut session = Session {
            sid: "s-intent-fixture".into(),
            process_pid: std::process::id(),
            process_birth: "first birth".into(),
            epoch: "first host".into(),
        };
        let Action::Start(original) = admit_intent(&session, target("c"), "b".repeat(64)).unwrap()
        else {
            panic!("new intent");
        };
        let pane = std::sync::Arc::new(std::sync::Mutex::new(View::default()));
        drop(pane);
        session.epoch = "replacement host".into();
        let Action::Poll(retry) = admit_intent(&session, target("c"), "f".repeat(64)).unwrap()
        else {
            panic!("must retain original intent");
        };
        assert_eq!(retry.operation, original.operation);
        assert_eq!(retry.revision, original.revision);
        assert_eq!(retained(&session).len(), 1);
        session.process_birth = "new process birth".into();
        assert!(retained(&session).is_empty());
        let Action::Start(new) = admit_intent(&session, target("c"), "b".repeat(64)).unwrap()
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
        let Action::Start(first) = admit_intent(&session, target("a"), "b".repeat(64)).unwrap()
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
        let Action::Start(second) = admit_intent(&session, target("a"), "c".repeat(64)).unwrap()
        else {
            panic!("new explicit intent");
        };
        reconcile(&session, &[record]);
        assert_eq!(retained(&session)[0].operation, second.operation);
        assert_ne!(first.operation, second.operation);
    }

    #[test]
    fn operation_label_requires_the_original_admission_or_its_roster_revision() {
        let work = target("c");
        let mut status = Status {
            schema: 1,
            request: "d".repeat(32),
            session: Session {
                sid: "s-label".into(),
                process_pid: std::process::id(),
                process_birth: "birth".into(),
                epoch: "host".into(),
            },
            revision: "b".repeat(64),
            capabilities: codex_alab_aterm::enrollment::Capabilities {
                enroll: true,
                create: false,
            },
            targets: vec![work.clone()],
            operations: vec![],
        };
        let operation = Operation {
            operation: "a".repeat(32),
            account_id: work.account_id.clone(),
            revision: status.revision.clone(),
            phase: codex_alab_aterm::enrollment::Phase::Running,
            generation: None,
        };
        assert_eq!(operation_label(&operation, &status, &[]), Some("Work"));
        let mut other = operation.clone();
        other.account_id = "f".repeat(64);
        assert_eq!(operation_label(&other, &status, &[]), None);
        let saved = Active {
            label: Some("Work".into()),
            ..operation.clone().into()
        };
        status.targets.push(Target {
            account_id: other.account_id.clone(),
            label: work.label,
        });
        assert_eq!(operation_label(&operation, &status, &[]), None);
        assert_eq!(
            operation_label(&operation, &status, std::slice::from_ref(&saved)),
            Some("Work")
        );
        status.targets.pop();
        status.targets[0].label = "New slot".into();
        status.revision = "e".repeat(64);
        assert_eq!(operation_label(&operation, &status, &[]), None);
        assert_eq!(
            operation_label(&operation, &status, std::slice::from_ref(&saved)),
            Some("Work")
        );
        assert_eq!(operation_label(&other, &status, &[saved]), None);
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
            label: Some("Work".into()),
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
        assert!(matches!(
            retry_action(active.clone(), &status),
            Action::Poll(_)
        ));
        status.revision.clone_from(&active.revision);
        status.capabilities.enroll = false;
        assert!(matches!(
            retry_action(active.clone(), &status),
            Action::Poll(_)
        ));
        status.capabilities.enroll = true;
        let retained = std::slice::from_ref(&active);
        let Some(Action::Start(retry)) =
            Conflicts::new(&status, retained).target_action(&status.targets[0], &status, retained)
        else {
            panic!("explicit repeat must resubmit the missing intent");
        };
        assert_eq!(retry.operation, active.operation);
        assert_eq!(retry.revision, active.revision);
        status.operations.push(Operation {
            operation: "f".repeat(32),
            account_id: active.account_id.clone(),
            revision: active.revision.clone(),
            phase: codex_alab_aterm::enrollment::Phase::Running,
            generation: None,
        });
        assert!(matches!(
            Conflicts::new(&status, retained).target_action(&status.targets[0], &status, retained),
            Some(Action::Poll(_))
        ));
        status.operations[0].operation.clone_from(&active.operation);
        assert!(matches!(
            Conflicts::new(&status, retained).target_action(&status.targets[0], &status, retained),
            Some(Action::Observe(_, _))
        ));
    }
}
