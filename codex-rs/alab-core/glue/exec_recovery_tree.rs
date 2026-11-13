// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Restore and activate one captured tree under independent native admissions.

use super::ExecRecoveryRequest;
use super::State;
use codex_core::CodexThread;
use codex_core::ExecLaunchBarrier;
use codex_core::ExecRestoredChild;
use codex_core::ExecTreeSnapshot;
use codex_core::PreparedExecInputBatch;
use codex_core::StartIfIdleSubmission;
use codex_protocol::ThreadId;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::watch;

type AdmissionFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
type Admission = Arc<dyn Fn(ThreadId, Option<String>) -> AdmissionFuture + Send + Sync>;
type Outcome = Result<Option<StartIfIdleSubmission>, String>;

/// A destination tree with immutable runtime bindings and native admission.
/// Preparation does not start turns. Activation is owned and retryable even
/// when the caller waiting for its reply disconnects.
pub struct ExecRecoveryTree {
    root: Arc<CodexThread>,
    tree: ExecTreeSnapshot,
    children: Vec<Arc<ExecRestoredChild>>,
    idle_root: Option<Arc<codex_core::ExecIdleRestore>>,
    threads: HashMap<ThreadId, Arc<CodexThread>>,
    reviews: HashMap<ThreadId, codex_extension_api::ExtensionDataInit>,
    barrier: OnceLock<ExecLaunchBarrier>,
    admit: Admission,
    deadline: tokio::time::Instant,
    outcome: OnceLock<watch::Receiver<Option<Outcome>>>,
}

#[cfg(test)]
#[path = "exec_recovery_tree_tests.rs"]
mod tests;

impl ExecRecoveryTree {
    pub(crate) fn native_threads(&self) -> Vec<Arc<CodexThread>> {
        self.threads.values().cloned().collect()
    }

    /// The root already owns its frame from `inputs`; remaining frames are
    /// consumed parent-first into fresh children in that root's local runtime.
    #[cfg(test)]
    pub async fn prepare(
        root: Arc<CodexThread>,
        tree: ExecTreeSnapshot,
        inputs: PreparedExecInputBatch,
        deadline: tokio::time::Instant,
        admit: impl Fn(ThreadId, Option<String>) -> AdmissionFuture + Send + Sync + 'static,
    ) -> Result<Arc<Self>, String> {
        let prepared = Self::prepare_unbound(root, tree, inputs, deadline, admit).await?;
        Self::bind_forest(std::slice::from_ref(&prepared))?;
        Ok(prepared)
    }

    pub(crate) async fn prepare_unbound(
        root: Arc<CodexThread>,
        tree: ExecTreeSnapshot,
        mut inputs: PreparedExecInputBatch,
        deadline: tokio::time::Instant,
        admit: impl Fn(ThreadId, Option<String>) -> AdmissionFuture + Send + Sync + 'static,
    ) -> Result<Arc<Self>, String> {
        tree.validate(root.exec_thread_id(), tree.turn())?;
        let idle_root = if tree.turn().is_none() {
            Some(root.prepare_exec_idle_root_restore(inputs.take_idle(tree.root())?)?)
        } else {
            None
        };
        let mut threads = HashMap::from([(root.exec_thread_id(), Arc::clone(&root))]);
        let mut children = Vec::new();
        let mut reviews = HashMap::new();
        let prepared = async {
            if tokio::time::Instant::now() >= deadline {
                return Err("native tree activation deadline elapsed".into());
            }
            for node in tree.threads().iter().skip(1) {
                let parent = threads
                    .get(&node.parent().ok_or("restored member has no parent")?)
                    .ok_or("restored member's parent is not loaded")?;
                let (fresh, available_roots) = parent.exec_child_restore_bindings().await;
                let roots = node.capability_roots().to_vec();
                if !roots.iter().all(|root| available_roots.contains(root)) {
                    return Err("restored child capabilities exceed its fresh parent".into());
                }
                let child = parent
                    .prepare_exec_child_restore(&tree, node.thread(), fresh, roots, &mut inputs)
                    .await?
                    .start()
                    .await?;
                threads.insert(node.thread(), Arc::clone(child.thread()));
                children.push(child);
            }
            if !inputs.is_empty() {
                return Err("tree restoration left unclaimed input frames".into());
            }
            for node in tree.threads() {
                if let Some(review) = node.review() {
                    let parent = threads
                        .get(&node.thread())
                        .ok_or("review parent is not loaded")?;
                    let child = threads
                        .get(&review.delegate())
                        .ok_or("review delegate is not loaded")?;
                    reviews.insert(node.thread(), review.prepare_recovery(parent, child).await?);
                }
            }
            Ok::<(), String>(())
        }
        .await;
        match prepared {
            Ok(()) => {}
            Err(reason) => {
                let _ = root.shutdown_exec_tree().await;
                return Err(reason);
            }
        }
        Ok(Arc::new(Self {
            root,
            tree,
            children,
            idle_root,
            threads,
            reviews,
            barrier: OnceLock::new(),
            admit: Arc::new(admit),
            deadline,
            outcome: OnceLock::new(),
        }))
    }

