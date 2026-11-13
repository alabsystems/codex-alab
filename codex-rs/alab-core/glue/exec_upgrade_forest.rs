// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Prepare every loaded tree before permanent manager commitment.
use super::super::ExecTreeSnapshot;
use super::super::PreparedExecInputBatch;
use super::super::tree_wire::ExecTreeRecords;
use super::*;
use crate::UpgradeManagerBarrier;
use serde::Deserialize;
use serde::Serialize;

struct Root {
    thread: Arc<CodexThread>,
    invocation: ExecConfiguration,
}

/// Core idle-state/data preparation only. Goal checkpoints, TUI fencing and
/// native execution admission must still join the final conditional decision.
pub struct PreparedUpgradeForest {
    barrier: UpgradeManagerBarrier,
    primary: ThreadId,
    roots: Vec<Root>,
    readers: Option<Arc<codex_alab_core::manager_catalog::CommittedUpgrade>>,
}

pub struct CommittedUpgradeForest {
    inventory: crate::CommittedUpgradeInventory,
    primary: ThreadId,
    roots: Vec<Root>,
    outcome: OnceLock<tokio::sync::watch::Receiver<Option<ForestResult>>>,
    readers: Option<Arc<codex_alab_core::manager_catalog::CommittedUpgrade>>,
}
type ForestResult = Result<Arc<UpgradeForestSnapshot>, String>;

/// One portable input table spans every tree, preserving publication identity
/// across roots. Records are data; none of this grants destination execution.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeForestSnapshot {
    version: u32,
    primary: ThreadId,
    trees: Vec<ExecTreeRecords>,
    inputs: ExecInputBatch,
}

pub struct DecodedUpgradeForest {
    trees: Vec<ExecTreeSnapshot>,
    inputs: PreparedExecInputBatch,
}

impl CodexThread {
    pub async fn prepare_upgrade_forest(
        &self,
        invocation: &ExecConfiguration,
    ) -> Result<PreparedUpgradeForest, String> {
        if self.session_source.is_non_root_agent() {
            return Err("upgrade forest requires its primary root".into());
        }
        let barrier = self
            .prepare_upgrade_manager()
            .await
            .map_err(|e| e.to_string())?;
        let primary = self.exec_thread_id();
        let prepare = async {
            let threads = barrier.threads();
            let mut roots = threads
                .iter()
                .filter(|thread| !thread.session_source.is_non_root_agent())
                .cloned()
                .collect::<Vec<_>>();
            let index = roots
                .iter()
                .position(|root| std::ptr::eq(root.as_ref(), self))
                .ok_or("forest primary is not registered")?;
            let first = roots.remove(index);
            roots.insert(0, first);
            let mut groups = Vec::new();
            let mut claimed = HashSet::new();
            for root in &roots {
                if !root.is_running() || root.config_snapshot().await.parent_thread_id.is_some() {
                    return Err("forest root is retired or has a parent".into());
                }
                let mut members = Vec::new();
                for thread in threads
                    .iter()
                    .filter(|thread| root.shares_exec_tree(thread))
                {
                    if !claimed.insert(thread.exec_thread_id()) {
                        return Err("forest member belongs to several roots".into());
                    }
                    let parent = thread.config_snapshot().await.parent_thread_id;
                    if thread
                        .session_source
                        .parent_thread_id()
                        .is_some_and(|id| Some(id) != parent)
                    {
                        return Err("forest member changed its parent identity".into());
                    }
                    members.push(Member {
                        thread: Arc::clone(thread),
                        parent,
                        active: None,
                        review: None,
                    });
                }
                groups.push(order_members(root.exec_thread_id(), members)?);
            }
            if claimed.len() != threads.len() {
                return Err("forest has an unloaded root or an unowned private member".into());
            }
            // Preflight storage and codecs while preparation remains revocable.
            // Final producer-stopped capture is a separate committed operation.
            for members in groups {
                let mut nodes = Vec::new();
                for member in members {
                    member.thread.prepare_upgrade_idle_inputs(&barrier).await?;
                    nodes.push(capture_idle_node(&member.thread, member.parent, invocation).await?);
                }
                ExecTreeSnapshot::preflight_idle_nodes(&nodes)?;
            }
            Ok::<_, String>(roots)
        };
        let roots = tokio::select! {
            biased;
            () = barrier.invalidated() => return Err("forest preparation was superseded".into()),
            result = prepare => result?,
        };
        if !barrier.valid() {
            return Err("forest preparation was superseded".into());
        }
        Ok(PreparedUpgradeForest {
            readers: None,
            barrier,
            primary,
            roots: roots
                .into_iter()
                .map(|thread| Root {
                    thread,
                    invocation: invocation.clone(),
                })
                .collect(),
        })
    }
}

