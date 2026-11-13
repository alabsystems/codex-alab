// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Prepare an authenticated tree capsule without starting its model work.
use super::MessageProcessor;
use super::recovery::ExecRecoveryTree;
use crate::outgoing_message::ConnectionId;
use codex_core::StartThreadOptions;
use codex_core::config::Config;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionConfiguredEvent;
use codex_protocol::protocol::ThreadHistoryMode;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

type AdmissionFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
type Admission = Arc<dyn Fn(ThreadId, Option<String>) -> AdmissionFuture + Send + Sync>;
pub type BootstrappedExec = (ThreadHistoryMode, SessionConfiguredEvent);

#[derive(Clone)]
pub struct FreshRootStartup {
    config: Arc<Config>,
    exec: bool,
    goal_hold: Option<codex_alab_core::upgrade_startup_hold::Fence>,
}
impl FreshRootStartup {
    /// Destination-owned startup choices, never deserialized source authority.
    /// Actual SDK constructor and model admission remain independently required.
    pub fn interactive(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            exec: false,
            goal_hold: None,
        }
    }
    pub fn exec(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            exec: true,
            goal_hold: None,
        }
    }
}

#[path = "exec_bootstrap_forest.rs"]
mod forest;
pub use forest::BootstrappedForest;
pub use forest::ForestBootstrapRequest;

