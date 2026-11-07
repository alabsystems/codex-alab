// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Auth-manager activity follows actual task completion, never cached turn contexts.

use super::Submission;
use super::session::Session;
use super::turn_context::TurnContext;
use codex_alab_core::assignment::Activity;
use codex_alab_core::assignment::ActivityRef;
use codex_protocol::protocol::Op;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct DispatchGate {
    deferred: VecDeque<Submission>,
}

fn starts_source_activity(op: &Op) -> bool {
    matches!(
        op,
        Op::TurnInput { .. }
            | Op::RecoverTurn { .. }
            | Op::Compact
            | Op::RunUserShellCommand { .. }
            | Op::Review { .. }
            | Op::RealtimeConversationStart(_)
            | Op::RealtimeConversationAttach { .. }
    )
}

pub(crate) async fn parent(session: &Session) -> Option<Activity> {
    let domain = session.services.auth_manager.alab_assignment_domain();
    if let Some(activity) = domain.current() {
        return Some(activity);
    }
    if let Some(activity) = session
        .services
        .thread_extension_data
        .get::<ActivityRef>()
        .and_then(|activity| activity.upgrade())
        && activity.same_domain(&domain)
    {
        return Some(activity);
    }
    let active = session.active_turn.lock().await;
    active
        .as_ref()?
        .task
        .as_ref()?
        .turn_context
        .extension_data
        .get::<ActivityRef>()?
        .upgrade()
}

fn child_parent(session: &Session) -> Option<Activity> {
    let domain = session.services.auth_manager.alab_assignment_domain();
    session
        .services
        .thread_extension_data
        .get::<ActivityRef>()?
        .upgrade()
        .filter(|activity| activity.same_domain(&domain))
}

pub(crate) fn automatic_start(session: &Session) -> Option<Activity> {
    let domain = session.services.auth_manager.alab_assignment_domain();
    domain.admit_start(child_parent(session).as_ref()).ok()
}

impl DispatchGate {
    pub fn has_deferred(&self) -> bool {
        !self.deferred.is_empty()
    }
    pub async fn next_ready(&mut self, session: &Session) -> Submission {
        let domain = session.services.auth_manager.alab_assignment_domain();
        // A poisoned domain is permanently closed; teardown remains independently selected.
        if domain.wait_open().await.is_err() {
            std::future::pending::<()>().await;
        }
        match self.deferred.pop_front() {
            Some(submission) => submission,
            None => std::future::pending().await,
        }
    }
    pub async fn admit(
        &mut self,
        session: &Session,
        sub: Submission,
    ) -> Option<(Submission, Option<Activity>)> {
        if !starts_source_activity(&sub.op) {
            return Some((sub, None));
        }
        let domain = session.services.auth_manager.alab_assignment_domain();
        let parent = child_parent(session);
        match domain.admit_start(parent.as_ref()) {
            Ok(activity) => Some((sub, Some(activity))),
            Err(_) => {
                // Retain the complete submission, reply channel and residency ownership.
                // Approval, interrupt and shutdown operations keep passing through this actor.
                self.deferred.push_back(sub);
                None
            }
        }
    }
}

pub(super) async fn scope<F: Future>(activity: Option<Activity>, future: F) -> F::Output {
    match activity {
        Some(activity) => activity.scope(future).await,
        None => future.await,
    }
}

pub(crate) async fn task_activity(
    session: &Arc<Session>,
    context: &TurnContext,
) -> Option<Activity> {
    let domain = session.services.auth_manager.alab_assignment_domain();
    let parent = domain
        .current()
        .filter(Activity::is_start)
        .or_else(|| {
            context
                .extension_data
                .get::<ActivityRef>()
                .and_then(|activity| activity.upgrade())
        })
        .or_else(|| child_parent(session));
    let activity = match domain.admit_work(parent.as_ref()) {
        Ok(activity) => activity,
        Err(_) => {
            session
                .send_event(
                    context,
                    codex_protocol::protocol::EventMsg::Error(
                        codex_protocol::protocol::ErrorEvent {
                            misalignment: None,
                            message: "Account assignment is pending; this task has not started."
                                .into(),
                            codex_error_info: Some(codex_protocol::protocol::CodexErrorInfo::Other),
                        },
                    ),
                )
                .await;
            return None;
        }
    };
    context.extension_data.insert(activity.downgrade());
    Some(activity)
}