impl PreparedUpgradeForest {
    pub fn valid(&self) -> bool {
        self.barrier.valid()
    }
    pub async fn invalidated(&self) {
        self.barrier.invalidated().await;
    }
    pub fn threads(&self) -> Vec<Arc<CodexThread>> {
        self.barrier.threads()
    }
    pub fn roots(&self) -> Vec<ThreadId> {
        self.roots
            .iter()
            .map(|root| root.thread.exec_thread_id())
            .collect()
    }
    /// Commit Core data custody only; no native reader retirement is implied.
    pub fn commit_if(&mut self, decide: impl FnOnce() -> bool) -> bool {
        self.barrier.commit_if(decide)
    }

    /// Join this exact manager inventory with the process's actual source-reader
    /// catalog at the final host decision. No destination SDK grant is created.
    pub fn commit_with_readers_if(
        &mut self,
        decide: impl FnOnce() -> bool,
    ) -> Result<bool, String> {
        if self.readers.is_some() {
            return Err("source readers already belong to this committed forest".into());
        }
        let mut readers = None;
        let mut refusal = None;
        let committed =
            self.barrier.commit_if(|| {
                match codex_alab_core::manager_catalog::global().commit_upgrade_if(decide) {
                    Ok(Some(owner)) => {
                        readers = Some(owner);
                        true
                    }
                    Ok(None) => false,
                    Err(error) => {
                        refusal = Some(format!("source reader census refused: {error:?}"));
                        false
                    }
                }
            });
        if let Some(error) = refusal {
            return Err(error);
        }
        self.readers = readers;
        Ok(committed)
    }

    pub fn into_committed(self) -> Result<Arc<CommittedUpgradeForest>, String> {
        Ok(Arc::new(CommittedUpgradeForest {
            inventory: self.barrier.into_committed().map_err(|e| e.to_string())?,
            primary: self.primary,
            roots: self.roots,
            readers: self.readers,
            outcome: OnceLock::new(),
        }))
    }
}

impl CommittedUpgradeForest {
    pub fn threads(&self) -> &[Arc<CodexThread>] {
        self.inventory.threads()
    }
    pub fn source_readers_committed(&self) -> bool {
        self.readers.is_some()
    }

