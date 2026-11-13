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

/// Long-lived factories retain one actual epoch, never the changing host facade.
pub(crate) fn capture_factory_manager(
    manager: &Arc<codex_login::AuthManager>,
) -> std::io::Result<Arc<codex_login::AuthManager>> {
    manager.alab_capture_current_manager()
}

/// Isolated startup moves only instruction snapshots. Keep the discarded
/// provider owners until the original startup boundary has completed.
pub(crate) fn startup_discarded_instruction_providers(
    instructions: &crate::agents_md_manager::SessionInstructions,
    isolation: codex_extension_api::SessionIsolation,
) -> crate::agents_md_manager::SessionInstructions {
    if isolation == codex_extension_api::SessionIsolation::Isolated {
        crate::agents_md_manager::SessionInstructions {
            user_provider: instructions.user_provider.clone(),
            thread_provider: instructions.thread_provider.clone(),
            ..Default::default()
        }
    } else {
        Default::default()
    }
}

#[path = "assignment_startup_poll.rs"]
mod startup_poll;
pub(crate) use startup_poll::boxed_startup;
pub(crate) use startup_poll::phased_startup;

#[path = "assignment_configuration_jobs.rs"]
mod configuration_jobs;
#[path = "assignment_host_watch.rs"]
mod host_watch;

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
        let manager = &session.services.auth_manager;
        loop {
            let domain = manager.alab_assignment_domain();
            let changed = manager.alab_assignment_domain_changed(&domain);
            tokio::pin!(changed);
            tokio::select! {
                biased;
                replacement = &mut changed => {
                    if replacement.is_err() {
                        std::future::pending::<()>().await;
                    }
                    continue;
                },
                opened = domain.wait_open() => {
                    if !Arc::ptr_eq(&domain, &manager.alab_assignment_domain()) {
                        continue;
                    }
                    if opened.is_ok() {
                        break;
                    }
                    // A retired domain never reopens. Keep the original queued
                    // submission until actual epoch publication replaces it.
                    // Teardown remains independently selected by the dispatcher.
                    if changed.await.is_err() {
                        std::future::pending::<()>().await;
                    }
                }
            }
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

/// Erase dispatch construction before either activity scope or its caller polls.
/// The factory constructs only an inert future; execution stays on this task.
#[inline(never)]
pub(super) fn scope<'a, F>(
    activity: Option<Activity>,
    make: impl FnOnce() -> F,
) -> futures::future::BoxFuture<'a, F::Output>
where
    F: Future + Send + 'a,
{
    let future = boxed_startup(make);
    boxed_startup(move || async move {
        match activity {
            Some(activity) => activity.scope(future).await,
            None => future.await,
        }
    })
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
                            rejected_audio_file_ids: Vec::new(),
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
    let unavailable = || {
        codex_protocol::error::CodexErr::InvalidRequest("Account assignment is unavailable.".into())
    };
    loop {
        let domain = manager.alab_assignment_domain();
        match domain.admit_start(parent) {
            Ok(activity) => return Ok(activity),
            Err(codex_alab_core::assignment::Refused::Pending) if parent.is_none() => {
                // A root can follow actual publication. A child retains its
                // original parent and must never migrate to another account.
                let changed = manager.alab_assignment_domain_changed(&domain);
                tokio::pin!(changed);
                tokio::select! {
                    biased;
                    replacement = &mut changed => replacement.map_err(|_| unavailable())?,
                    opened = domain.wait_open() => {
                        if Arc::ptr_eq(&domain, &manager.alab_assignment_domain())
                            && opened.is_err()
                        {
                            // Retired A cannot reopen; only actual publication
                            // can provide a new root, independently of counters.
                            changed.await.map_err(|_| unavailable())?;
                        }
                    }
                }
            }
            Err(_) => return Err(unavailable()),
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
    ConfigurationRegistration {
        reply: tokio::sync::oneshot::Sender<
            std::io::Result<crate::alab_destination_config::Registration>,
        >,
    },
    BeginConfiguration {
        registration: crate::alab_destination_config::Registration,
        reply: tokio::sync::oneshot::Sender<
            std::io::Result<Arc<crate::alab_destination_config::StagedPlan>>,
        >,
    },
    PrepareConfiguration {
        plan: Arc<crate::alab_destination_config::StagedPlan>,
        context: codex_login::auth::AlabDestinationResourceContext,
        reply: tokio::sync::oneshot::Sender<
            std::io::Result<crate::alab_destination_config::PreparedConfig>,
        >,
    },
    Retire {
        domain: Arc<codex_alab_core::assignment::Domain>,
        permit: Arc<dyn codex_alab_core::assignment::RetirementProof>,
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
        retirement: &'a Arc<dyn codex_alab_core::assignment::RetirementProof>,
    ) -> ActionFuture<'a>;
    fn prepare_destination<'a>(
        &'a self,
        boundary: &'a Arc<codex_alab_core::assignment::BoundaryPermit>,
    ) -> ActionFuture<'a>;
    fn resume(&self) -> ActionFuture<'_>;
}

struct ActionSender {
    sender: tokio::sync::mpsc::Sender<SessionAction>,
    stopped: std::sync::atomic::AtomicBool,
    stopped_changed: tokio::sync::Notify,
}
struct RuntimeParticipant(Arc<ActionSender>);
impl codex_alab_core::manager_catalog::Participant for ActionSender {
    fn retire<'a>(
        &'a self,
        domain: &'a Arc<codex_alab_core::assignment::Domain>,
        proof: Arc<dyn codex_alab_core::assignment::RetirementProof>,
    ) -> codex_alab_core::manager_catalog::ResourceFuture<'a> {
        Box::pin(async move {
            if !proof.protects(domain) {
                return Err(codex_alab_core::assignment::Refused::Pending);
            }
            let stopped = self.stopped_changed.notified();
            tokio::pin!(stopped);
            stopped.as_mut().enable();
            if self.stopped.load(std::sync::atomic::Ordering::Acquire) {
                return Ok(());
            }
            let (reply, result) = tokio::sync::oneshot::channel();
            let request = self.send(
                SessionAction::Retire {
                    domain: Arc::clone(domain),
                    permit: proof,
                    reply,
                },
                result,
            );
            tokio::select! {
                result = request => {
                    if self.stopped.load(std::sync::atomic::Ordering::Acquire) { Ok(()) } else { result }
                },
                _ = stopped => {
                    if self.stopped.load(std::sync::atomic::Ordering::Acquire) { Ok(()) } else { Err(codex_alab_core::assignment::Refused::Unavailable) }
                }
            }
        })
    }
}
/// Close admission before producer teardown. This is not a joined/stopped proof.
pub(super) fn runtime_stopping(session: &Session) {
    session
        .services
        .session_extension_data
        .get_or_init(configuration_jobs::Jobs::default)
        .close();
}
/// Called only after the real shutdown path joined producers, realtime and MCP.
pub(super) async fn runtime_stopped(session: &Session) {
    if session
        .services
        .session_extension_data
        .get_or_init(configuration_jobs::Jobs::default)
        .shutdown()
        .await
        .is_err()
    {
        return; // Unknown drain cannot become a Stopped resource participant.
    }
    session
        .services
        .model_client
        .alab_retire_account_transport();
    if let Some(participant) = session
        .services
        .session_extension_data
        .get::<Arc<RuntimeParticipant>>()
    {
        participant
            .0
            .stopped
            .store(true, std::sync::atomic::Ordering::Release);
        participant.0.stopped_changed.notify_waiters();
    }
}
struct ActionMailbox(std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<SessionAction>>>);
impl ActionSender {
    async fn configuration_request<T>(
        &self,
        action: SessionAction,
        reply: tokio::sync::oneshot::Receiver<std::io::Result<T>>,
    ) -> std::io::Result<T> {
        if self.stopped.load(std::sync::atomic::Ordering::Acquire) {
            return Err(std::io::Error::other(
                "session configuration participant stopped",
            ));
        }
        self.sender
            .send(action)
            .await
            .map_err(|_| std::io::Error::other("session configuration participant unavailable"))?;
        reply
            .await
            .map_err(|_| std::io::Error::other("session configuration participant unavailable"))?
    }
    async fn send(
        &self,
        action: SessionAction,
        reply: tokio::sync::oneshot::Receiver<ActionResult>,
    ) -> ActionResult {
        use codex_alab_core::assignment::Refused;
        self.sender
            .send(action)
            .await
            .map_err(|_| Refused::Unavailable)?;
        reply.await.map_err(|_| Refused::Unavailable)?
    }
}
impl crate::alab_destination_config::SessionParticipant for ActionSender {
    fn registration(&self) -> crate::alab_destination_config::RegistrationFuture<'_> {
        Box::pin(async move {
            let (reply, receive) = tokio::sync::oneshot::channel();
            self.configuration_request(SessionAction::ConfigurationRegistration { reply }, receive)
                .await
        })
    }
    fn begin<'a>(
        &'a self,
        registration: crate::alab_destination_config::Registration,
    ) -> crate::alab_destination_config::PlanFuture<'a> {
        Box::pin(async move {
            let (reply, receive) = tokio::sync::oneshot::channel();
            self.configuration_request(
                SessionAction::BeginConfiguration {
                    registration,
                    reply,
                },
                receive,
            )
            .await
        })
    }
    fn prepare<'a>(
        &'a self,
        plan: Arc<crate::alab_destination_config::StagedPlan>,
        context: codex_login::auth::AlabDestinationResourceContext,
    ) -> crate::alab_destination_config::ConfigurationFuture<'a> {
        Box::pin(async move {
            let (reply, receive) = tokio::sync::oneshot::channel();
            self.configuration_request(
                SessionAction::PrepareConfiguration {
                    plan,
                    context,
                    reply,
                },
                receive,
            )
            .await
        })
    }
}
impl SessionActions for ActionSender {
    fn retire<'a>(
        &'a self,
        domain: &'a Arc<codex_alab_core::assignment::Domain>,
        retirement: &'a Arc<dyn codex_alab_core::assignment::RetirementProof>,
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
            SessionAction::ConfigurationRegistration { reply } => {
                let registration = configuration_input(session)
                    .await
                    .map(|(registration, _)| registration);
                let _ = reply.send(registration);
            }
            SessionAction::BeginConfiguration {
                registration,
                reply,
            } => {
                match configuration_input(session).await {
                    Ok((actual, snapshot)) if actual.same_occurrence(&registration) => {
                        // Own the request beyond observer cancellation, without
                        // blocking submissions/approvals on config/backend I/O.
                        let jobs = session
                            .services
                            .session_extension_data
                            .get_or_init(configuration_jobs::Jobs::default);
                        let _ = jobs.spawn(async move {
                            let _ = reply.send(actual.begin(snapshot).await.map(Arc::new));
                        });
                    }
                    _ => {
                        let _ = reply.send(Err(std::io::Error::other(
                            "session configuration owner differs",
                        )));
                    }
                }
            }
            SessionAction::PrepareConfiguration {
                plan,
                context,
                reply,
            } => match configuration_input(session).await {
                Ok((actual, snapshot)) if actual.same_occurrence(plan.registration()) => {
                    let jobs = session
                        .services
                        .session_extension_data
                        .get_or_init(configuration_jobs::Jobs::default);
                    let _ = jobs.spawn(async move {
                        let _ = reply.send(plan.prepare(context, snapshot).await);
                    });
                }
                _ => {
                    let _ = reply.send(Err(std::io::Error::other(
                        "session configuration owner differs",
                    )));
                }
            },
            SessionAction::Retire {
                domain,
                permit,
                reply,
            } => {
                let result = Box::pin(retire_session(session, &domain, permit.as_ref())).await;
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
async fn configuration_input(
    session: &Session,
) -> std::io::Result<(
    crate::alab_destination_config::Registration,
    crate::ThreadConfigSnapshot,
)> {
    if session
        .services
        .session_extension_data
        .get_or_init(configuration_jobs::Jobs::default)
        .closed()
    {
        return Err(std::io::Error::other("session configuration work closed"));
    }
    let manager = capture_factory_manager(&session.services.auth_manager)?;
    let registration = crate::alab_destination_config::Registration::from_store(
        &session.services.session_extension_data,
        &manager,
    )?;
    let snapshot = {
        let state = session.state.lock().await;
        let configuration = &state.session_configuration;
        configuration.thread_config_snapshot(configuration.environments.clone())
    };
    registration.check_source(&capture_factory_manager(&session.services.auth_manager)?)?;
    Ok((registration, snapshot))
}

async fn retire_session(
    session: &Session,
    domain: &Arc<codex_alab_core::assignment::Domain>,
    retirement: &dyn codex_alab_core::assignment::RetirementProof,
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
            let manager = capture_factory_manager(&session.services.auth_manager)
                .map_err(|_| Refused::Unavailable)?;
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
            session.mark_mcp_runtime_dirty(
                crate::thread_manager::McpRefreshReason::RuntimeAuthChanged,
            );
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
    host_watch::register(session);
    session
        .services
        .session_extension_data
        .get_or_init(configuration_jobs::Jobs::default);
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
    let sender = Arc::new(ActionSender {
        sender,
        stopped: std::sync::atomic::AtomicBool::new(false),
        stopped_changed: tokio::sync::Notify::new(),
    });
    crate::alab_destination_config::ParticipantHandle::install(
        &session.services.session_extension_data,
        sender.clone(),
    );
    if let Some(record) = session
        .services
        .auth_manager
        .alab_assignment_domain()
        .catalog()
    {
        let _ = record.participant(sender.clone());
    }
    session
        .services
        .session_extension_data
        .insert(Arc::new(RuntimeParticipant(sender.clone())));
    let participant: Arc<dyn SessionActions> = sender;
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
        let proof: Arc<dyn codex_alab_core::assignment::RetirementProof> = retirement.clone();
        for participant in self.participants()? {
            participant.retire(&self.domain, &proof).await?;
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

/// Ordinary turn suspension observes only its actual use family; it is not a
/// process/account migration and cannot close unrelated managers or factories.
pub(super) fn suspension_cleanup(
    context: &TurnContext,
) -> Option<codex_alab_core::assignment::UseDrain> {
    context
        .extension_data
        .get::<ActivityRef>()
        .map(|activity| activity.native_cleanup())
}
pub(super) async fn finish_suspension_cleanup(
    cleanup: Option<codex_alab_core::assignment::UseDrain>,
) -> Result<(), codex_protocol::error::CodexErr> {
    if let Some(cleanup) = cleanup {
        cleanup.settled().await.map_err(|_| {
            codex_protocol::error::CodexErr::InvalidRequest(
                "Turn credential cleanup has not settled.".into(),
            )
        })?;
    }
    Ok(())
}

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
            alab_upgrade: Vec::new(), // ALAB(upgrade): the native sender supplies admission.
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
    async fn root_startup_waits_for_publication_after_actual_retirement() {
        let (session, _) = super::super::tests::make_session_and_context().await;
        let manager = &session.services.auth_manager;
        let domain = manager.alab_assignment_domain();
        domain.request(intent(&session)).unwrap();
        let _retirement = domain.try_retire(&"a".repeat(32)).unwrap();
        let pending = startup(manager, None);
        tokio::pin!(pending);
        assert!(
            std::future::poll_fn(|cx| {
                std::task::Poll::Ready(pending.as_mut().poll(cx).is_pending())
            })
            .await,
            "retired A must not fail root startup before actual publication"
        );
    }

    #[tokio::test]
    async fn child_startup_cannot_move_its_parent_to_another_domain() {
        let (first, _) = super::super::tests::make_session_and_context().await;
        let (second, _) = super::super::tests::make_session_and_context().await;
        let parent = first
            .services
            .auth_manager
            .alab_assignment_domain()
            .admit_start(None)
            .unwrap();
        assert!(
            startup(&second.services.auth_manager, Some(&parent))
                .await
                .is_err()
        );
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
    async fn deferred_original_input_reselects_actual_published_source_domain() {
        let (mut session, _) = super::super::tests::make_session_and_context().await;
        let home = tempfile::tempdir().unwrap();
        let manager = codex_login::AuthManager::from_auth_for_testing_with_home(
            codex_login::CodexAuth::from_api_key("fixture-domain-a"),
            home.path().to_path_buf(),
        );
        session.services.auth_manager = manager.clone();
        let original_domain = manager.alab_assignment_domain();
        let original_auth = manager.auth_cached().unwrap();
        let original_state = *manager.auth_change_state_receiver().borrow();
        original_domain.request(intent(&session)).unwrap();
        let mut gate = DispatchGate::default();
        assert!(
            gate.admit(
                &session,
                submission("original-after-publication", Op::Compact)
            )
            .await
            .is_none()
        );
        let publication = async {
            tokio::task::yield_now().await;
            codex_login::login_with_api_key(
                home.path(),
                "fixture-domain-b",
                codex_login::AuthCredentialsStoreMode::File,
                codex_login::AuthKeyringBackendKind::Direct,
            )
            .unwrap();
            // Upstream CodexAuth equality compares API-key auth by mode. A
            // same-kind reload returns false even when its real key/source and
            // refresh generations changed; it is not a publication receipt.
            assert!(!manager.reload().await);
            let published = manager.auth_cached().unwrap();
            assert_eq!(published.api_key(), Some("fixture-domain-b"));
            assert!(
                manager.auth_change_state_receiver().borrow().generation
                    > original_state.generation
            );
            assert!(
                manager.auth_change_state_for(&original_auth).is_none(),
                "a retained A snapshot must not be relabeled as published B"
            );
            assert!(!Arc::ptr_eq(
                &original_domain,
                &manager.alab_assignment_domain()
            ));
        };
        let (original, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(gate.next_ready(&session), publication)
        })
        .await
        .unwrap();
        assert_eq!(original.id, "original-after-publication");
        assert!(matches!(original.op, Op::Compact));
        assert!(!gate.has_deferred());
        assert_ne!(
            original_domain.phase(),
            codex_alab_core::assignment::Phase::Open,
            "retired A was not reopened to release the original input"
        );
        let (_, activity) = gate.admit(&session, original).await.unwrap();
        assert!(
            activity
                .unwrap()
                .same_domain(&manager.alab_assignment_domain())
        );
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
    struct ConfigurationBackend {
        source: Arc<codex_login::AuthManager>,
        calls: std::sync::atomic::AtomicUsize,
        started: tokio::sync::Notify,
        release: tokio::sync::Notify,
        finished: tokio::sync::Notify,
        active: std::sync::atomic::AtomicUsize,
    }
    impl crate::alab_destination_config::ConfigPreparer for ConfigurationBackend {
        fn source_manager(&self) -> &Arc<codex_login::AuthManager> {
            &self.source
        }
        fn begin<'a>(
            &'a self,
            _: crate::ThreadConfigSnapshot,
        ) -> crate::alab_destination_config::BeginFuture<'a> {
            Box::pin(async move {
                struct Active<'a>(&'a std::sync::atomic::AtomicUsize);
                impl Drop for Active<'_> {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                    }
                }
                self.active
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                let _active = Active(&self.active);
                self.calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                self.started.notify_one();
                self.release.notified().await;
                self.finished.notify_one();
                Err(std::io::Error::other(
                    "fixture has no destination configuration",
                ))
            })
        }
    }
    #[tokio::test]
    async fn configuration_mailbox_rejects_missing_and_foreign_registration_before_backend() {
        let (session, _) = super::super::tests::make_session_and_context().await;
        let session = Arc::new(session);
        register(&session);
        let participant = crate::alab_destination_config::ParticipantHandle::from_store(
            &session.services.session_extension_data,
        )
        .unwrap();
        let actions = serve_actions(&session);
        tokio::pin!(actions);
        tokio::select! {
            result = participant.participant().registration() => assert!(result.is_err()),
            _ = &mut actions => panic!("actor must remain live"),
        }
        let source = capture_factory_manager(&session.services.auth_manager).unwrap();
        let backend = Arc::new(ConfigurationBackend {
            source: source.clone(),
            calls: Default::default(),
            started: Default::default(),
            release: Default::default(),
            finished: Default::default(),
            active: Default::default(),
        });
        let registration =
            crate::alab_destination_config::Registration::new(backend.clone()).unwrap();
        registration
            .install(&session.services.session_extension_data)
            .unwrap();
        let foreign =
            crate::alab_destination_config::Registration::new(Arc::new(ConfigurationBackend {
                source,
                calls: Default::default(),
                started: Default::default(),
                release: Default::default(),
                finished: Default::default(),
                active: Default::default(),
            }))
            .unwrap();
        tokio::select! {
            result = participant.participant().begin(foreign) => assert!(result.is_err()),
            _ = &mut actions => panic!("actor must remain live"),
        }
        assert_eq!(backend.calls.load(std::sync::atomic::Ordering::Acquire), 0);
    }
    #[tokio::test]
    async fn configuration_backend_is_owned_without_blocking_mailbox_or_interrupt() {
        let (session, _) = super::super::tests::make_session_and_context().await;
        let session = Arc::new(session);
        register(&session);
        let backend = Arc::new(ConfigurationBackend {
            source: capture_factory_manager(&session.services.auth_manager).unwrap(),
            calls: Default::default(),
            started: Default::default(),
            release: Default::default(),
            finished: Default::default(),
            active: Default::default(),
        });
        let registration =
            crate::alab_destination_config::Registration::new(backend.clone()).unwrap();
        registration
            .install(&session.services.session_extension_data)
            .unwrap();
        let participant = crate::alab_destination_config::ParticipantHandle::from_store(
            &session.services.session_extension_data,
        )
        .unwrap();
        let actions = serve_actions(&session);
        tokio::pin!(actions);
        {
            let request = participant.participant().begin(registration.clone());
            tokio::pin!(request);
            tokio::select! {
                _ = backend.started.notified() => {},
                _ = &mut request => panic!("backend must wait on actual owner"),
                _ = &mut actions => panic!("actor must remain live"),
            }
            tokio::select! {
                result = participant.participant().registration() => assert!(result.unwrap().same_occurrence(&registration)),
                _ = &mut actions => panic!("actor must remain live"),
            }
            let mut dispatch = DispatchGate::default();
            assert!(
                dispatch
                    .admit(&session, submission("interrupt", Op::Interrupt))
                    .await
                    .is_some()
            );
        } // Cancelling the observer cannot cancel the already accepted backend.
        assert_eq!(backend.active.load(std::sync::atomic::Ordering::Acquire), 1);
        backend.release.notify_one();
        backend.finished.notified().await;
        assert_eq!(backend.calls.load(std::sync::atomic::Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn shutdown_joins_configuration_backend_before_participant_stopped() {
        let (session, _) = super::super::tests::make_session_and_context().await;
        let session = Arc::new(session);
        register(&session);
        let backend = Arc::new(ConfigurationBackend {
            source: capture_factory_manager(&session.services.auth_manager).unwrap(),
            calls: Default::default(),
            started: Default::default(),
            release: Default::default(),
            finished: Default::default(),
            active: Default::default(),
        });
        let registration =
            crate::alab_destination_config::Registration::new(backend.clone()).unwrap();
        registration
            .install(&session.services.session_extension_data)
            .unwrap();
        let participant = crate::alab_destination_config::ParticipantHandle::from_store(
            &session.services.session_extension_data,
        )
        .unwrap();
        let actions = serve_actions(&session);
        tokio::pin!(actions);
        let request = participant.participant().begin(registration);
        tokio::pin!(request);
        tokio::select! {
            _ = backend.started.notified() => {},
            _ = &mut request => panic!("backend must wait"),
            _ = &mut actions => panic!("actor must remain live"),
        }
        let runtime = session
            .services
            .session_extension_data
            .get::<Arc<RuntimeParticipant>>()
            .unwrap();
        assert_eq!(backend.active.load(std::sync::atomic::Ordering::Acquire), 1);
        assert!(!runtime.0.stopped.load(std::sync::atomic::Ordering::Acquire));
        super::super::handlers::shutdown_session_runtime(&session).await;
        assert_eq!(backend.active.load(std::sync::atomic::Ordering::Acquire), 0);
        assert!(runtime.0.stopped.load(std::sync::atomic::Ordering::Acquire));
        assert!(request.await.is_err());
        assert!(participant.participant().registration().await.is_err());
        assert!(
            session
                .services
                .session_extension_data
                .get_or_init(configuration_jobs::Jobs::default)
                .closed()
        );
    }
}
