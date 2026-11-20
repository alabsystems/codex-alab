// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One manager may host several independently owned result writers.
use super::*;

pub(crate) struct ForestResults(Vec<Tree>);

struct Tree {
    root: Arc<CodexThread>,
    members: Vec<Arc<CodexThread>>,
    source: Arc<ExecResultCustody>,
}

impl Tree {
    fn runtime(&self) -> &LocalAgentRuntime {
        &self.root.session.services.local_agent_runtime
    }
}

impl ForestResults {
    pub(crate) fn prepare(
        roots: &[Arc<CodexThread>],
        inventory: &[Arc<CodexThread>],
    ) -> Result<Self, String> {
        let mut groups = Vec::with_capacity(roots.len());
        for root in roots {
            let members = inventory
                .iter()
                .filter(|member| root.shares_exec_tree(member))
                .cloned()
                .collect::<Vec<_>>();
            let parents = ExecResultCustody::parents(&members);
            groups.push((root, members, parents));
        }
        if !groups.iter().any(|(root, _, parents)| {
            !parents.is_empty()
                || root
                    .session
                    .services
                    .local_agent_runtime
                    .exec_result_counter()
                    .load(Ordering::Acquire)
                    != 0
        }) {
            return Ok(Self(Vec::new()));
        }
        let mut trees = Vec::with_capacity(roots.len());
        for (root, members, parents) in groups {
            let source = ExecResultCustody::prepare(
                &root.session.services.local_agent_runtime,
                &members,
                parents,
            )?;
            trees.push(Tree {
                root: Arc::clone(root),
                members,
                source,
            });
        }
        Ok(Self(trees))
    }

    pub(crate) fn commit_if(
        &self,
        manager: &mut crate::UpgradeManagerBarrier,
        decide: impl FnOnce() -> bool,
    ) -> Result<bool, String> {
        // Check all original queues, controllers and outstanding native writer
        // decisions before invoking any irreversible host or manager transition.
        let mut queues = Vec::with_capacity(self.0.len());
        let mut controllers = Vec::with_capacity(self.0.len());
        let mut dispositions = Vec::with_capacity(self.0.len());
        for tree in &self.0 {
            let queue = Queues::lock(&tree.members)?;
            if queue.turn.iter().any(Option::is_some) {
                return Err("a forest result owner is no longer idle".into());
            }
            let controller = tree.runtime().lock_exec_result_controller()?;
            if !controller.result_routes_belong_to(|id| {
                tree.members
                    .iter()
                    .any(|member| member.exec_thread_id() == id)
            }) {
                return Err(
                    "queued forest result requires its receiving member to be loaded".into(),
                );
            }
            tree.source
                .grant
                .recheck(queue.messages().chain(controller.messages()))?;
            let disposition = tree
                .runtime()
                .prepare_exec_result_shutdown()
                .ok_or("another shutdown already owns a forest result writer")?;
            queues.push(queue);
            controllers.push(controller);
            dispositions.push(disposition);
        }
        Ok(manager.commit_if(|| {
            if !decide() {
                return false;
            }
            for commit in dispositions {
                commit();
            }
            for ((tree, queue), controller) in self.0.iter().zip(&mut queues).zip(controllers) {
                tree.source.hold(&tree.root, &tree.members, queue);
                controller.seal();
            }
            true
        }))
    }

    pub(crate) async fn finish(&self) -> Result<(), String> {
        for tree in &self.0 {
            tree.source.finish(&tree.root, &tree.members).await?;
        }
        Ok(())
    }

    pub(crate) fn captured(&self) {
        for tree in &self.0 {
            tree.root
                .thread_extension_data()
                .remove::<ExecResultCustody>();
        }
    }
}
