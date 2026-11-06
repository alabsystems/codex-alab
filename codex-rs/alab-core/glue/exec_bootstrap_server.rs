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
        if request.tree.root() != thread || request.tree.turn() != turn {
            return Err("exec activation changed the bootstrapped origin".into());
        }
        tree.activate().await
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
        let tree = &request.tree;
        tree.validate(tree.root(), tree.turn())?;
        let node = tree.threads().first().ok_or("exec tree has no root")?;
        let manager = &processor.thread_processor.thread_manager;
        if manager.get_thread(tree.root()).await.is_ok() {
            return Err("exec bootstrap cannot replace an existing runtime".into());
        }
        let mut options = if let Some(history) = node.ephemeral_history() {
            if !request.config.ephemeral {
                return Err("exec bootstrap changed ephemeral mode".into());
            }
            history.clone().prepare_start(
                StartThreadOptions::new(request.config.as_ref().clone()),
                tree.root(),
                tree.turn(),
            )?
        } else {
            if request.config.ephemeral {
                return Err("persisted tree cannot load ephemeral history".into());
            }
            let mut options = StartThreadOptions::new(request.config.as_ref().clone());
            options.initial_history = processor
                .thread_processor
                .alab_exec_history(tree.root())
                .await?;
            options
                .thread_extension_init
                .insert(codex_alab_core::quota::HeadlessExec);
            options
                .turn_extension_init
                .insert(codex_alab_core::quota::HeadlessExec);
            options
        };
        options.dynamic_tools = node.dynamic_tools().to_vec();
        let mut inputs = tree.inputs().clone().prepare(tree.root(), tree.turn())?;
        let options = inputs.prepare_start(options, tree.root(), tree.turn())?;
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
                .verify_exec_restored_snapshot(node, &startup)
                .await?;
            let admit = Arc::clone(&request.admit);
            ExecRecoveryTree::prepare(
                Arc::clone(&started.thread),
                tree.as_ref().clone(),
                inputs,
                request.deadline,
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
        processor
            .try_attach_thread_listener(started.thread_id, vec![connection])
            .await;
        let result = (
            started.thread.config_snapshot().await.history_mode,
            started.session_configured,
        );
        self.accepted = Some((request, result.clone(), prepared));
        Ok(result)
    }
}
