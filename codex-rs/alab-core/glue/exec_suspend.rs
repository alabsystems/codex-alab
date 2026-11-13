// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Embedded exec's whole-tree suspension boundary, not a JSON-RPC method.
//! A writer-closed snapshot is data, never native account-selection authority.
use super::MessageProcessor;
use codex_alab_core::quota::HeadlessExec;
use codex_core::ExecRecoveryOptions;
use codex_core::ThreadManager;
use codex_core::config::ExecConfiguration;
use codex_protocol::ThreadId;
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct SuspendedExec {
    tree: codex_core::ExecTreeSnapshot,
    options: ExecRecoveryOptions,
}
impl SuspendedExec {
    pub fn thread(&self) -> ThreadId {
        self.tree.root()
    }
    pub fn turn(&self) -> &str {
        self.options.turn()
    }
    pub fn options(&self) -> &ExecRecoveryOptions {
        &self.options
    }
    pub fn tree(&self) -> &codex_core::ExecTreeSnapshot {
        &self.tree
    }
    pub fn ephemeral_history(&self) -> Option<&codex_core::ExecEphemeralHistory> {
        self.tree
            .threads()
            .first()
            .and_then(codex_core::ExecThreadSnapshot::ephemeral_history)
    }
}

#[path = "exec_recover.rs"]
mod recovery;
#[cfg(test)]
use recovery::ExecRecoveryRequest;
#[path = "exec_bootstrap_server.rs"]
mod bootstrap;
pub use bootstrap::BootstrappedExec;
pub use bootstrap::BootstrappedForest;
pub use bootstrap::ExecBootstrapRequest;
pub use bootstrap::ForestBootstrapRequest;
pub use bootstrap::FreshRootStartup;