/// Thread creation may wait for a new root, outside the session approval dispatcher.
pub(crate) async fn startup(
    manager: &codex_login::AuthManager,
    parent: Option<&Activity>,
) -> Result<Activity, codex_protocol::error::CodexErr> {
    let domain = manager.alab_assignment_domain();
    loop {
        match domain.admit_start(parent) {
            Ok(activity) => return Ok(activity),
            Err(codex_alab_core::assignment::Refused::Pending) if parent.is_none() => {
                domain.wait_open().await.map_err(|_| {
                    codex_protocol::error::CodexErr::InvalidRequest(
                        "Account assignment is unavailable.".into(),
                    )
                })?;
            }
            Err(_) => {
                return Err(codex_protocol::error::CodexErr::InvalidRequest(
                    "Account assignment is unavailable.".into(),
                ));
            }
        }
    }
}

/// Old-source prewarming is admitted without waiting and cancelled before resource drain.
pub(super) async fn prewarm(session: &Arc<Session>) {
    let domain = session.services.auth_manager.alab_assignment_domain();
    let Ok(activity) = domain.admit_resource(None) else {
        return;
    };
    activity
        .scope(async {
            tokio::select! {
                biased;
                _ = domain.wait_retiring() => {},
                _ = session.refresh_mcp_if_dirty() => {},
            }
        })
        .await;
}

pub(super) fn background(session: &Session) -> Option<Activity> {
    session
        .services
        .auth_manager
        .alab_assignment_domain()
        .admit_current_resource()
        .ok()
}

pub(super) fn publication(session: &Session) -> Option<Activity> {
    session
        .services
        .auth_manager
        .alab_assignment_domain()
        .admit_publication()
        .ok()
}

type ActionFuture<'a> = std::pin::Pin<
    Box<dyn Future<Output = Result<(), codex_alab_core::assignment::Refused>> + Send + 'a>,
>;

type ActionResult = Result<(), codex_alab_core::assignment::Refused>;
type ActionReply = tokio::sync::oneshot::Sender<ActionResult>;
enum SessionAction {
    Retire {
        domain: Arc<codex_alab_core::assignment::Domain>,
        permit: Arc<codex_alab_core::assignment::RetirementPermit>,
        reply: ActionReply,
    },
    Destination {
        boundary: Arc<codex_alab_core::assignment::BoundaryPermit>,
        reply: ActionReply,
    },
    Resume {
        reply: ActionReply,
    },
}

/// Global participants contain channels and exact permits, never Session's recursive graph.
trait SessionActions: Send + Sync {
    fn retire<'a>(
        &'a self,
        domain: &'a Arc<codex_alab_core::assignment::Domain>,
        retirement: &'a Arc<codex_alab_core::assignment::RetirementPermit>,
    ) -> ActionFuture<'a>;
    fn prepare_destination<'a>(
        &'a self,
        boundary: &'a Arc<codex_alab_core::assignment::BoundaryPermit>,
    ) -> ActionFuture<'a>;
    fn resume(&self) -> ActionFuture<'_>;
}

struct ActionSender(tokio::sync::mpsc::Sender<SessionAction>);
struct ActionMailbox(std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<SessionAction>>>);
impl ActionSender {
    async fn send(
        &self,
        action: SessionAction,
        reply: tokio::sync::oneshot::Receiver<ActionResult>,
    ) -> ActionResult {
        use codex_alab_core::assignment::Refused;
        self.0
            .send(action)
            .await
            .map_err(|_| Refused::Unavailable)?;
        reply.await.map_err(|_| Refused::Unavailable)?
    }
}
impl SessionActions for ActionSender {
    fn retire<'a>(
        &'a self,
        domain: &'a Arc<codex_alab_core::assignment::Domain>,
        retirement: &'a Arc<codex_alab_core::assignment::RetirementPermit>,
    ) -> ActionFuture<'a> {
        Box::pin(async move {
            let (reply, result) = tokio::sync::oneshot::channel();
            self.send(
                SessionAction::Retire {
                    domain: Arc::clone(domain),
                    permit: Arc::clone(retirement),
                    reply,
                },
                result,
            )
            .await
        })
    }
    fn prepare_destination<'a>(
        &'a self,
        boundary: &'a Arc<codex_alab_core::assignment::BoundaryPermit>,
    ) -> ActionFuture<'a> {
        Box::pin(async move {
            let (reply, result) = tokio::sync::oneshot::channel();
            self.send(
                SessionAction::Destination {
                    boundary: Arc::clone(boundary),
                    reply,
                },
                result,
            )
            .await
        })
    }
    fn resume(&self) -> ActionFuture<'_> {
        Box::pin(async move {
            let (reply, result) = tokio::sync::oneshot::channel();
            self.send(SessionAction::Resume { reply }, result).await
        })
    }
}

