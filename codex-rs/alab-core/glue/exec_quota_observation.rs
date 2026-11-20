// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Process-local observations from the actual exec sampler. These identify a
//! live account-quota wait; native account selection remains a separate grant.

use super::ExecRecoveryOptions;
use crate::CodexThread;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_alab_core::exec_wall::ExecWalls;
use codex_alab_core::exec_wall::GrantHold;
use codex_alab_core::exec_wall::Verdict;
use codex_alab_core::quota::ExecQuotaMember;
use codex_alab_core::quota::HeadlessExec;
use codex_alab_core::renewal::AuthAttempt;
use codex_alab_core::renewal::AuthRequestScope;
use codex_alab_core::renewal::RequestAuth;
use codex_alab_core::wall_kind::WallKind;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::watch;

/// A tree-wide observation stream anchored to the subscribing exec root.
/// Child request evidence never supplies the root's recovery settings.
pub struct ExecQuotaWatch {
    receiver: watch::Receiver<Option<Arc<ExecQuotaWall>>>,
    root: Weak<Session>,
    events: Arc<Events>,
}

/// Authorizes the drain only while the root tree selects this exact request, and retains
/// the output owner from the instant draining begins.
pub struct ExecQuotaDrain {
    wall: Arc<ExecQuotaWall>,
    handling: Arc<AtomicBool>,
}

impl ExecQuotaDrain {
    pub fn new(wall: Arc<ExecQuotaWall>, handling: Arc<AtomicBool>) -> Self {
        Self { wall, handling }
    }

    pub fn observation(&self) -> Weak<ExecQuotaWall> {
        Arc::downgrade(&self.wall)
    }

    pub fn current(&self, watch: &ExecQuotaWatch) -> bool {
        watch.observes(&self.wall)
    }

    pub fn commit(&self, watch: &ExecQuotaWatch, drain: impl FnOnce()) -> bool {
        watch.commit_if_observed(&self.wall, || {
            self.handling.store(true, Ordering::Release);
            drain();
        })
    }
}

impl std::ops::Deref for ExecQuotaWatch {
    type Target = watch::Receiver<Option<Arc<ExecQuotaWall>>>;
    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

impl std::ops::DerefMut for ExecQuotaWatch {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.receiver
    }
}

impl ExecQuotaWatch {
    /// The exact still-authorized observation selected by this tree. An observation from
    /// another child can stay live after a higher-priority or newer wall supersedes it.
    pub fn observes(&self, wall: &ExecQuotaWall) -> bool {
        self.borrow()
            .as_ref()
            .is_some_and(|selected| std::ptr::eq(selected.as_ref(), wall))
            && wall.current()
    }

    /// Commit a synchronous drain only while this tree still selects the authenticated request.
    /// The observation lock serializes replacement and lease retirement with the actual commit.
    fn commit_if_observed(&self, wall: &ExecQuotaWall, commit: impl FnOnce()) -> bool {
        let _observations = self
            .events
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.observes(wall) {
            return false;
        }
        commit();
        true
    }

    /// Snapshot the root's regular turn or registered review consumer. Suspension
    /// must later validate this exact turn before any replacement can execute.
    pub async fn root_options(&self) -> Option<ExecRecoveryOptions> {
        let root = self.root.upgrade()?;
        root.services.thread_extension_data.get::<HeadlessExec>()?;
        let active = root.active_turn.lock().await;
        let task = active.as_ref()?.task.as_ref()?;
        if !(task.kind == crate::state::TaskKind::Regular
            || (task.kind == crate::state::TaskKind::Review
                && crate::tasks::review_recovery::has_consumer(&task.turn_context)))
            || task.cancellation_token.is_cancelled()
            || task
                .turn_context
                .extension_data
                .get::<HeadlessExec>()
                .is_none()
        {
            return None;
        }
        Some(ExecRecoveryOptions::from_context(
            root.thread_id,
            &task.turn_context,
        ))
    }