    pub(crate) fn bind_forest(trees: &[Arc<Self>]) -> Result<(), String> {
        if trees.iter().any(|tree| tree.barrier.get().is_some()) {
            return Err("tree launch is already bound".into());
        }
        let snapshots = trees.iter().map(|tree| &tree.tree).collect::<Vec<_>>();
        let loaded = trees
            .iter()
            .flat_map(|tree| tree.threads.values().cloned())
            .collect::<Vec<_>>();
        let barrier = ExecLaunchBarrier::prepare_forest(&snapshots, &loaded)?;
        for tree in trees {
            tree.barrier
                .set(barrier.clone())
                .map_err(|_| "tree launch was bound concurrently")?;
        }
        Ok(())
    }
    fn barrier(&self) -> Result<&ExecLaunchBarrier, String> {
        self.barrier
            .get()
            .ok_or_else(|| "tree launch is not bound".into())
    }
    pub(crate) async fn stop(&self, reason: &str) {
        if let Ok(barrier) = self.barrier() {
            barrier.abort(reason.into());
        }
        let _ = self.root.shutdown_exec_tree().await;
    }

    pub async fn activate(self: &Arc<Self>) -> Result<StartIfIdleSubmission, String> {
        if self.tree.turn().is_none() {
            return Err("idle root requires idle activation".into());
        }
        self.activate_once()
            .await?
            .ok_or_else(|| "active tree has no recovered root".into())
    }

    pub async fn activate_idle(self: &Arc<Self>) -> Result<(), String> {
        if self.tree.turn().is_some() {
            return Err("active root requires original-turn activation".into());
        }
        if self.activate_once().await?.is_some() {
            return Err("idle activation unexpectedly recovered a root turn".into());
        }
        Ok(())
    }

    async fn activate_once(self: &Arc<Self>) -> Outcome {
        let owner = Arc::clone(self);
        let mut outcome = self
            .outcome
            .get_or_init(move || {
                let (send, receive) = watch::channel(None);
                tokio::spawn(async move {
                    let result = tokio::time::timeout_at(owner.deadline, owner.run())
                        .await
                        .unwrap_or_else(|_| Err("native tree activation deadline elapsed".into()));
                    if let Err(reason) = &result {
                        owner.stop(reason).await;
                    }
                    send.send_replace(Some(result));
                });
                receive
            })
            .clone();
        loop {
            if let Some(result) = outcome.borrow_and_update().clone() {
                return result;
            }
            outcome
                .changed()
                .await
                .map_err(|_| "tree activation ended without recording its result".to_owned())?;
        }
    }

    async fn run(&self) -> Outcome {
        let result = self.install().await?;
        self.barrier()?.wait_ready().await?;
        self.barrier()?.release()?;
        self.publish_idle().await?;
        Ok(result)
    }

    pub(crate) async fn install(&self) -> Outcome {
        self.barrier()?;
        // Idle mail must precede any model work or new inter-agent delivery.
        if let Some(root) = &self.idle_root {
            let admit = Arc::clone(&self.admit);
            let deadline = self.deadline;
            root.restore(move |thread| {
                Box::pin(async move {
                    tokio::time::timeout_at(deadline, admit(thread, None))
                        .await
                        .map_err(|_| "native idle-root admission deadline elapsed".to_owned())?
                })
            })
            .await?;
        }
        for child in &self.children {
            if child.recovery().is_none() {
                let admit = Arc::clone(&self.admit);
                let deadline = self.deadline;
                child
                    .restore_idle_inputs(move |thread| {
                        Box::pin(async move {
                            tokio::time::timeout_at(deadline, admit(thread, None))
                                .await
                                .map_err(|_| "native idle admission deadline elapsed".to_owned())?
                        })
                    })
                    .await?;
            }
        }
        let mut root_result = None;
        for node in self.tree.threads() {
            let Some(options) = node.recovery() else {
                continue;
            };
            let thread = self
                .threads
                .get(&node.thread())
                .ok_or("tree member disappeared")?;
            let admit = Arc::clone(&self.admit);
            let id = node.thread();
            let deadline = self.deadline;
            let mut request = ExecRecoveryRequest::new_for_launch(
                options.clone(),
                self.barrier()?.permit(id)?,
                move |turn| {
                    let admit = Arc::clone(&admit);
                    Box::pin(async move {
                        tokio::time::timeout_at(deadline, admit(id, Some(turn)))
                            .await
                            .map_err(|_| "native turn admission deadline elapsed".to_owned())?
                    })
                },
            )?;
            request.review = self.reviews.get(&id).cloned();
            let result = State::default()
                .run_loaded(Arc::clone(thread), request)
                .await?;
            if !matches!(result, StartIfIdleSubmission::Started { .. }) {
                return Err("original tree member was not started".into());
            }
            if id == self.tree.root() {
                root_result = Some(result);
            }
        }
        Ok(root_result)
    }

    pub(crate) async fn ready(&self) -> Result<(), String> {
        self.barrier()?.wait_ready().await
    }
    pub(crate) fn release(&self) -> Result<(), String> {
        self.barrier()?.release()
    }
    pub(crate) async fn publish_idle(&self) -> Result<(), String> {
        for child in &self.children {
            if child.recovery().is_none() {
                child.activate_idle_inputs().await?;
            }
            child.release_residency_pin();
        }
        if let Some(root) = &self.idle_root {
            root.activate().await?;
        }
        Ok(())
    }
}