/// Pinned once by submission_loop and polled beside normal submissions. Cancelling a select
/// arm must never discard an accepted action, so callers must retain this future across turns.
pub(super) async fn serve_actions(session: &Arc<Session>) {
    let mailbox = session
        .services
        .session_extension_data
        .get::<Arc<ActionMailbox>>();
    let receiver = mailbox.and_then(|mailbox| mailbox.0.lock().ok()?.take());
    let Some(mut receiver) = receiver else {
        return std::future::pending().await;
    };
    while let Some(action) = receiver.recv().await {
        match action {
            SessionAction::Retire {
                domain,
                permit,
                reply,
            } => {
                let result = Box::pin(retire_session(session, &domain, &permit)).await;
                drop(permit);
                drop(domain);
                let _ = reply.send(result);
            }
            SessionAction::Destination { boundary, reply } => {
                let result = Box::pin(prepare_session(session, &boundary)).await;
                drop(boundary);
                let _ = reply.send(result);
            }
            SessionAction::Resume { reply } => {
                Box::pin(session.maybe_start_turn_for_pending_work()).await;
                let _ = reply.send(Ok(()));
            }
        }
    }
    std::future::pending().await
}

async fn retire_session(
    session: &Session,
    domain: &Arc<codex_alab_core::assignment::Domain>,
    retirement: &codex_alab_core::assignment::RetirementPermit,
) -> ActionResult {
    use codex_alab_core::assignment::Refused;
    if !Arc::ptr_eq(
        domain,
        &session.services.auth_manager.alab_assignment_domain(),
    ) || !retirement.protects(domain)
    {
        return Err(Refused::WrongDomain);
    }
    if !matches!(
        session.services.mcp_runtime.alab_enterprise_state(),
        codex_mcp::EnterpriseState::Absent
    ) {
        return Err(Refused::Unavailable);
    }
    session
        .conversation
        .shutdown()
        .await
        .map_err(|_| Refused::Unavailable)?;
    let _refresh = session
        .mcp_refresh
        .acquire()
        .await
        .map_err(|_| Refused::Unavailable)?;
    session
        .services
        .mcp_runtime
        .alab_retire_account(domain, retirement)
        .await
        .map_err(|_| Refused::Unavailable)?;
    session
        .services
        .model_client
        .alab_retire_account_transport();
    Ok(())
}
async fn prepare_session(
    session: &Arc<Session>,
    boundary: &codex_alab_core::assignment::BoundaryPermit,
) -> ActionResult {
    use codex_alab_core::assignment::Refused;
    let binding = boundary.bound()?;
    boundary
        .destination(&binding, async {
            let manager = &session.services.auth_manager;
            if !boundary.protects(
                &manager.alab_assignment_domain(),
                boundary.operation(),
                boundary.native_intent(),
            ) {
                return Err(Refused::WrongDomain);
            }
            if session
                .services
                .models_manager
                .auth_manager()
                .is_some_and(|catalog_manager| !std::ptr::eq(catalog_manager, manager.as_ref()))
            {
                return Err(Refused::WrongDomain);
            }
            let state = *manager.auth_change_state_receiver().borrow();
            if state.generation != binding.generations.auth
                || state.owner_generation != binding.generations.owner
                || state.source_generation != binding.generations.source
                || state.login_generation != binding.generations.login
            {
                return Err(Refused::Busy);
            }
            session.mark_mcp_runtime_dirty();
            session.refresh_mcp_if_dirty().await;
            if !session
                .services
                .mcp_runtime
                .current_auth_matches(manager.auth_cached().as_ref())
            {
                return Err(Refused::Unavailable);
            }
            session
                .services
                .mcp_runtime
                .latest_hard_refresh_codex_apps_tools_cache()
                .await
                .map_err(|_| Refused::Unavailable)?;
            // Existing model identity admission exposes destination or safe static/empty models.
            let _ = session.services.models_manager.get_remote_models().await;
            if *manager.auth_change_state_receiver().borrow() != state {
                return Err(Refused::Busy);
            }
            Ok(())
        })
        .await?
}

pub(crate) struct Cohort {
    domain: Arc<codex_alab_core::assignment::Domain>,
    manager: Arc<codex_login::AuthManager>,
    coordinator: coordinator::State,
    participants: std::sync::Mutex<Vec<std::sync::Weak<dyn SessionActions>>>,
}
static COHORTS: std::sync::OnceLock<std::sync::Mutex<Vec<std::sync::Weak<Cohort>>>> =
    std::sync::OnceLock::new();