    /// The host's verdict for wall `kind` of root turn `root` (gap 4, step S3): whether this
    /// tree's requests keep waiting at such a wall ([`ExecWalls::keep`]). Only a host-kept wall
    /// reads it.
    pub fn decide(&self, root: &str, kind: &WallKind, verdict: Verdict) {
        self.events.walls.decide(root, kind, verdict);
    }

    /// Hold a grant for root turn `root`, armed or reserving, until the hold drops: meanwhile a
    /// kept wait does not end on its timer, so its observation, which the grant needs, stays.
    pub fn hold_grant(&self, root: &str) -> GrantHold {
        self.events.walls.hold(root)
    }
}

/// Volatile evidence tied to one actual request attempt. It is deliberately
/// not serializable and does not authorize a different account or a new turn.
pub struct ExecQuotaWall {
    options: ExecRecoveryOptions,
    scope: Weak<AuthRequestScope>,
    attempt: AuthAttempt,
    active: AtomicBool,
    kind: WallKind,
}

impl std::fmt::Debug for ExecQuotaWall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecQuotaWall")
            .field("thread", &self.options.thread())
            .field("turn", &self.options.turn())
            .field("kind", &self.kind)
            .field("current", &self.current())
            .finish_non_exhaustive()
    }
}

impl ExecQuotaWall {
    pub fn options(&self) -> &ExecRecoveryOptions {
        &self.options
    }

    /// The bucket the observed request's wall stands in, a model's own limit by its limit id.
    pub fn kind(&self) -> &WallKind {
        &self.kind
    }

    /// Must be rechecked before using an observation after an await.
    pub fn current(&self) -> bool {
        if !self.active.load(Ordering::Acquire) {
            return false;
        }
        let Some(scope) = self.scope.upgrade() else {
            return false;
        };
        let now = scope.snapshot();
        scope.eligible(self.options.turn())
            && now.auth == self.attempt.auth
            && now.owner_generation == self.attempt.owner_generation
            && now.generation == self.attempt.generation
            && self.attempt.credentials_changed() == Some(false)
    }

    /// Account identifiers only, never credential bytes. A native consumer
    /// hashes these through the shared aterm account adapter.
    pub fn source(&self) -> Option<RequestAuth> {
        self.current().then(|| self.attempt.auth.clone()).flatten()
    }
}

/// The exec tree's observations, and its host-kept walls. Each exec member's turn carries it
/// ([`bind`]), so the quota retry, which sees no session, reads the tree's keep state.
#[derive(Default)]
pub(crate) struct Events {
    current: watch::Sender<Option<Arc<ExecQuotaWall>>>,
    observed: std::sync::Mutex<Vec<Arc<ExecQuotaWall>>>,
    walls: Arc<ExecWalls>,
}

impl Events {
    /// How long a refusal at host-kept wall `kind` waits before its retry ([`ExecWalls::keep`]).
    /// An exec handoff runtime is subscribed while one holds this tree's watch.
    pub(crate) fn keep(
        &self,
        kind: &WallKind,
        now: std::time::Instant,
    ) -> Option<std::time::Duration> {
        self.walls
            .keep(kind, self.current.receiver_count() > 0, now)
    }

    pub(crate) fn walls(&self) -> &Arc<ExecWalls> {
        &self.walls
    }

    /// A runtime's subscription to the tree's current observation.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<Arc<ExecQuotaWall>>> {
        self.current.subscribe()
    }

    fn insert(&self, wall: Arc<ExecQuotaWall>) {
        let mut walls = self
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        walls.retain(|old| {
            if old.options.thread() == wall.options.thread() {
                old.active.store(false, Ordering::Release);
            }
            old.current()
        });
        walls.push(wall);
        self.refresh(&mut walls);
    }

    fn remove(&self, wall: &Arc<ExecQuotaWall>) {
        let mut walls = self
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        wall.active.store(false, Ordering::Release);
        walls.retain(|old| !Arc::ptr_eq(old, wall));
        self.refresh(&mut walls);
    }

    /// Publish the live observation the runtime acts on: the latest of the most pressing kind
    /// ([`precedence`]), so a member's wait at another kind of wall never hides an ordinary one.
    fn refresh(&self, walls: &mut Vec<Arc<ExecQuotaWall>>) {
        walls.retain(|wall| wall.current());
        // `min_by_key` keeps the first of equals: reversed, the latest.
        let next = walls
            .iter()
            .rev()
            .min_by_key(|wall| precedence(&wall.kind))
            .cloned();
        self.current.send_if_modified(|current| {
            let same = match (&*current, &next) {
                (None, None) => true,
                (Some(current), Some(next)) => Arc::ptr_eq(current, next),
                _ => false,
            };
            if same {
                false
            } else {
                *current = next;
                true
            }
        });
    }
}

