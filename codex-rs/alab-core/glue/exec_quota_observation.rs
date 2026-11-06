// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Process-local observations from the actual exec sampler. These identify a
//! live account-quota wait; native account selection remains a separate grant.

use super::ExecRecoveryOptions;
use crate::CodexThread;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_alab_core::quota::HeadlessExec;
use codex_alab_core::renewal::AuthAttempt;
use codex_alab_core::renewal::AuthRequestScope;
use codex_alab_core::renewal::RequestAuth;
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
}

/// Volatile evidence tied to one actual request attempt. It is deliberately
/// not serializable and does not authorize a different account or a new turn.
pub struct ExecQuotaWall {
    options: ExecRecoveryOptions,
    scope: Weak<AuthRequestScope>,
    attempt: AuthAttempt,
    active: AtomicBool,
}

impl std::fmt::Debug for ExecQuotaWall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecQuotaWall")
            .field("thread", &self.options.thread())
            .field("turn", &self.options.turn())
            .field("current", &self.current())
            .finish_non_exhaustive()
    }
}

impl ExecQuotaWall {
    pub fn options(&self) -> &ExecRecoveryOptions {
        &self.options
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

#[derive(Default)]
pub(crate) struct Events {
    current: watch::Sender<Option<Arc<ExecQuotaWall>>>,
    walls: std::sync::Mutex<Vec<Arc<ExecQuotaWall>>>,
}

impl Events {
    fn insert(&self, wall: Arc<ExecQuotaWall>) {
        let mut walls = self
            .walls
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
            .walls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        walls.retain(|old| !Arc::ptr_eq(old, wall));
        self.refresh(&mut walls);
    }

    fn refresh(&self, walls: &mut Vec<Arc<ExecQuotaWall>>) {
        walls.retain(|wall| wall.current());
        let next = walls.last().cloned();
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

pub(crate) struct WaitLease {
    events: Arc<Events>,
    wall: Arc<ExecQuotaWall>,
}

impl Drop for WaitLease {
    fn drop(&mut self) {
        self.wall.active.store(false, Ordering::Release);
        self.events.remove(&self.wall);
    }
}

impl CodexThread {
    /// Subscribe to this embedded exec runtime only. A resumed thread's saved
    /// origin does not enable observation in a non-exec host.
    pub fn watch_exec_quota(&self) -> Option<ExecQuotaWatch> {
        let data = self.thread_extension_data();
        data.get::<HeadlessExec>()?;
        Some(ExecQuotaWatch {
            receiver: self
                .session
                .services
                .local_agent_runtime
                .exec_quota_events()
                .current
                .subscribe(),
            root: Arc::downgrade(&self.session),
        })
    }
}

pub(crate) fn publish(sess: &Session, turn: &TurnContext) -> Option<WaitLease> {
    let scope = turn.extension_data.get::<AuthRequestScope>();
    tracing::debug!(target: "codex_alab_exec_handoff",
        turn_exec = turn.extension_data.get::<HeadlessExec>().is_some(),
        thread_exec = sess.services.thread_extension_data.get::<HeadlessExec>().is_some(),
        scope_present = scope.is_some(),
        scope_eligible = scope.as_ref().is_some_and(|scope| scope.eligible(&turn.sub_id)),
        "observing quota wait");
    let data = &sess.services.thread_extension_data;
    if !(turn.extension_data.get::<HeadlessExec>().is_some()
        && data.get::<HeadlessExec>().is_some())
        && !(turn
            .extension_data
            .get::<codex_alab_core::quota::ExecQuotaMember>()
            .is_some()
            && data
                .get::<codex_alab_core::quota::ExecQuotaMember>()
                .is_some())
    {
        return None;
    }
    let scope = turn.extension_data.get::<AuthRequestScope>()?;
    if !scope.eligible(&turn.sub_id) {
        return None;
    }
    let attempt = scope.snapshot();
    if !matches!(attempt.auth, Some(RequestAuth::ChatGpt { .. })) {
        return None;
    }
    let options = ExecRecoveryOptions::from_context(sess.thread_id, turn);
    let events = sess.services.local_agent_runtime.exec_quota_events();
    let wall = Arc::new(ExecQuotaWall {
        options,
        scope: Arc::downgrade(&scope),
        attempt,
        active: AtomicBool::new(true),
    });
    let current = wall.current();
    tracing::debug!(target: "codex_alab_exec_handoff", current, "quota observation prepared");
    if !current {
        return None;
    }
    events.insert(Arc::clone(&wall));
    Some(WaitLease { events, wall })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_core::renewal::record_auth_attempt;
    use codex_protocol::ThreadId;

    fn wall(scope: &Arc<AuthRequestScope>) -> Arc<ExecQuotaWall> {
        Arc::new(ExecQuotaWall {
            options: ExecRecoveryOptions {
                thread: ThreadId::new(),
                turn: scope.turn.clone(),
                review: false,
                schema: None,
                service_tier: None,
                cyber_access_program: None,
                step: super::super::tests::saved().step,
            },
            scope: Arc::downgrade(scope),
            attempt: scope.snapshot(),
            active: AtomicBool::new(true),
        })
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