pub(super) fn register(session: &Arc<Session>) {
    let domain = session.services.auth_manager.alab_assignment_domain();
    let mut cohorts = COHORTS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cohorts.retain(|cohort| cohort.strong_count() != 0);
    let cohort = cohorts
        .iter()
        .filter_map(std::sync::Weak::upgrade)
        .find(|cohort| Arc::ptr_eq(&cohort.domain, &domain))
        .unwrap_or_else(|| {
            let cohort = Arc::new(Cohort {
                domain,
                manager: Arc::clone(&session.services.auth_manager),
                coordinator: Default::default(),
                participants: Default::default(),
            });
            cohorts.push(Arc::downgrade(&cohort));
            cohort
        });
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    let participant: Arc<dyn SessionActions> = Arc::new(ActionSender(sender));
    session
        .services
        .session_extension_data
        .insert(Arc::new(ActionMailbox(std::sync::Mutex::new(Some(
            receiver,
        )))));
    cohort
        .participants
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(Arc::downgrade(&participant));
    session.services.session_extension_data.insert(participant);
    let consumer = coordinator::register(session.thread_id().to_string(), Arc::clone(&cohort));
    session.services.session_extension_data.insert(consumer);
    session.services.session_extension_data.insert(cohort);
}

impl Cohort {
    fn participants(
        &self,
    ) -> Result<Vec<Arc<dyn SessionActions>>, codex_alab_core::assignment::Refused> {
        let mut participants = self
            .participants
            .lock()
            .map_err(|_| codex_alab_core::assignment::Refused::Unavailable)?;
        participants.retain(|participant| participant.strong_count() != 0);
        Ok(participants
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .collect())
    }
    async fn retire(
        &self,
        retirement: &Arc<codex_alab_core::assignment::RetirementPermit>,
    ) -> Result<(), codex_alab_core::assignment::Refused> {
        if !retirement.protects(&self.domain) {
            return Err(codex_alab_core::assignment::Refused::WrongDomain);
        }
        for participant in self.participants()? {
            participant.retire(&self.domain, retirement).await?;
        }
        Ok(())
    }
    async fn prepare_destination(
        &self,
        boundary: &Arc<codex_alab_core::assignment::BoundaryPermit>,
    ) -> Result<(), codex_alab_core::assignment::Refused> {
        let binding = boundary.bound()?;
        for participant in self.participants()? {
            participant.prepare_destination(boundary).await?;
        }
        boundary.record_resources_ready(&binding)
    }
    async fn resume(&self) -> Result<(), codex_alab_core::assignment::Refused> {
        for participant in self.participants()? {
            participant.resume().await?;
        }
        Ok(())
    }
}