/// One immutable startup and native admission attempt. Clones reconcile replies.
#[derive(Clone)]
pub struct ExecBootstrapRequest {
    config: Arc<Config>,
    tree: Arc<codex_core::ExecTreeSnapshot>,
    admit: Admission,
    deadline: tokio::time::Instant,
    attempted: Arc<AtomicBool>,
}
impl ExecBootstrapRequest {
    pub fn new(
        config: Config,
        tree: codex_core::ExecTreeSnapshot,
        deadline: tokio::time::Instant,
        admit: impl Fn(ThreadId, Option<String>) -> AdmissionFuture + Send + Sync + 'static,
    ) -> Self {
        Self {
            config: Arc::new(config),
            tree: Arc::new(tree),
            admit: Arc::new(admit),
            deadline,
            attempted: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    accepted: Option<(
        ExecBootstrapRequest,
        BootstrappedExec,
        Arc<ExecRecoveryTree>,
    )>,
    forest: Option<forest::Accepted>,
}
impl State {
    pub(super) async fn activate_tree(
        &self,
        thread: ThreadId,
        turn: &str,
    ) -> Result<codex_core::StartIfIdleSubmission, String> {
        let (request, _, tree) = self
            .accepted
            .as_ref()
            .ok_or("exec tree has not been bootstrapped")?;
        if request.tree.root() != thread || request.tree.turn() != Some(turn) {
            return Err("exec activation changed the bootstrapped origin".into());
        }
        tree.activate().await
    }

    pub(super) async fn activate_idle_tree(&self, thread: ThreadId) -> Result<(), String> {
        let (request, _, tree) = self
            .accepted
            .as_ref()
            .ok_or("exec tree has not been bootstrapped")?;
        if request.tree.root() != thread || request.tree.turn().is_some() {
            return Err("idle activation changed the bootstrapped origin".into());
        }
        tree.activate_idle().await
    }

    pub(super) async fn run(
        &mut self,
        processor: &MessageProcessor,
        request: ExecBootstrapRequest,
        connection: ConnectionId,
    ) -> Result<BootstrappedExec, String> {
        if let Some((accepted, result, _)) = &self.accepted {
            return if Arc::ptr_eq(&accepted.attempted, &request.attempted) {
                Ok(result.clone())
            } else {
                Err("exec runtime already accepted another bootstrap".into())
            };
        }
        if request.attempted.swap(true, Ordering::AcqRel) {
            return Err("exec bootstrap requires a fresh attempt".into());
        }
        if self.forest.is_some() {
            return Err("runtime already accepted a forest bootstrap".into());
        }
        let tree = &request.tree;
        let inputs = match tree.turn() {
            Some(turn) => tree.inputs().clone().prepare(tree.root(), turn)?,
            None => tree.inputs().clone().prepare_idle(tree.root())?,
        };
        let startup = FreshRootStartup {
            config: request.config.clone(),
            exec: true,
            goal_hold: None,
        };
        let (result, prepared) = prepare_root(
            processor,
            &startup,
            tree,
            inputs,
            request.deadline,
            request.admit.clone(),
        )
        .await?;
        if let Err(error) = ExecRecoveryTree::bind_forest(std::slice::from_ref(&prepared)) {
            prepared.stop(&error).await;
            return Err(error);
        }
        processor
            .try_attach_thread_listener(tree.root(), vec![connection])
            .await;
        self.accepted = Some((request, result.clone(), prepared));
        Ok(result)
    }
}

async fn prepare_root(
    processor: &MessageProcessor,
    startup: &FreshRootStartup,
    tree: &codex_core::ExecTreeSnapshot,
    mut inputs: codex_core::PreparedExecInputBatch,
    deadline: tokio::time::Instant,
    admit: Admission,
) -> Result<(BootstrappedExec, Arc<ExecRecoveryTree>), String> {
    tree.validate(tree.root(), tree.turn())?;
    let node = tree.threads().first().ok_or("exec tree has no root")?;
    let manager = &processor.thread_processor.thread_manager;
    if manager.get_thread(tree.root()).await.is_ok() {
        return Err("exec bootstrap cannot replace an existing runtime".into());
    }
    // Restore invocation preferences under this root's freshly loaded policy
    // before creating Core services, SDK clients or the initial model settings.
    let restored = node
        .configuration()
        .restore(startup.config.as_ref())
        .await
        .map_err(|error| error.to_string())?;
    let startup = FreshRootStartup {
        config: Arc::new(restored),
        exec: startup.exec,
        goal_hold: startup.goal_hold.clone(),
    };
    let mut options = if let Some(history) = node.ephemeral_history() {
        if !startup.config.ephemeral {
            return Err("exec bootstrap changed ephemeral mode".into());
        }
        let options = StartThreadOptions::new(startup.config.as_ref().clone());
        match tree.turn() {
            Some(turn) => history.clone().prepare_start(options, tree.root(), turn)?,
            None if startup.exec => history.clone().prepare_idle_start(options, tree.root())?,
            None => history
                .clone()
                .prepare_idle_runtime_start(options, tree.root())?,
        }
    } else {
        if startup.config.ephemeral {
            return Err("persisted tree cannot load ephemeral history".into());
        }
        let mut options = StartThreadOptions::new(startup.config.as_ref().clone());
        options.initial_history = processor
            .thread_processor
            .alab_exec_history(tree.root())
            .await?;
        options
    };
    if startup.exec {
        options
            .thread_extension_init
            .insert(codex_alab_core::quota::HeadlessExec);
        options
            .turn_extension_init
            .insert(codex_alab_core::quota::HeadlessExec);
    } else {
        if tree.turn().is_some() {
            return Err("interactive forest startup requires an idle root".into());
        }
        options.session_source = Some(codex_protocol::protocol::SessionSource::Cli);
    }
    node.restrict_root_capabilities(&mut options.thread_extension_init);
    node.stage_helper_restore(&mut options.thread_extension_init);
    if let Some(hold) = &startup.goal_hold {
        options.thread_extension_init.insert(hold.clone());
    }
    options.dynamic_tools = node.dynamic_tools().to_vec();
    let options = match tree.turn() {
        Some(turn) => inputs.prepare_start(options, tree.root(), turn)?,
        None => options,
    };
    let _admission = processor
        .turn_admission
        .admit()
        .map_err(|_| "exec bootstrap start admission is unavailable".to_owned())?;
    let started = manager
        .start_thread(options)
        .await
        .map_err(|_| "exec bootstrap failed".to_owned())?;
    let prepared = async {
        if started.thread_id != tree.root() {
            return Err("exec bootstrap changed the original thread".into());
        }
        let startup = started.thread.thread_settings_snapshot().await;
        let (fresh, roots) = started.thread.exec_child_restore_bindings().await;
        let settings = node.restore_settings(&fresh, &roots)?;
        started
            .thread
            .restore_thread_settings(settings)
            .await
            .map_err(|e| e.to_string())?;
        started
            .thread
            .finish_exec_snapshot_restore(node, &startup)
            .await?;
        let admit = Arc::clone(&admit);
        ExecRecoveryTree::prepare_unbound(
            Arc::clone(&started.thread),
            tree.clone(),
            inputs,
            deadline,
            move |thread, turn| admit(thread, turn),
        )
        .await
    }
    .await;
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = started.thread.shutdown_exec_tree().await;
            return Err(error);
        }
    };
    let current = started.thread.config_snapshot().await;
    let mut configured = started.session_configured;
    configured.cwd = current.cwd().clone();
    configured.model = current.model;
    configured.model_provider_id = current.model_provider_id;
    configured.service_tier = current.service_tier;
    configured.approval_policy = current.approval_policy;
    configured.approvals_reviewer = current.approvals_reviewer;
    configured.permission_profile = current.permission_profile;
    configured.active_permission_profile = current.active_permission_profile;
    configured.reasoning_effort = current.reasoning_effort;
    configured.forked_from_id = current.forked_from_thread_id;
    configured.parent_thread_id = current.parent_thread_id;
    configured.thread_source = current.thread_source;
    let result = (current.history_mode, configured);
    Ok((result, prepared))
}
