// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Destination forest bootstrap. Source data never supplies native authority.
use super::*;
use codex_core::DecodedUpgradeForest;
use codex_core::UpgradeForestSnapshot;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::OnceLock;

#[derive(Clone)]
pub struct BootstrappedForest {
    pub primary: ThreadId,
    pub roots: Vec<(ThreadId, BootstrappedExec)>,
    threads: Arc<HashMap<ThreadId, Arc<codex_core::CodexThread>>>,
}

impl BootstrappedForest {
    /// Read the actual destination Core rather than reconstructing display
    /// settings from source DATA or the primary startup configuration.
    pub async fn root_context(
        &self,
        root: ThreadId,
    ) -> Result<
        (
            codex_core::ThreadConfigSnapshot,
            Vec<codex_utils_path_uri::PathUri>,
        ),
        String,
    > {
        let thread = self
            .threads
            .get(&root)
            .ok_or("unknown restored forest root")?;
        Ok((
            thread.config_snapshot().await,
            thread.instruction_sources().await,
        ))
    }
}

#[derive(Clone)]
enum RootPolicy {
    Fresh(FreshRootStartup),
    Reload(Box<codex_core::config::ConfigOverrides>),
}

#[derive(Clone)]
pub struct ForestBootstrapRequest {
    primary: ThreadId,
    roots: Arc<HashMap<ThreadId, RootPolicy>>,
    inputs: Arc<Mutex<Option<DecodedUpgradeForest>>>,
    order: Arc<Vec<ThreadId>>,
    admit: Admission,
    activate: Arc<dyn Fn() -> AdmissionFuture + Send + Sync>,
    deadline: tokio::time::Instant,
    attempted: Arc<AtomicBool>,
    goal_hold: Arc<codex_alab_core::upgrade_startup_hold::Owner>,
    goals: Option<Arc<codex_alab_autopilot::upgrade_goals::GoalForestState>>,
}
impl ForestBootstrapRequest {
    /// The inventory and startup policies come from the fresh native host.
    /// This constructor validates data; it grants no SDK/model admission.
    pub fn new(
        primary: ThreadId,
        expected_members: &[ThreadId],
        snapshot: UpgradeForestSnapshot,
        roots: Vec<(ThreadId, FreshRootStartup)>,
        deadline: tokio::time::Instant,
        admit: impl Fn(ThreadId, Option<String>) -> AdmissionFuture + Send + Sync + 'static,
        activate: impl Fn() -> AdmissionFuture + Send + Sync + 'static,
    ) -> Result<Self, String> {
        Self::from_policies(
            primary,
            expected_members,
            snapshot,
            roots
                .into_iter()
                .map(|(id, policy)| (id, RootPolicy::Fresh(policy)))
                .collect(),
            deadline,
            Arc::new(admit),
            Arc::new(activate),
        )
    }
    fn from_policies(
        primary: ThreadId,
        expected_members: &[ThreadId],
        snapshot: UpgradeForestSnapshot,
        roots: Vec<(ThreadId, RootPolicy)>,
        deadline: tokio::time::Instant,
        admit: Admission,
        activate: Arc<dyn Fn() -> AdmissionFuture + Send + Sync>,
    ) -> Result<Self, String> {
        let goal_hold = Arc::new(codex_alab_core::upgrade_startup_hold::Owner::default());
        let roots = roots
            .into_iter()
            .map(|(id, mut policy)| {
                if let RootPolicy::Fresh(startup) = &mut policy {
                    startup.goal_hold = Some(goal_hold.fence());
                }
                (id, policy)
            })
            .collect::<Vec<_>>();
        let order = snapshot.roots();
        let expected_roots = order.iter().copied().collect::<HashSet<_>>();
        let count = roots.len();
        let roots = roots.into_iter().collect::<HashMap<_, _>>();
        if roots.len() != count || roots.keys().copied().collect::<HashSet<_>>() != expected_roots {
            return Err("fresh forest policies differ from its original roots".into());
        }
        let decoded = snapshot.prepare(primary, expected_members)?;
        Ok(Self {
            primary,
            order: Arc::new(order),
            roots: Arc::new(roots),
            inputs: Arc::new(Mutex::new(Some(decoded))),
            admit,
            activate,
            deadline,
            attempted: Arc::new(AtomicBool::new(false)),
            goal_hold,
            goals: None,
        })
    }
    fn new_reloading(
        primary: ThreadId,
        expected_members: &[ThreadId],
        snapshot: UpgradeForestSnapshot,
        deadline: tokio::time::Instant,
        admit: impl Fn(ThreadId, Option<String>) -> AdmissionFuture + Send + Sync + 'static,
        activate: impl Fn() -> AdmissionFuture + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let roots = snapshot
            .startup_requests()?
            .into_iter()
            .map(|(id, request)| (id, RootPolicy::Reload(Box::new(request))))
            .collect();
        Self::from_policies(
            primary,
            expected_members,
            snapshot,
            roots,
            deadline,
            Arc::new(admit),
            Arc::new(activate),
        )
    }
    /// Decode one complete goal forest as data before obtaining fresh policies.
    pub fn from_native_bytes(
        primary: ThreadId,
        bytes: &[u8],
        continuation: Arc<dyn codex_alab_core::runtime_upgrade::Continuation>,
        deadline: tokio::time::Instant,
    ) -> Result<Self, String> {
        let goals = codex_alab_autopilot::upgrade_goals::GoalForestState::decode(bytes, primary)?;
        Self::from_native(primary, Arc::new(goals), continuation, deadline)
    }