#[path = "assignment_coordinator.rs"]
mod coordinator;

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_core::assignment::Intent;
    use codex_alab_core::assignment::Selection;

    fn intent(session: &Session) -> Intent {
        Intent {
            operation: "a".repeat(32),
            thread: session.thread_id().to_string(),
            selection: Selection::Manual {
                account: "b".repeat(64),
                revision: "c".repeat(64),
            },
        }
    }
    fn submission(id: &str, op: Op) -> Submission {
        Submission {
            id: id.into(),
            op,
            turn_extension_init: None,
            trace: None,
            parent_turn_id: None,
            root_turn_id: None,
            residency_guard: None,
            message_delivery: None,
        }
    }

    #[tokio::test]
    async fn assignment_cohort_drains_every_session_sharing_the_actual_manager() {
        let (first, first_context) = super::super::tests::make_session_and_context().await;
        let (mut second, second_context) = super::super::tests::make_session_and_context().await;
        second.services.auth_manager = Arc::clone(&first.services.auth_manager);
        let first = Arc::new(first);
        let second = Arc::new(second);
        register(&first);
        register(&second);
        let cohort = first
            .services
            .session_extension_data
            .get::<Arc<Cohort>>()
            .unwrap();
        let second_cohort = second
            .services
            .session_extension_data
            .get::<Arc<Cohort>>()
            .unwrap();
        assert!(Arc::ptr_eq(cohort.as_ref(), second_cohort.as_ref()));
        assert_eq!(cohort.participants().unwrap().len(), 2);
        let domain = first.services.auth_manager.alab_assignment_domain();
        let first_work = task_activity(&first, &first_context).await.unwrap();
        let second_work = task_activity(&second, &second_context).await.unwrap();
        domain.request(intent(&first)).unwrap();
        assert!(domain.try_retire(&"a".repeat(32)).is_err());
        drop(first_work);
        assert!(domain.try_retire(&"a".repeat(32)).is_err());
        drop(second_work);
        let retirement = Arc::new(domain.try_retire(&"a".repeat(32)).unwrap());
        tokio::select! {
            result = cohort.retire(&retirement) => result.unwrap(),
            _ = serve_actions(&first) => panic!("first actor must remain live"),
            _ = serve_actions(&second) => panic!("second actor must remain live"),
        }
        let retirement = Arc::try_unwrap(retirement).ok().unwrap();
        assert!(retirement.freeze().is_ok());
    }

    #[tokio::test]
    async fn assignment_mailbox_retirement_stays_live_while_interrupt_is_serviced() {
        let (session, _) = super::super::tests::make_session_and_context().await;
        let session = Arc::new(session);
        register(&session);
        let cohort = session
            .services
            .session_extension_data
            .get::<Arc<Cohort>>()
            .unwrap();
        let domain = session.services.auth_manager.alab_assignment_domain();
        let refresh = session.mcp_refresh.acquire().await.unwrap();
        domain.request(intent(&session)).unwrap();
        let retirement = Arc::new(domain.try_retire(&"a".repeat(32)).unwrap());
        {
            let actions = serve_actions(&session);
            let retiring = cohort.retire(&retirement);
            tokio::pin!(actions, retiring);
            tokio::select! {
                biased;
                _ = &mut actions => panic!("action service must remain live"),
                _ = &mut retiring => panic!("retirement must wait for actual refresh ownership"),
                _ = tokio::task::yield_now() => {},
            }
            let mut dispatch = DispatchGate::default();
            let (interrupt, _) = dispatch
                .admit(&session, submission("interrupt", Op::Interrupt))
                .await
                .unwrap();
            assert_eq!(interrupt.id, "interrupt");
            assert!(
                dispatch
                    .admit(&session, submission("queued", Op::Compact))
                    .await
                    .is_none()
            );
            drop(refresh);
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                tokio::select! {
                    result = &mut retiring => result.unwrap(),
                    _ = &mut actions => panic!("action service must remain live"),
                }
            })
            .await
            .unwrap();
        }
        // Actor ACK dropped every borrowed permit before the driver can advance the boundary.
        assert!(Arc::try_unwrap(retirement).ok().unwrap().freeze().is_ok());
    }

    #[tokio::test]
    async fn assignment_dispatch_defers_original_input_but_services_interrupt() {
        let (session, _) = super::super::tests::make_session_and_context().await;
        let domain = session.services.auth_manager.alab_assignment_domain();
        domain.request(intent(&session)).unwrap();
        let mut gate = DispatchGate::default();
        assert!(
            gate.admit(&session, submission("original", Op::Compact))
                .await
                .is_none()
        );
        let (interrupt, lease) = gate
            .admit(&session, submission("cancel", Op::Interrupt))
            .await
            .unwrap();
        assert_eq!(interrupt.id, "cancel");
        assert!(lease.is_none());
        assert!(gate.has_deferred());
        domain.cancel_pending(&"a".repeat(32)).unwrap();
        let original = gate.next_ready(&session).await;
        assert_eq!(original.id, "original");
        assert!(matches!(original.op, Op::Compact));
        assert!(!gate.has_deferred());
        let (_, activity) = gate.admit(&session, original).await.unwrap();
        assert!(activity.is_some());
    }

    #[tokio::test]
    async fn assignment_live_task_admits_nested_work_without_cached_context_pin() {
        let (session, context) = super::super::tests::make_session_and_context().await;
        let session = Arc::new(session);
        let domain = session.services.auth_manager.alab_assignment_domain();
        let work = task_activity(&session, &context).await.unwrap();
        domain.request(intent(&session)).unwrap();
        let mut gate = DispatchGate::default();
        assert!(
            gate.admit(&session, submission("new-user-input", Op::Compact))
                .await
                .is_none()
        );
        assert!(
            automatic_start(&session).is_none(),
            "pending mailbox input is retained before extraction"
        );
        assert!(domain.try_retire(&"a".repeat(32)).is_err());
        let source = work.downgrade();
        work.scope(async {
            session.services.thread_extension_data.insert(source);
            let child = task_activity(&session, &context).await.unwrap();
            assert!(domain.try_retire(&"a".repeat(32)).is_err());
            drop(child);
            assert!(domain.try_retire(&"a".repeat(32)).is_err());
        })
        .await;
        assert!(
            context
                .extension_data
                .get::<ActivityRef>()
                .unwrap()
                .upgrade()
                .is_none()
        );
        assert!(domain.try_retire(&"a".repeat(32)).is_ok());
    }
}
