// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Coordinate already-admitted original turns before any resumed task runs.
//! This barrier supplies ordering, never native execution authority.

use crate::CodexThread;
use crate::ExecTreeSnapshot;
use crate::session::input_queue::exec_input_state;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
enum Status {
    Waiting,
    Ready,
    Released,
    Failed(String),
}

struct Participant {
    turn: String,
    kind: TaskKind,
    session: Weak<Session>,
}

struct Inner {
    members: HashMap<ThreadId, Participant>,
    arrived: Mutex<HashSet<ThreadId>>,
    status: watch::Sender<Status>,
}

/// Bound to the fresh runtimes and exact original turns of one decoded tree.
#[derive(Clone)]
pub struct ExecLaunchBarrier(Arc<Inner>);

/// A turn-only ordering attachment. It cannot replace native turn admission.
#[derive(Clone)]
pub struct ExecLaunchPermit {
    barrier: ExecLaunchBarrier,
    thread: ThreadId,
    turn: String,
}

// An idle member has no original turn. Any real input arriving during restore
// may register a new task, but its body must wait for the same tree release.
struct IdleLaunch {
    barrier: ExecLaunchBarrier,
    session: Weak<Session>,
}

impl ExecLaunchBarrier {
    /// Prepare after all captured members have been loaded, before recovering
    /// any original turn. Idle queues must be staged before the permits run.
    pub fn prepare(tree: &ExecTreeSnapshot, threads: &[Arc<CodexThread>]) -> Result<Self, String> {
        Self::prepare_forest(&[tree], threads)
    }

    /// One release boundary for every fresh runtime in the complete forest.
    pub fn prepare_forest(
        trees: &[&ExecTreeSnapshot],
        threads: &[Arc<CodexThread>],
    ) -> Result<Self, String> {
        if trees.is_empty() {
            return Err("forest launch has no roots".into());
        }
        let mut loaded = HashMap::new();
        for thread in threads {
            if loaded.insert(thread.session.thread_id, thread).is_some() {
                return Err("exec launch repeats a loaded thread".into());
            }
        }
        let mut members = HashMap::new();
        let mut idle = Vec::new();
        let mut seen = HashSet::new();
        for tree in trees {
            tree.validate(tree.root(), tree.turn())?;
            let root = loaded
                .get(&tree.root())
                .ok_or("exec launch root is missing")?;
            for node in tree.threads() {
                if !seen.insert(node.thread()) {
                    return Err("forest launch repeats a member".into());
                }
                let thread = loaded
                    .get(&node.thread())
                    .ok_or("exec launch member is missing")?;
                if !thread.is_running() || !root.shares_exec_tree(thread) {
                    return Err("exec launch members do not share a live runtime".into());
                }
                if let Some(recovery) = node.recovery() {
                    if exec_input_state::pending_origin(&thread.session)
                        != Some((node.thread(), recovery.turn().to_owned()))
                    {
                        return Err(
                            "exec launch member has no matching pending original input".into()
                        );
                    }
                    members.insert(
                        node.thread(),
                        Participant {
                            turn: recovery.turn().to_owned(),
                            kind: if recovery.is_review() {
                                TaskKind::Review
                            } else {
                                TaskKind::Regular
                            },
                            session: Arc::downgrade(&thread.session),
                        },
                    );
                } else {
                    idle.push(Arc::clone(&thread.session));
                }
            }
        }
        if seen.len() != loaded.len() {
            return Err("forest launch does not contain the complete loaded inventory".into());
        }
        let barrier = Self::from_members(members);
        for session in idle {
            session.services.thread_extension_data.insert(IdleLaunch {
                barrier: barrier.clone(),
                session: Arc::downgrade(&session),
            });
        }
        Ok(barrier)
    }

    fn from_members(members: HashMap<ThreadId, Participant>) -> Self {
        let initial = if members.is_empty() {
            Status::Ready
        } else {
            Status::Waiting
        };
        let (status, _) = watch::channel(initial);
        Self(Arc::new(Inner {
            members,
            arrived: Mutex::new(HashSet::new()),
            status,
        }))
    }

    pub fn permit(&self, thread: ThreadId) -> Result<ExecLaunchPermit, String> {
        let member = self
            .0
            .members
            .get(&thread)
            .ok_or("thread is not an active exec launch member")?;
        Ok(ExecLaunchPermit {
            barrier: self.clone(),
            thread,
            turn: member.turn.clone(),
        })
    }

    /// A failed admission aborts the waiting or ready group. After release,
    /// ordinary task cancellation owns any later interruption.
    pub fn abort(&self, reason: String) -> bool {
        let _arrived = self
            .0
            .arrived
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !matches!(*self.0.status.borrow(), Status::Waiting | Status::Ready) {
            return false;
        }
        self.0.status.send_replace(Status::Failed(reason));
        true
    }

    pub async fn wait_ready(&self) -> Result<(), String> {
        self.wait(false).await
    }

