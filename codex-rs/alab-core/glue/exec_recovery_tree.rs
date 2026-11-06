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
type Outcome = Result<StartIfIdleSubmission, String>;

/// A destination tree with immutable runtime bindings and native admission.
/// Preparation does not start turns. Activation is owned and retryable even
/// when the caller waiting for its reply disconnects.
pub struct ExecRecoveryTree {
    root: Arc<CodexThread>,
    tree: ExecTreeSnapshot,
    children: Vec<Arc<ExecRestoredChild>>,
    threads: HashMap<ThreadId, Arc<CodexThread>>,
    reviews: HashMap<ThreadId, codex_extension_api::ExtensionDataInit>,
    barrier: ExecLaunchBarrier,
    admit: Admission,
    deadline: tokio::time::Instant,
    outcome: OnceLock<watch::Receiver<Option<Outcome>>>,
}

#[cfg(test)]
#[path = "exec_recovery_tree_tests.rs"]
mod tests;

impl ExecRecoveryTree {
    /// The root already owns its frame from `inputs`; remaining frames are
    /// consumed parent-first into fresh children in that root's local runtime.
    pub async fn prepare(
        root: Arc<CodexThread>,
        tree: ExecTreeSnapshot,
        mut inputs: PreparedExecInputBatch,
        deadline: tokio::time::Instant,
        admit: impl Fn(ThreadId, Option<String>) -> AdmissionFuture + Send + Sync + 'static,
    ) -> Result<Arc<Self>, String> {
        tree.validate(root.exec_thread_id(), tree.turn())?;
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
            let loaded = threads.values().cloned().collect::<Vec<_>>();
            ExecLaunchBarrier::prepare(&tree, &loaded)
        }
        .await;
        let barrier = match prepared {
            Ok(barrier) => barrier,
            Err(reason) => {
                let _ = root.shutdown_exec_tree().await;
                return Err(reason);
            }
        };
        Ok(Arc::new(Self {
            root,
            tree,
            children,
            threads,
            reviews,
            barrier,
            admit: Arc::new(admit),
            deadline,
            outcome: OnceLock::new(),
        }))
    }

    pub async fn activate(self: &Arc<Self>) -> Outcome {
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
                        owner.barrier.abort(reason.clone());
                        let _ = owner.root.shutdown_exec_tree().await;
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
        // Idle mail must precede any model work or new inter-agent delivery.
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
                self.barrier.permit(id)?,
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
        self.barrier.wait_ready().await?;
        self.barrier.release()?;
        for child in &self.children {
            if child.recovery().is_none() {
                child.activate_idle_inputs()?;
            }
            child.release_residency_pin();
        }
        root_result.ok_or_else(|| "tree has no recovered root".into())
    }
}