    /// Restore data under fresh application policies and the native connection
    /// issued to this process's real SDK constructor. No source policy is a grant.
    pub fn from_native(
        primary: ThreadId,
        goals: Arc<codex_alab_autopilot::upgrade_goals::GoalForestState>,
        continuation: Arc<dyn codex_alab_core::runtime_upgrade::Continuation>,
        deadline: tokio::time::Instant,
    ) -> Result<Self, String> {
        goals.validate(primary)?;
        let members = goals
            .goals
            .iter()
            .map(|goal| goal.thread)
            .collect::<Vec<_>>();
        let check = continuation.clone();
        let mut request = Self::new_reloading(
            primary,
            &members,
            (*goals.forest).clone(),
            deadline,
            move |_, _| {
                let check = check.clone();
                Box::pin(async move { check.check().await.map_err(|error| error.to_string()) })
            },
            move || {
                let continuation = continuation.clone();
                Box::pin(async move {
                    continuation
                        .activate()
                        .await
                        .map_err(|error| error.to_string())
                })
            },
        )?;
        request.goals = Some(goals);
        Ok(request)
    }
    /// Construct from one complete data capsule, so its input forest and goal
    /// rows cannot be mixed with another source capture of the same thread IDs.
    pub fn new_with_goals(
        primary: ThreadId,
        expected_members: &[ThreadId],
        goals: Arc<codex_alab_autopilot::upgrade_goals::GoalForestState>,
        roots: Vec<(ThreadId, FreshRootStartup)>,
        deadline: tokio::time::Instant,
        admit: impl Fn(ThreadId, Option<String>) -> AdmissionFuture + Send + Sync + 'static,
        activate: impl Fn() -> AdmissionFuture + Send + Sync + 'static,
    ) -> Result<Self, String> {
        goals.validate(primary)?;
        let mut request = Self::new(
            primary,
            expected_members,
            (*goals.forest).clone(),
            roots,
            deadline,
            admit,
            activate,
        )?;
        request.goals = Some(goals);
        Ok(request)
    }
}