/// Which live observation the exec runtime acts on, lowest first. The tree's watch carries one,
/// and the runtime asks its host about that one only:
/// - an ordinary wall, whose ask no policy word gates and which waits to its reset whatever the
///   answer, comes first;
/// - then a host-kept wall ([`WallKind::host_kept`]), asked only where the policy leaves one. The
///   ask is the same `prepare`, and a move takes the whole tree. While an ordinary wall stands, a
///   host-kept wall of the same tree gets no verdict of its own: unless a grant for the root turn
///   is held, it ends at its second refusal, as an unanswered one does
///   (`codex_alab_core::exec_wall`);
/// - last a model's own limit, which only an explicitly capable host can move.
fn precedence(kind: &WallKind) -> u8 {
    match kind {
        WallKind::Ordinary => 0,
        WallKind::Workspace(_) => 1,
        WallKind::Limit(_) => 2,
    }
}

pub(crate) struct WaitLease {
    events: Arc<Events>,
    wall: Arc<ExecQuotaWall>,
}

impl Drop for WaitLease {
    fn drop(&mut self) {
        self.events.remove(&self.wall);
    }
}

impl CodexThread {
    /// Subscribe to this embedded exec runtime only. A resumed thread's saved
    /// origin does not enable observation in a non-exec host.
    pub fn watch_exec_quota(&self) -> Option<ExecQuotaWatch> {
        let data = self.thread_extension_data();
        data.get::<HeadlessExec>()?;
        let events = self
            .session
            .services
            .local_agent_runtime
            .exec_quota_events();
        Some(ExecQuotaWatch {
            receiver: events.subscribe(),
            root: Arc::downgrade(&self.session),
            events,
        })
    }
}

/// Whether this turn's quota waits are the exec host's: the root's own turn, or a tree member's.
fn exec_member(sess: &Session, turn: &TurnContext) -> bool {
    let data = &sess.services.thread_extension_data;
    (turn.extension_data.get::<HeadlessExec>().is_some() && data.get::<HeadlessExec>().is_some())
        || (turn.extension_data.get::<ExecQuotaMember>().is_some()
            && data.get::<ExecQuotaMember>().is_some())
}

/// Give an exec member's turn its tree's observations and host-kept walls, before its first
/// request, and name the root's own turn as the tree's root turn: a new one starts the walls over.
/// An exec review's own turn, which only its delegate samples for, is bound as its task begins
/// (`exec_review_stream.rs` `begin`): it is the root turn [`ExecQuotaWatch::root_options`] names.
pub(crate) fn bind(sess: &Session, turn: &TurnContext) {
    if !codex_alab_core::active() || !exec_member(sess, turn) {
        return;
    }
    let events = sess.services.local_agent_runtime.exec_quota_events();
    if turn.extension_data.get::<HeadlessExec>().is_some()
        && sess
            .services
            .thread_extension_data
            .get::<HeadlessExec>()
            .is_some()
        && !turn.session_source.is_non_root_agent()
    {
        events.walls.root_turn(&turn.sub_id);
    }
    if turn.extension_data.get::<Events>().is_none() {
        turn.extension_data.insert_arc(events);
    }
}