pub(crate) enum Request {
    BootstrapForest {
        request: ForestBootstrapRequest,
        reply: oneshot::Sender<Result<BootstrappedForest, String>>,
    },
    ActivateForest {
        primary: ThreadId,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Bootstrap {
        request: ExecBootstrapRequest,
        reply: oneshot::Sender<Result<BootstrappedExec, String>>,
    },
    WatchQuota {
        thread: ThreadId,
        reply: oneshot::Sender<Result<codex_core::ExecQuotaWatch, String>>,
    },
    Suspend {
        thread: ThreadId,
        turn: String,
        configuration: Box<ExecConfiguration>,
        reply: oneshot::Sender<Result<SuspendedExec, String>>,
    },
    ActivateIdleTree {
        thread: ThreadId,
        reply: oneshot::Sender<Result<(), String>>,
    },
    ActivateTree {
        thread: ThreadId,
        turn: String,
        reply: oneshot::Sender<Result<codex_core::StartIfIdleSubmission, String>>,
    },
}
#[derive(Default)]
pub(crate) struct State {
    completed: Option<SuspendedExec>,
    bootstrap: bootstrap::State,
}
impl State {
    async fn suspend(
        &mut self,
        manager: &ThreadManager,
        admission: &crate::turn_admission::TurnAdmission,
        thread: ThreadId,
        turn: &str,
        configuration: &ExecConfiguration,
    ) -> Result<SuspendedExec, String> {
        if let Some(completed) = &self.completed {
            return if completed.thread() == thread && completed.turn() == turn {
                Ok(completed.clone())
            } else {
                Err("exec runtime was suspended for a different origin".into())
            };
        }
        let completed = suspend(manager, admission, thread, turn, configuration).await?;
        self.completed = Some(completed.clone());
        Ok(completed)
    }
}
impl Request {
    pub(crate) fn reject(self, reason: &str) {
        match self {
            Self::BootstrapForest { reply, .. } => {
                let _ = reply.send(Err(reason.into()));
            }
            Self::ActivateForest { reply, .. } => {
                let _ = reply.send(Err(reason.into()));
            }
            Self::Bootstrap { reply, .. } => {
                let _ = reply.send(Err(reason.into()));
            }
            Self::WatchQuota { reply, .. } => {
                let _ = reply.send(Err(reason.into()));
            }
            Self::Suspend { reply, .. } => {
                let _ = reply.send(Err(reason.into()));
            }
            Self::ActivateIdleTree { reply, .. } => {
                let _ = reply.send(Err(reason.into()));
            }
            Self::ActivateTree { reply, .. } => {
                let _ = reply.send(Err(reason.into()));
            }
        }
    }
}
pub(crate) async fn process(
    processor: &MessageProcessor,
    state: &mut State,
    request: Request,
    connection: crate::outgoing_message::ConnectionId,
) {
    // Queued operations are owned by the processor, even if a caller disconnects.
    match request {
        Request::BootstrapForest { request, reply } => {
            let result = if state.completed.is_some() {
                Err("a suspended runtime cannot bootstrap a forest".into())
            } else {
                state
                    .bootstrap
                    .run_forest(processor, request, connection)
                    .await
            };
            let _ = reply.send(result);
        }
        Request::ActivateForest { primary, reply } => {
            let result = if state.completed.is_some() {
                Err("a suspended runtime cannot activate a forest".into())
            } else {
                state.bootstrap.activate_forest(primary).await
            };
            let _ = reply.send(result);
        }
        Request::Bootstrap { request, reply } => {
            let result = if state.completed.is_some() {
                Err("a suspended runtime cannot bootstrap execution".into())
            } else {
                state.bootstrap.run(processor, request, connection).await
            };
            let _ = reply.send(result);
        }
        Request::WatchQuota { thread, reply } => {
            let result = match processor
                .thread_processor
                .thread_manager
                .get_thread(thread)
                .await
            {
                Ok(thread) => thread
                    .watch_exec_quota()
                    .ok_or_else(|| "quota observation requires an embedded exec thread".to_owned()),
                Err(_) => Err("exec quota observation thread is unavailable".into()),
            };
            let _ = reply.send(result);
        }
        Request::Suspend {
            thread,
            turn,
            configuration,
            reply,
        } => {
            let result = state
                .suspend(
                    &processor.thread_processor.thread_manager,
                    &processor.turn_admission,
                    thread,
                    &turn,
                    &configuration,
                )
                .await;
            let _ = reply.send(result);
        }
        Request::ActivateIdleTree { thread, reply } => {
            let result = if state.completed.is_some() {
                Err("a suspended runtime cannot activate execution".into())
            } else {
                state.bootstrap.activate_idle_tree(thread).await
            };
            let _ = reply.send(result);
        }
        Request::ActivateTree {
            thread,
            turn,
            reply,
        } => {
            let result = if state.completed.is_some() {
                Err("a suspended runtime cannot activate execution".into())
            } else {
                state.bootstrap.activate_tree(thread, &turn).await
            };
            let _ = reply.send(result);
        }
    }
}
async fn suspend(
    manager: &ThreadManager,
    admission: &crate::turn_admission::TurnAdmission,
    thread_id: ThreadId,
    turn: &str,
    configuration: &ExecConfiguration,
) -> Result<SuspendedExec, String> {
    let _pause = admission
        .pause()
        .await
        .map_err(|_| "exec suspension could not fence new starts".to_owned())?;
    let thread = manager
        .get_thread(thread_id)
        .await
        .map_err(|_| "exec suspension thread is unavailable".to_owned())?;
    if thread
        .thread_extension_data()
        .get::<HeadlessExec>()
        .is_none()
        || thread
            .current_turn_extension_data::<HeadlessExec>(turn)
            .await
            .is_none()
    {
        return Err("exec suspension origin is no longer active in this runtime".into());
    }
    let plan = thread.prepare_exec_tree(turn, configuration).await?;
    admission.begin_drain();
    let captured = plan.suspend_and_capture().await?;
    let tree = codex_core::ExecTreeSnapshot::capture(&captured)?;
    let options = tree
        .threads()
        .first()
        .and_then(codex_core::ExecThreadSnapshot::recovery)
        .cloned()
        .ok_or("captured tree has no original root turn")?;
    Ok(SuspendedExec { tree, options })
}
#[cfg(test)]
#[path = "exec_recovery_admission_tests.rs"]
mod recovery_options_tests;
#[cfg(test)]
#[path = "exec_suspend_tests.rs"]
mod tests;