    /// An owned operation joins all source producers even if its waiter leaves.
    /// No exec is authorized by a successful capture.
    pub async fn capture(self: &Arc<Self>) -> ForestResult {
        let owner = Arc::clone(self);
        let mut outcome = self
            .outcome
            .get_or_init(move || {
                let (send, receive) = tokio::sync::watch::channel(None);
                tokio::spawn(async move {
                    send.send_replace(Some(owner.capture_once().await.map(Arc::new)));
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
                .map_err(|_| "forest capture ended without a result".to_owned())?;
        }
    }

    async fn capture_once(&self) -> Result<UpgradeForestSnapshot, String> {
        for member in self.inventory.threads() {
            member.seal_upgrade_idle_inputs(&self.inventory).await?;
        }
        let mut failure = None;
        for root in &self.roots {
            if let Err(error) = root.thread.shutdown_exec_tree().await {
                failure.get_or_insert(error);
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        let mut records = Vec::new();
        let mut members = Vec::new();
        for root in &self.roots {
            let tree =
                ExecTreeSnapshot::capture_idle(&root.thread, &self.inventory, &root.invocation)
                    .await?;
            for node in tree.threads() {
                let member = self
                    .inventory
                    .threads()
                    .iter()
                    .find(|member| member.exec_thread_id() == node.thread())
                    .ok_or("forest lost an original member")?;
                members.push(member.as_ref());
            }
            records.push(tree.into_records());
        }
        let roots = self
            .roots
            .iter()
            .map(|root| root.thread.exec_thread_id())
            .collect::<Vec<_>>();
        let inputs = ExecInputBatch::capture_idle_forest(&roots, &members).await?;
        let forest = UpgradeForestSnapshot {
            version: 1,
            primary: self.primary,
            trees: records,
            inputs,
        };
        let bytes = serde_json::to_vec(&forest).map_err(|e| e.to_string())?;
        let forest: UpgradeForestSnapshot =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let ids = self
            .inventory
            .threads()
            .iter()
            .map(|member| member.exec_thread_id())
            .collect::<Vec<_>>();
        forest.validate(self.primary, &ids)?;
        if let Some(readers) = &self.readers {
            readers
                .drain()
                .await
                .map_err(|error| format!("source reader retirement refused: {error:?}"))?;
        }
        Ok(forest)
    }
}

impl UpgradeForestSnapshot {
    pub fn roots(&self) -> Vec<ThreadId> {
        self.trees.iter().map(ExecTreeRecords::root).collect()
    }
    /// Original location/workspace requests are data for fresh policy loading.
    /// Reading them does not decode or duplicate the global input table.
    pub fn startup_requests(
        &self,
    ) -> Result<Vec<(ThreadId, crate::config::ConfigOverrides)>, String> {
        self.trees
            .iter()
            .map(ExecTreeRecords::startup_request)
            .collect()
    }
    pub fn validate(&self, primary: ThreadId, expected: &[ThreadId]) -> Result<(), String> {
        self.clone().prepare(primary, expected).map(|_| ())
    }
    pub fn prepare(
        self,
        primary: ThreadId,
        expected: &[ThreadId],
    ) -> Result<DecodedUpgradeForest, String> {
        if self.version != 1
            || self.primary != primary
            || self.trees.first().map(ExecTreeRecords::root) != Some(primary)
        {
            return Err("forest snapshot changed its primary or version".into());
        }
        let roots = self.roots();
        let mut seen = HashSet::new();
        let mut trees = Vec::new();
        for record in self.trees {
            let ids = record.threads();
            if ids.iter().any(|id| !seen.insert(*id)) {
                return Err("forest repeats a member across trees".into());
            }
            let inputs = self.inputs.select_idle_tree(&ids)?;
            trees.push(record.with_inputs(inputs)?);
        }
        let wanted = expected.iter().copied().collect::<HashSet<_>>();
        if wanted.len() != expected.len() || seen != wanted {
            return Err("forest differs from its native member inventory".into());
        }
        let inputs = self.inputs.prepare_idle_forest(&roots, expected)?;
        Ok(DecodedUpgradeForest { trees, inputs })
    }
}

impl DecodedUpgradeForest {
    /// Take tree data and its globally decoded input together, exactly once.
    /// A destination must use these inputs rather than decoding a tree again.
    pub fn take_tree(
        &mut self,
        root: ThreadId,
    ) -> Result<(ExecTreeSnapshot, PreparedExecInputBatch), String> {
        let index = self
            .trees
            .iter()
            .position(|tree| tree.root() == root)
            .ok_or("forest root was absent or already taken")?;
        let ids = self.trees[index]
            .threads()
            .iter()
            .map(crate::ExecThreadSnapshot::thread)
            .collect::<Vec<_>>();
        let inputs = self.inputs.take_idle_tree(&ids)?;
        Ok((self.trees.remove(index), inputs))
    }
    pub fn is_empty(&self) -> bool {
        self.trees.is_empty() && self.inputs.is_empty()
    }
}