pub(crate) fn publish(sess: &Session, turn: &TurnContext, kind: WallKind) -> Option<WaitLease> {
    let scope = turn.extension_data.get::<AuthRequestScope>();
    tracing::debug!(target: "codex_alab_exec_handoff",
        turn_exec = turn.extension_data.get::<HeadlessExec>().is_some(),
        thread_exec = sess.services.thread_extension_data.get::<HeadlessExec>().is_some(),
        scope_present = scope.is_some(),
        scope_eligible = scope.as_ref().is_some_and(|scope| scope.eligible(&turn.sub_id)),
        ?kind,
        "observing quota wait");
    if !exec_member(sess, turn) {
        return None;
    }
    let scope = turn.extension_data.get::<AuthRequestScope>()?;
    observe(
        sess.services.local_agent_runtime.exec_quota_events(),
        &scope,
        ExecRecoveryOptions::from_context(sess.thread_id, turn),
        kind,
    )
}

/// Publish the wait of the request `scope` admits, at a wall of `kind`, for as long as the lease.
fn observe(
    events: Arc<Events>,
    scope: &Arc<AuthRequestScope>,
    options: ExecRecoveryOptions,
    kind: WallKind,
) -> Option<WaitLease> {
    if !scope.eligible(options.turn()) {
        return None;
    }
    let attempt = scope.snapshot();
    // Any attempted login is observed: the handoff controller moves only a ChatGPT one and
    // tells an exec run with another kind of login, once, why it waits (CONTRACT §1.1).
    attempt.auth.as_ref()?;
    let wall = Arc::new(ExecQuotaWall {
        options,
        scope: Arc::downgrade(scope),
        attempt,
        active: AtomicBool::new(true),
        kind,
    });
    let current = wall.current();
    tracing::debug!(target: "codex_alab_exec_handoff", current, "quota observation prepared");
    if !current {
        return None;
    }
    events.insert(Arc::clone(&wall));
    Some(WaitLease { events, wall })
}