    /// Release only after every task is registered and the controller has
    /// finished restoring idle queues and shared delivery state.
    pub fn release(&self) -> Result<(), String> {
        let _arrived = self
            .0
            .arrived
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = self.0.status.borrow().clone();
        match current {
            Status::Ready | Status::Released => {}
            Status::Waiting => return Err("exec launch still has unregistered tasks".into()),
            Status::Failed(reason) => return Err(reason),
        }
        self.0.status.send_replace(Status::Released);
        Ok(())
    }

    async fn wait(&self, released: bool) -> Result<(), String> {
        let mut status = self.0.status.subscribe();
        loop {
            let current = status.borrow_and_update().clone();
            match current {
                Status::Released => return Ok(()),
                Status::Ready if !released => return Ok(()),
                Status::Failed(reason) => return Err(reason),
                Status::Waiting | Status::Ready => {}
            }
            status
                .changed()
                .await
                .map_err(|_| "exec launch owner disappeared".to_owned())?;
        }
    }
}

impl ExecLaunchPermit {
    pub fn matches(&self, thread: ThreadId, turn: &str) -> bool {
        self.thread == thread && self.turn == turn
    }

    async fn arrive(
        &self,
        session: &Arc<Session>,
        context: &Arc<TurnContext>,
        cancelled: &CancellationToken,
    ) -> Result<(), String> {
        let result = self.register(session, context, cancelled).await;
        if let Err(reason) = result {
            self.barrier.abort(reason.clone());
            return Err(reason);
        }
        tokio::select! {
            biased;
            _ = cancelled.cancelled() => {
                self.barrier.abort("exec launch participant was cancelled".into());
                Err("exec launch participant was cancelled".into())
            }
            ready = self.barrier.wait(true) => ready,
        }
    }

    async fn register(
        &self,
        session: &Arc<Session>,
        context: &Arc<TurnContext>,
        cancelled: &CancellationToken,
    ) -> Result<(), String> {
        let member = self
            .barrier
            .0
            .members
            .get(&self.thread)
            .ok_or("exec launch member disappeared")?;
        if !self.matches(session.thread_id, &context.sub_id)
            || !member
                .session
                .upgrade()
                .is_some_and(|expected| Arc::ptr_eq(&expected, session))
        {
            return Err("exec launch attachment belongs to another runtime or turn".into());
        }
        // start_task holds this lock through assigning RunningTask. The spawned
        // future cannot count as ready until that registration has finished.
        {
            let active = tokio::select! {
                biased;
                _ = cancelled.cancelled() => return Err("exec launch participant was cancelled".into()),
                active = session.active_turn.lock() => active,
            };
            if !active
                .as_ref()
                .and_then(|turn| turn.task.as_ref())
                .is_some_and(|task| {
                    task.kind == member.kind && Arc::ptr_eq(&task.turn_context, context)
                })
            {
                return Err("exec launch task is not registered for its original context".into());
            }
        }
        let mut arrived = self
            .barrier
            .0
            .arrived
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Status::Failed(reason) = self.barrier.0.status.borrow().clone() {
            return Err(reason);
        }
        if !arrived.insert(self.thread) {
            return Err("exec launch member registered twice".into());
        }
        self.barrier
            .0
            .status
            .send_replace(if arrived.len() == self.barrier.0.members.len() {
                Status::Ready
            } else {
                Status::Waiting
            });
        Ok(())
    }
}

pub(super) async fn admit(
    session: &Arc<Session>,
    context: &Arc<TurnContext>,
    cancelled: &CancellationToken,
) -> Result<(), CodexErr> {
    if let Some(permit) = context.extension_data.get::<ExecLaunchPermit>() {
        permit
            .arrive(session, context, cancelled)
            .await
            .map_err(CodexErr::Fatal)?;
    } else if let Some(idle) = session.services.thread_extension_data.get::<IdleLaunch>() {
        if !idle
            .session
            .upgrade()
            .is_some_and(|expected| Arc::ptr_eq(&expected, session))
        {
            return Err(CodexErr::Fatal(
                "idle launch belongs to another runtime".into(),
            ));
        }
        tokio::select! {
            biased;
            _ = cancelled.cancelled() => {
                idle.barrier.abort("idle launch task was cancelled".into());
                return Err(CodexErr::Fatal("idle launch task was cancelled".into()));
            }
            ready = idle.barrier.wait(true) => ready.map_err(CodexErr::Fatal)?,
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) trait ExecTaskLaunch {
    fn run_with_exec_launch(
        self,
        session: Arc<Session>,
        context: Arc<TurnContext>,
        input: Vec<crate::session::input_queue::TurnInput>,
        cancelled: CancellationToken,
    ) -> impl std::future::Future<Output = super::SessionTaskResult> + Send;
}

#[cfg(test)]
impl ExecTaskLaunch for Arc<dyn super::AnySessionTask> {
    async fn run_with_exec_launch(
        self,
        session: Arc<Session>,
        context: Arc<TurnContext>,
        input: Vec<crate::session::input_queue::TurnInput>,
        cancelled: CancellationToken,
    ) -> super::SessionTaskResult {
        admit(&session, &context, &cancelled).await?;
        self.run(session, context, input, cancelled).await
    }
}

#[cfg(test)]
#[path = "exec_launch_barrier_tests.rs"]
mod tests;