pub(super) struct Accepted {
    request: ForestBootstrapRequest,
    result: BootstrappedForest,
    owner: Arc<RecoveryForest>,
}
struct RecoveryForest {
    trees: Vec<Arc<ExecRecoveryTree>>,
    deadline: tokio::time::Instant,
    outcome: OnceLock<tokio::sync::watch::Receiver<Option<Result<(), String>>>>,
    goal_hold: Arc<codex_alab_core::upgrade_startup_hold::Owner>,
    goals: Option<Arc<codex_alab_autopilot::upgrade_goals::GoalForestState>>,
    activate: Arc<dyn Fn() -> AdmissionFuture + Send + Sync>,
}
impl RecoveryForest {
    async fn activate(self: &Arc<Self>) -> Result<(), String> {
        let owner = self.clone();
        let mut outcome = self
            .outcome
            .get_or_init(move || {
                let (send, receive) = tokio::sync::watch::channel(None);
                tokio::spawn(async move {
                    let result = tokio::time::timeout_at(owner.deadline, owner.run())
                        .await
                        .unwrap_or_else(|_| Err("forest activation deadline elapsed".into()));
                    if let Err(error) = &result {
                        for tree in &owner.trees {
                            tree.stop(error).await;
                        }
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
                .map_err(|_| "forest activation ended without a result".to_owned())?;
        }
    }
    async fn run(&self) -> Result<(), String> {
        // Every tree shares the same unreleased barrier. No first-root release
        // or pending mailbox notification can run work before the last admission.
        for tree in &self.trees {
            if tree.install().await?.is_some() {
                return Err("idle forest recovered an active root".into());
            }
        }
        let first = self.trees.first().ok_or("forest has no prepared root")?;
        first.ready().await?;
        let threads = self
            .trees
            .iter()
            .flat_map(|tree| tree.native_threads())
            .collect::<Vec<_>>();
        codex_alab_autopilot::upgrade_destination::restore_goals(
            &threads,
            &self.goal_hold.fence(),
            self.goals.as_deref(),
        )
        .await?;
        // The source data and every restored goal/clock are installed before
        // the native host may transfer ownership and authorize continuation.
        // Its denial keeps the shared hold closed and stops every restored tree.
        (self.activate)().await?;
        self.goal_hold.release_if(|| first.release())?;
        for tree in &self.trees {
            tree.publish_idle().await?;
        }
        Ok(())
    }
}

impl State {
    pub(crate) async fn run_forest(
        &mut self,
        processor: &MessageProcessor,
        request: ForestBootstrapRequest,
        connection: ConnectionId,
    ) -> Result<BootstrappedForest, String> {
        if let Some(accepted) = &self.forest {
            return if Arc::ptr_eq(&accepted.request.attempted, &request.attempted) {
                Ok(accepted.result.clone())
            } else {
                Err("runtime already accepted another forest bootstrap".into())
            };
        }
        if self.accepted.is_some() || request.attempted.swap(true, Ordering::AcqRel) {
            return Err("forest bootstrap requires a fresh runtime and attempt".into());
        }
        if !processor
            .thread_processor
            .thread_manager
            .list_thread_ids()
            .await
            .is_empty()
        {
            return Err("forest bootstrap cannot replace existing loaded runtimes".into());
        }
        // Finish every fresh policy load before creating any restored Core.
        // A bad secondary project cannot leave an already-started primary.
        let mut policies = HashMap::new();
        for id in request.order.iter() {
            let policy = request
                .roots
                .get(id)
                .ok_or("fresh forest root policy is missing")?;
            let mut startup = match policy {
                RootPolicy::Fresh(startup) => startup.clone(),
                RootPolicy::Reload(overrides) => FreshRootStartup::interactive(
                    tokio::time::timeout_at(
                        request.deadline,
                        processor
                            .thread_processor
                            .alab_upgrade_config(overrides.as_ref().clone()),
                    )
                    .await
                    .map_err(|_| "fresh forest policy deadline elapsed")??,
                ),
            };
            startup.goal_hold = Some(request.goal_hold.fence());
            policies.insert(*id, startup);
        }
        let mut decoded = request
            .inputs
            .lock()
            .map_err(|_| "forest data custody is unavailable")?
            .take()
            .ok_or("forest data was already taken")?;
        let mut trees = Vec::new();
        let mut roots = Vec::new();
        let prepared = async {
            // Primary first, then stable native root order. Data remains globally
            // decoded; prepare_root must never reconstruct a per-tree input table.
            for &id in request.order.iter() {
                if tokio::time::Instant::now() >= request.deadline {
                    return Err("forest bootstrap deadline elapsed".into());
                }
                let (tree, inputs) = decoded.take_tree(id)?;
                let startup = policies
                    .get(&id)
                    .ok_or("fresh forest root policy is missing")?;
                let (configured, loaded) = prepare_root(
                    processor,
                    startup,
                    &tree,
                    inputs,
                    request.deadline,
                    request.admit.clone(),
                )
                .await?;
                trees.push(loaded);
                roots.push((id, configured));
            }
            if !decoded.is_empty() {
                return Err("forest bootstrap left unclaimed data".into());
            }
            ExecRecoveryTree::bind_forest(&trees)
        }
        .await;
        if let Err(error) = prepared {
            for tree in &trees {
                tree.stop(&error).await;
            }
            return Err(error);
        }
        for (id, _) in &roots {
            processor
                .try_attach_thread_listener(*id, vec![connection])
                .await;
        }
        let threads = trees
            .iter()
            .flat_map(|tree| tree.native_threads())
            .filter(|thread| policies.contains_key(&thread.exec_thread_id()))
            .map(|thread| (thread.exec_thread_id(), thread))
            .collect();
        let result = BootstrappedForest {
            primary: request.primary,
            roots,
            threads: Arc::new(threads),
        };
        let owner = Arc::new(RecoveryForest {
            trees,
            deadline: request.deadline,
            outcome: OnceLock::new(),
            goal_hold: request.goal_hold.clone(),
            goals: request.goals.clone(),
            activate: request.activate.clone(),
        });
        self.forest = Some(Accepted {
            request,
            result: result.clone(),
            owner,
        });
        Ok(result)
    }

    pub(crate) async fn activate_forest(&self, primary: ThreadId) -> Result<(), String> {
        let accepted = self
            .forest
            .as_ref()
            .ok_or("forest has not been bootstrapped")?;
        if accepted.request.primary != primary {
            return Err("forest activation changed its primary".into());
        }
        accepted.owner.activate().await
    }
}

#[cfg(test)]
#[path = "exec_bootstrap_forest_tests.rs"]
mod tests;