/// The rate limits the session's latest model response reported. They carry no
/// account, permission or observation time, so the usage preflight uses them
/// only as evidence against reusing an earlier clear reading.
pub(crate) async fn response_limits(
    sess: &Session,
) -> Option<codex_protocol::protocol::RateLimitSnapshot> {
    sess.state.lock().await.latest_rate_limits.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_core::renewal::record_auth_attempt;
    use codex_protocol::ThreadId;

    fn options(scope: &Arc<AuthRequestScope>) -> ExecRecoveryOptions {
        ExecRecoveryOptions {
            direct_only_tool_namespaces: None,
            prompt_cache_ttl: None,
            thread: ThreadId::new(),
            turn: scope.turn.clone(),
            review: false,
            schema: None,
            service_tier: None,
            cyber_access_program: None,
            helper_choice: None,
            step: super::super::tests::saved().step,
        }
    }

    fn wall(scope: &Arc<AuthRequestScope>) -> Arc<ExecQuotaWall> {
        Arc::new(ExecQuotaWall {
            options: options(scope),
            scope: Arc::downgrade(scope),
            attempt: scope.snapshot(),
            active: AtomicBool::new(true),
            kind: WallKind::Ordinary,
        })
    }

    /// AN OBSERVATION CARRIES ITS WALL'S KIND, a model's own limit by its limit id, so the
    /// runtime can tell what to ask (gap 4, step S3): every usage limit a run waits on is
    /// published, not only the ordinary bucket's.
    #[test]
    fn an_observation_carries_its_walls_kind_and_limit() {
        use codex_alab_core::wall_kind::Workspace;
        let (scope, _owner) = scope();
        let events = Arc::new(Events::default());
        let receiver = events.current.subscribe();
        for kind in [
            WallKind::Limit("codex_other".into()),
            WallKind::Workspace(Workspace::MemberUsageLimitReached),
            WallKind::Ordinary,
        ] {
            let lease = observe(Arc::clone(&events), &scope, options(&scope), kind.clone())
                .expect("a live request's wait is observed");
            let observed = receiver.borrow().clone().expect("published");
            assert_eq!(observed.kind(), &kind);
            assert!(format!("{observed:?}").contains(&format!("{kind:?}")));
            drop(lease);
            assert!(receiver.borrow().is_none());
        }
        // A request that is no longer the turn's own is not observed, whatever its kind.
        let stale = Arc::new(AuthRequestScope::new(
            "another-turn".into(),
            None,
            1,
            4,
            || true,
        ));
        assert!(observe(events, &stale, options(&scope), WallKind::Ordinary).is_none());
    }

    /// THE VERDICT IS THE TREE'S AND STARTS OVER AT A NEW WALL: one decided at the root's
    /// observation keeps a child's wait at the same kind of wall too, through the child's own
    /// handle on the tree's runtime; a new root turn, or another kind, has none; and no runtime
    /// subscribed means no host-kept wait at all.
    #[test]
    fn the_verdict_is_shared_across_the_exec_tree_and_resets_at_a_new_wall() {
        use codex_alab_core::exec_wall::UNKNOWN_RESET_WAIT;
        use codex_alab_core::wall_kind::Workspace;
        let runtime = crate::agent::LocalAgentControl::default().runtime;
        let root = runtime.exec_quota_events();
        let child = runtime.exec_quota_events();
        let now = std::time::Instant::now();
        let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);
        root.walls().root_turn("root-turn");
        assert_eq!(root.keep(&credits, now), None, "no runtime subscribed");
        let receiver = root.current.subscribe();
        assert_eq!(child.keep(&credits, now), Some(UNKNOWN_RESET_WAIT));
        // The child's first wait is the tree's first wait at this wall.
        child.walls().waited(&credits, now);
        assert_eq!(
            root.keep(&credits, now),
            None,
            "unanswered after the first wait"
        );
        // The runtime answers through the root's watch; the child's next refusal is kept.
        root.walls().decide("root-turn", &credits, Verdict::Keep);
        assert_eq!(child.keep(&credits, now), Some(UNKNOWN_RESET_WAIT));
        assert_eq!(root.keep(&credits, now), Some(UNKNOWN_RESET_WAIT));
        // Another kind of wall has its own first wait and no verdict yet.
        let cap = WallKind::Workspace(Workspace::MemberUsageLimitReached);
        child.walls().waited(&cap, now);
        assert_eq!(child.keep(&cap, now), None);
        assert_eq!(child.keep(&credits, now), Some(UNKNOWN_RESET_WAIT));
        // A new root turn starts over.
        child.walls().root_turn("next-turn");
        assert_eq!(root.walls().verdict(&credits), Verdict::Unknown);
        root.walls().waited(&credits, now);
        assert_eq!(child.keep(&credits, now), None);
        drop(receiver);
        root.walls().decide("next-turn", &credits, Verdict::Keep);
        assert_eq!(child.keep(&credits, now), None, "the runtime is gone");
        // Another tree shares nothing.
        let other = crate::agent::LocalAgentControl::default()
            .runtime
            .exec_quota_events();
        assert_eq!(other.walls().verdict(&credits), Verdict::Unknown);
    }

    /// A HELD GRANT KEEPS A KEPT WAIT'S OBSERVATION LIVE PAST ITS TIMER: a retry would end it, and
    /// the runtime cancels a grant whose observation ended. Letting the grant go ends the wait
    /// and then the observation.
    #[tokio::test(start_paused = true)]
    async fn an_armed_grant_is_not_cancelled_by_a_kept_waits_timer() {
        use codex_alab_core::exec_wall::UNKNOWN_RESET_WAIT;
        let (scope, _owner) = scope();
        let events = Arc::new(Events::default());
        events.walls().root_turn("root-turn");
        let lease = WaitLease {
            events: Arc::clone(&events),
            wall: wall(&scope),
        };
        let observed = Arc::clone(&lease.wall);
        events.insert(Arc::clone(&observed));
        let hold = events.walls.hold("root-turn");
        let waiting = {
            let events = Arc::clone(&events);
            tokio::spawn(async move {
                let _lease = lease;
                events
                    .walls()
                    .kept_wait(tokio::time::sleep(UNKNOWN_RESET_WAIT))
                    .await;
            })
        };
        tokio::time::sleep(UNKNOWN_RESET_WAIT * 3).await;
        assert!(!waiting.is_finished());
        assert!(observed.current(), "the grant's observation stands");
        drop(hold);
        tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .expect("the wait ends with the grant")
            .expect("the wait");
        assert!(!observed.current());
        assert!(events.current.borrow().is_none());
    }

    fn scope() -> (Arc<AuthRequestScope>, Arc<()>) {
        let scope = Arc::new(AuthRequestScope::new(
            "original-turn".into(),
            Some(RequestAuth::ChatGpt {
                account: "fixture-account".into(),
                user: "fixture-user".into(),
            }),
            1,
            4,
            || true,
        ));
        let owner = Arc::new(());
        scope.register(Arc::clone(&owner));
        record_auth_attempt(
            Arc::as_ptr(&owner) as usize,
            &scope.turn,
            scope.snapshot().auth,
            1,
            4,
            || true,
            || false,
        );
        (scope, owner)
    }

    #[test]
    fn initial_auth_binding_is_not_an_actual_request_attempt() {
        let scope = Arc::new(AuthRequestScope::new(
            "original-turn".into(),
            Some(RequestAuth::ChatGpt {
                account: "fixture-account".into(),
                user: "fixture-user".into(),
            }),
            1,
            4,
            || true,
        ));
        assert!(wall(&scope).source().is_none());
    }

    #[test]
    fn replacing_a_wait_revokes_only_the_old_observation() {
        let (scope, _owner) = scope();
        let events = Arc::new(Events::default());
        let old = wall(&scope);
        events.insert(Arc::clone(&old));
        let old_lease = WaitLease {
            events: Arc::clone(&events),
            wall: Arc::clone(&old),
        };
        let mut new = wall(&scope);
        Arc::get_mut(&mut new).unwrap().options.thread = old.options.thread();
        events.insert(Arc::clone(&new));
        let new_lease = WaitLease {
            events: Arc::clone(&events),
            wall: Arc::clone(&new),
        };
        drop(old_lease);
        assert!(!old.current());
        assert!(new.current());
        assert!(Arc::ptr_eq(events.current.borrow().as_ref().unwrap(), &new));
        drop(new_lease);
        assert!(!new.current());
        assert!(events.current.borrow().is_none());
    }

    #[test]
    fn concurrent_member_waits_remain_independently_live() {
        let (root_scope, _root_owner) = scope();
        let (child_scope, _child_owner) = scope();
        let events = Arc::new(Events::default());
        let root = wall(&root_scope);
        let child = wall(&child_scope);
        events.insert(Arc::clone(&root));
        let root_lease = WaitLease {
            events: Arc::clone(&events),
            wall: Arc::clone(&root),
        };
        events.insert(Arc::clone(&child));
        let child_lease = WaitLease {
            events: Arc::clone(&events),
            wall: Arc::clone(&child),
        };
        assert!(root.current());
        assert!(child.current());
        assert!(Arc::ptr_eq(
            events.current.borrow().as_ref().unwrap(),
            &child
        ));
        drop(child_lease);
        assert!(!child.current());
        assert!(Arc::ptr_eq(
            events.current.borrow().as_ref().unwrap(),
            &root
        ));
        drop(root_lease);
        assert!(events.current.borrow().is_none());
    }

    /// AN ORDINARY WALL STAYS THE RUNTIME'S WHILE A MEMBER WAITS AT ANOTHER KIND (gap 4, step
    /// S3): the runtime asks its host about the watch's observation only, and asks nothing at a
    /// model's own limit, so a member's later wait there, or at a workspace's wall, must neither
    /// hide the root's ordinary wait nor wake the runtime. Each more pressing wall takes the
    /// watch as it comes, and among one kind the latest is current, as before.
    #[test]
    fn an_ordinary_wall_stays_current_while_a_member_waits_at_another_kind() {
        use codex_alab_core::wall_kind::Workspace;
        let events = Arc::new(Events::default());
        let mut runtime = ExecQuotaWatch {
            receiver: events.subscribe(),
            root: Weak::new(),
            events: Arc::clone(&events),
        };
        let current = || {
            events
                .current
                .borrow()
                .clone()
                .expect("published")
                .kind()
                .clone()
        };
        let scopes = [scope(), scope(), scope(), scope()];
        let at = |index: usize, kind: &WallKind| {
            let scope = &scopes[index].0;
            observe(Arc::clone(&events), scope, options(scope), kind.clone())
                .expect("a live request's wait is observed")
        };
        let model = WallKind::Limit("codex_other".into());
        let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);

        let ordinary = at(0, &WallKind::Ordinary);
        runtime.borrow_and_update();
        let limit = at(1, &model);
        assert_eq!(
            current(),
            WallKind::Ordinary,
            "a model's limit hides nothing"
        );
        assert!(!runtime.has_changed().unwrap(), "nor wakes the runtime");
        assert!(
            limit.wall.current(),
            "the named request itself is still current"
        );
        assert!(
            !runtime.observes(&limit.wall),
            "only the selected wall can move"
        );
        assert!(runtime.observes(&ordinary.wall));
        let workspace = at(2, &credits);
        assert_eq!(current(), WallKind::Ordinary, "nor does a workspace's wall");
        assert!(!runtime.has_changed().unwrap());
        drop(ordinary);
        assert_eq!(
            current(),
            credits,
            "a workspace's wall before a model's limit"
        );
        drop(workspace);
        assert_eq!(current(), model);
        assert!(runtime.observes(&limit.wall));
        let another = at(3, &WallKind::Limit("codex_second".into()));
        assert!(limit.wall.current() && !runtime.observes(&limit.wall));
        assert!(runtime.observes(&another.wall));
        drop(another);

        // The other order: each more pressing wall takes the watch.
        let workspace = at(2, &credits);
        assert_eq!(current(), credits);
        let ordinary = at(0, &WallKind::Ordinary);
        assert_eq!(current(), WallKind::Ordinary);
        // Among one kind, the latest.
        let latest = at(3, &WallKind::Ordinary);
        assert!(Arc::ptr_eq(
            events.current.borrow().as_ref().unwrap(),
            &latest.wall
        ));
        drop(latest);
        assert!(Arc::ptr_eq(
            events.current.borrow().as_ref().unwrap(),
            &ordinary.wall
        ));
        drop((ordinary, workspace, limit));
        assert!(events.current.borrow().is_none());
    }

    #[test]
    fn retiring_another_member_does_not_clear_the_selected_wall() {
        let (root_scope, _root_owner) = scope();
        let (child_scope, _child_owner) = scope();
        let events = Arc::new(Events::default());
        let root = wall(&root_scope);
        let child = wall(&child_scope);
        events.insert(Arc::clone(&root));
        let root_lease = WaitLease {
            events: Arc::clone(&events),
            wall: root,
        };
        events.insert(Arc::clone(&child));
        drop(root_lease);
        assert!(child.current());
        assert!(Arc::ptr_eq(
            events.current.borrow().as_ref().unwrap(),
            &child
        ));
    }

    #[test]
    fn a_new_attempt_invalidates_the_old_source_even_for_the_same_account() {
        let (scope, owner) = scope();
        let observed = wall(&scope);
        assert!(observed.source().is_some());
        record_auth_attempt(
            Arc::as_ptr(&owner) as usize,
            &scope.turn,
            scope.snapshot().auth,
            1,
            5,
            || true,
            || false,
        );
        assert!(!observed.current());
        assert!(observed.source().is_none());
        let replacement = wall(&scope);
        assert!(replacement.current());
        record_auth_attempt(
            Arc::as_ptr(&owner) as usize,
            &scope.turn,
            scope.snapshot().auth,
            2,
            5,
            || true,
            || false,
        );
        assert!(replacement.source().is_none());
    }

    #[test]
    fn changed_credentials_or_a_dropped_request_revoke_source_identity() {
        let (scope, owner) = scope();
        let changed = Arc::new(AtomicBool::new(false));
        let current_changed = Arc::clone(&changed);
        record_auth_attempt(
            Arc::as_ptr(&owner) as usize,
            &scope.turn,
            scope.snapshot().auth,
            1,
            4,
            || true,
            move || current_changed.load(Ordering::Acquire),
        );
        let observed = wall(&scope);
        assert!(observed.current());
        changed.store(true, Ordering::Release);
        assert!(observed.source().is_none());
        changed.store(false, Ordering::Release);
        assert!(observed.current());
        drop(scope);
        assert!(observed.source().is_none());
    }
}
