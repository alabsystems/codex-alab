// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source-side tree capture. These Core records are not a portable execution grant.
use super::ExecInputBatch;
use super::ExecPreparedChild;
use super::ExecRecoveryOptions;
use crate::CodexThread;
use crate::CodexThreadSettingsOverrides;
use crate::ExecTreeSeal;
use crate::config::ExecConfiguration;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::turn_input::SuspendTurnOutcome;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

type CaptureResult = Result<Arc<ExecCapturedTree>, String>;

/// A capture of a writer-closed source, not a portable snapshot or permission
/// to restore its host-owned settings. Transport and fresh admission are separate.
pub struct ExecCapturedTree {
    root: ThreadId,
    turn: String,
    threads: Vec<ExecCapturedThread>,
    inputs: ExecInputBatch,
}

pub struct ExecCapturedThread {
    thread: ThreadId,
    parent: Option<ThreadId>,
    source: SessionSource,
    root_capabilities_restricted: bool,
    configuration: ExecConfiguration,
    settings: CodexThreadSettingsOverrides,
    dynamic_tools: Vec<codex_protocol::dynamic_tools::DynamicToolSpec>,
    capability_roots: Vec<codex_protocol::capabilities::SelectedCapabilityRoot>,
    ephemeral_history: Option<super::ExecEphemeralHistory>,
    helper: Option<crate::agent::helper_daybreak::turn::alab_exec_helper::Snapshot>,
    recovery: Option<ExecRecoveryOptions>,
    review: Option<crate::ExecReviewSnapshot>,
    history: codex_thread_store::StoredThreadHistory,
}

impl ExecCapturedTree {
    pub fn root(&self) -> ThreadId {
        self.root
    }
    pub fn turn(&self) -> &str {
        &self.turn
    }
    pub fn threads(&self) -> &[ExecCapturedThread] {
        &self.threads
    }
    pub fn inputs(&self) -> &ExecInputBatch {
        &self.inputs
    }
}

impl ExecCapturedThread {
    pub fn thread(&self) -> ThreadId {
        self.thread
    }
    pub fn parent(&self) -> Option<ThreadId> {
        self.parent
    }
    pub fn source(&self) -> &SessionSource {
        &self.source
    }
    pub fn root_capabilities_restricted(&self) -> bool {
        self.root_capabilities_restricted
    }
    pub fn configuration(&self) -> &ExecConfiguration {
        &self.configuration
    }
    pub fn settings(&self) -> &CodexThreadSettingsOverrides {
        &self.settings
    }
    pub fn dynamic_tools(&self) -> &[codex_protocol::dynamic_tools::DynamicToolSpec] {
        &self.dynamic_tools
    }
    pub fn capability_roots(&self) -> &[codex_protocol::capabilities::SelectedCapabilityRoot] {
        &self.capability_roots
    }
    pub fn ephemeral_history(&self) -> Option<&super::ExecEphemeralHistory> {
        self.ephemeral_history.as_ref()
    }
    pub(crate) fn helper(
        &self,
    ) -> Option<&crate::agent::helper_daybreak::turn::alab_exec_helper::Snapshot> {
        self.helper.as_ref()
    }
    pub fn recovery(&self) -> Option<&ExecRecoveryOptions> {
        self.recovery.as_ref()
    }
    pub fn review(&self) -> Option<&crate::ExecReviewSnapshot> {
        self.review.as_ref()
    }
    /// Canonical latest-turn evidence; persistent model context is loaded separately.
    pub fn history(&self) -> &codex_thread_store::StoredThreadHistory {
        &self.history
    }
}

struct Member {
    thread: Arc<CodexThread>,
    parent: Option<ThreadId>,
    active: Option<ExecPreparedChild>,
    review: Option<crate::ExecFrozenReview>,
}

struct Plan {
    root: Arc<CodexThread>,
    turn: String,
    root_options: ExecRecoveryOptions,
    invocation: ExecConfiguration,
    seal: ExecTreeSeal,
    members: Vec<Member>,
}

/// Preflighted loaded members of exactly one live tree. Dropping an unstarted
/// plan reopens admission. Once started, an owned operation completes capture
/// even if a caller disconnects; retries reconcile the same result.
pub struct ExecPreparedTree {
    plan: Mutex<Option<Plan>>,
    outcome: OnceLock<tokio::sync::watch::Receiver<Option<CaptureResult>>>,
}

impl CodexThread {
    pub async fn prepare_exec_tree(
        &self,
        turn: &str,
        invocation: &ExecConfiguration,
    ) -> Result<ExecPreparedTree, String> {
        if self.session_source.is_non_root_agent() {
            return Err("tree capture requires the owning root".into());
        }
        let seal = self
            .seal_exec_tree_starts()
            .await
            .map_err(|e| e.to_string())?;
        let root_review = self.freeze_exec_review(&seal, turn).await?;
        let root_options = ExecRecoveryOptions::capture(self, turn)
            .await
            .ok_or("tree capture requires the original active exec root")?;
        self.validate_exec_input_handoff(turn).await?;
        let threads = seal.loaded_threads(self).await.map_err(|e| e.to_string())?;
        let root = threads
            .iter()
            .find(|thread| std::ptr::eq(thread.as_ref(), self))
            .cloned()
            .ok_or("tree capture root changed")?;
        let mut members = Vec::with_capacity(threads.len());
        let mut root_review = root_review;
        for thread in threads {
            let role = if Arc::ptr_eq(&thread, &root) {
                "root"
            } else {
                "child"
            };
            let snapshot = thread.config_snapshot().await;
            let parent = snapshot.parent_thread_id;
            // Validate configuration portability before stopping any member.
            capture_configuration(&thread, invocation).await?;
            let mut settings = thread.restorable_thread_settings().await;
            settings.turn_extension_init = None;
            super::tree_wire::preflight_settings(&settings)?;
            let review = if Arc::ptr_eq(&thread, &root) {
                root_review.take()
            } else {
                let turn = thread
                    .session
                    .active_turn
                    .lock()
                    .await
                    .as_ref()
                    .and_then(|active| active.task.as_ref())
                    .map(|task| task.turn_context.sub_id.clone());
                match turn {
                    Some(turn) => thread.freeze_exec_review(&seal, &turn).await?,
                    None => None,
                }
            };
            let active = if Arc::ptr_eq(&thread, &root) {
                None
            } else {
                if !thread.session_source.is_non_root_agent() {
                    return Err("tree capture found another root in its runtime".into());
                }
                let has_active = { thread.session.active_turn.lock().await.is_some() };
                if has_active {
                    Some(
                        thread
                            .prepare_exec_child(&seal)
                            .await
                            .map_err(|e| format!("exec child preparation: {e}"))?,
                    )
                } else {
                    thread.validate_exec_idle_inputs(&seal).await?;
                    if thread.session.live_thread().is_none() && !snapshot.ephemeral {
                        return Err("idle durable member has no live writer".into());
                    }
                    // An idle memory journal must already exist from startup.
                    // Materialization cannot synthesize missing evidence.
                    thread.ensure_rollout_materialized().await;
                    thread.flush_rollout().await.map_err(|e| e.to_string())?;
                    thread
                        .exec_recovery_history()
                        .await
                        .map_err(|e| format!("exec idle {role} preflight history: {e}"))?;
                    None
                }
            };
            thread.flush_rollout().await.map_err(|e| e.to_string())?;
            let evidence = thread
                .exec_recovery_history()
                .await
                .map_err(|e| format!("exec {role} preflight history: {e}"))?;
            super::ephemeral::HistoryEvidence::capture(&evidence)?.restore(evidence.thread_id)?;
            members.push(Member {
                thread,
                parent,
                active,
                review,
            });
        }
        let members = order_members(root.session.thread_id, members)?;
        for member in &members {
            if let Some(review) = &member.review {
                let delegate = review.delegate()?;
                if !members.iter().any(|candidate| {
                    candidate.thread.session.thread_id == delegate
                        && candidate.parent == Some(member.thread.session.thread_id)
                }) {
                    return Err("review delegate is absent from its owning tree".into());
                }
            }
        }
        Ok(ExecPreparedTree {
            plan: Mutex::new(Some(Plan {
                root,
                turn: turn.into(),
                root_options,
                invocation: invocation.clone(),
                seal,
                members,
            })),
            outcome: OnceLock::new(),
        })
    }
}

async fn capture_configuration(
    thread: &CodexThread,
    invocation: &ExecConfiguration,
) -> Result<ExecConfiguration, String> {
    let state = thread.session.state.lock().await;
    let current = &state.session_configuration;
    let mut config = (*current.original_config_do_not_use).clone();
    config.model = Some(current.step_settings.collaboration_mode.model().to_owned());
    config.model_provider = current.provider.info().clone();
    config.model_reasoning_effort = current.step_settings.collaboration_mode.reasoning_effort();
    config.model_reasoning_summary = current.step_settings.reasoning_summary;
    config.service_tier = current.step_settings.service_tier.clone();
    config.personality = current.step_settings.personality;
    config.base_instructions = Some(current.base_instructions.clone());
    config.base_instructions_provenance = state.base_instructions_provenance.clone();
    config.developer_instructions = current.developer_instructions.clone();
    config.cwd = current.legacy_fallback_cwd.clone();
    config.workspace_roots = current.runtime_workspace_roots.clone();
    config
        .permissions
        .approval_policy
        .set(current.step_settings.approval_policy.value())
        .map_err(|e| e.to_string())?;
    config.approvals_reviewer = current.step_settings.approvals_reviewer;
    config
        .permissions
        .set_permission_profile_from_session_snapshot(current.permission_profile_state.snapshot())
        .map_err(|e| e.to_string())?;
    config
        .permissions
        .set_workspace_roots(current.runtime_workspace_roots.clone());
    invocation.capture_child(&config).map_err(|e| e.to_string())
}

fn order_members(root: ThreadId, mut pending: Vec<Member>) -> Result<Vec<Member>, String> {
    let root_index = pending
        .iter()
        .position(|member| member.thread.session.thread_id == root)
        .ok_or("tree capture has no root")?;
    let root_member = pending.remove(root_index);
    let mut result = vec![root_member];
    let mut seen = HashSet::from([root]);
    while !pending.is_empty() {
        let index = pending
            .iter()
            .position(|member| member.parent.is_some_and(|parent| seen.contains(&parent)))
            .ok_or("loaded tree has a missing parent or a parent cycle")?;
        let member = pending.remove(index);
        if !seen.insert(member.thread.session.thread_id) {
            return Err("loaded tree repeats a thread identity".into());
        }
        result.push(member);
    }
    Ok(result)
}

impl ExecPreparedTree {
    pub async fn suspend_and_capture(&self) -> CaptureResult {
        let mut outcome = self
            .outcome
            .get_or_init(|| {
                let (sender, receiver) = tokio::sync::watch::channel(None);
                let plan = self
                    .plan
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                tokio::spawn(async move {
                    let result = match plan {
                        Some(plan) => plan.capture().await.map(Arc::new),
                        None => Err("tree capture plan was already taken".into()),
                    };
                    let _ = sender.send_replace(Some(result));
                });
                receiver
            })
            .clone();
        loop {
            let result = { outcome.borrow_and_update().clone() };
            if let Some(result) = result {
                return result;
            }
            outcome
                .changed()
                .await
                .map_err(|_| "tree capture owner exited without an outcome".to_owned())?;
        }
    }
}

impl Plan {
    async fn capture(mut self) -> Result<ExecCapturedTree, String> {
        self.seal.commit();
        for member in &mut self.members {
            if let Some(review) = &mut member.review {
                review.commit()?;
            }
        }
        let mut recovery = Vec::with_capacity(self.members.len());
        recovery.push(Some(self.root_options.clone()));
        // The root's shared shutdown must not abort an active child first.
        for member in self.members.iter().skip(1) {
            let options = match &member.active {
                Some(active) => active
                    .suspend(&self.seal)
                    .await
                    .map_err(|e| e.to_string())?,
                None => None,
            };
            if options.is_none() {
                member.thread.seal_exec_idle_inputs(&self.seal).await?;
            }
            recovery.push(options);
        }
        match self
            .root
            .suspend_turn_and_shutdown()
            .await
            .map_err(|e| e.to_string())?
        {
            SuspendTurnOutcome::Suspended { turn_id } if turn_id == self.turn => {}
            _ => return Err("tree capture did not preserve its original root turn".into()),
        }
        self.root
            .session
            .services
            .local_agent_runtime
            .request_shutdown()
            .wait()
            .await
            .map_err(|e| e.to_string())?;
        let active = self
            .members
            .iter()
            .zip(&recovery)
            .filter_map(|(member, recovery)| {
                recovery
                    .as_ref()
                    .map(|options| (member.thread.as_ref(), options.turn()))
            })
            .collect::<Vec<_>>();
        let idle = self
            .members
            .iter()
            .zip(&recovery)
            .filter_map(|(member, recovery)| recovery.is_none().then_some(member.thread.as_ref()))
            .collect::<Vec<_>>();
        let inputs = ExecInputBatch::capture_with_idle(&active, &idle)
            .await
            .map_err(|e| format!("exec stopped tree input capture: {e}"))?;
        inputs.validate(self.root.session.thread_id, &self.turn)?;
        let mut threads = Vec::with_capacity(self.members.len());
        let mut reviews = std::collections::HashMap::new();
        for member in &self.members {
            if let Some(review) = &member.review {
                let delegate = review.delegate()?;
                let child = self
                    .members
                    .iter()
                    .find(|candidate| candidate.thread.session.thread_id == delegate)
                    .ok_or("review delegate disappeared from capture")?;
                reviews.insert(
                    member.thread.session.thread_id,
                    review
                        .snapshot(&child.thread)
                        .await
                        .map_err(|e| format!("exec stopped review capture: {e}"))?,
                );
            }
        }
        for (member, recovery) in self.members.into_iter().zip(recovery) {
            let role = if member.thread.session.thread_id == self.root.session.thread_id {
                "root"
            } else {
                "child"
            };
            let history = member
                .thread
                .exec_recovery_history()
                .await
                .map_err(|e| format!("exec stopped {role} history: {e}"))?;
            let expected = recovery.as_ref().map(ExecRecoveryOptions::turn);
            if super::ephemeral::open_turn(&history.items) != expected {
                return Err("captured child history changed its terminal state".into());
            }
            let ephemeral_history = match &recovery {
                Some(options) => member.thread.exec_ephemeral_history(options.turn()).await?,
                None => member.thread.exec_idle_ephemeral_history().await?,
            };
            // Producers are now closed. Retain their final settings rather
            // than a preflight snapshot that an accepted update could outdate.
            let configuration = capture_configuration(&member.thread, &self.invocation).await?;
            let mut settings = member.thread.restorable_thread_settings().await;
            settings.turn_extension_init = None;
            let dynamic_tools = member.thread.session.dynamic_tools().await.to_vec();
            let root_capabilities_restricted = member
                .thread
                .session
                .state
                .lock()
                .await
                .session_configuration
                .root_capabilities_restricted;
            threads.push(ExecCapturedThread {
                thread: member.thread.session.thread_id,
                parent: member.parent,
                source: member.thread.session_source.clone(),
                root_capabilities_restricted,
                configuration,
                settings,
                dynamic_tools,
                capability_roots: member
                    .thread
                    .session
                    .services
                    .selected_capability_roots
                    .clone(),
                ephemeral_history,
                helper: crate::agent::helper_daybreak::turn::alab_exec_helper::Snapshot::capture(
                    &member.thread.session,
                ),
                recovery,
                review: reviews.remove(&member.thread.session.thread_id),
                history,
            });
        }
        let captured = ExecCapturedTree {
            root: self.root.session.thread_id,
            turn: self.turn,
            threads,
            inputs,
        };
        super::ExecTreeSnapshot::capture(&captured)?;
        Ok(captured)
    }
}

/// Retain final data from every committed member in this root's local tree.
pub(super) async fn capture_idle(
    root: &CodexThread,
    inventory: &crate::CommittedUpgradeInventory,
    invocation: &ExecConfiguration,
) -> Result<(Vec<ExecCapturedThread>, ExecInputBatch), String> {
    if root.session_source.is_non_root_agent() || !inventory.protects_member(root) {
        return Err("idle tree capture requires its committed original root".into());
    }
    if !root.exec_tree_shutdown_complete() {
        return Err("idle tree capture requires joined runtime producer shutdown".into());
    }
    let mut members = Vec::new();
    for thread in inventory
        .threads()
        .iter()
        .filter(|thread| root.shares_exec_tree(thread))
    {
        if !inventory.protects_member(thread) {
            return Err("idle tree capture changed manager membership".into());
        }
        members.push(Member {
            thread: Arc::clone(thread),
            parent: thread.config_snapshot().await.parent_thread_id,
            active: None,
            review: None,
        });
    }
    let members = order_members(root.exec_thread_id(), members)?;
    let idle = members
        .iter()
        .map(|member| member.thread.as_ref())
        .collect::<Vec<_>>();
    let inputs = ExecInputBatch::capture_idle_root(root.exec_thread_id(), &[], &idle).await?;
    let mut nodes = Vec::with_capacity(members.len());
    for member in members {
        nodes.push(capture_idle_node(&member.thread, member.parent, invocation).await?);
    }
    Ok((nodes, inputs))
}

async fn capture_idle_node(
    thread: &CodexThread,
    parent: Option<ThreadId>,
    invocation: &ExecConfiguration,
) -> Result<ExecCapturedThread, String> {
    let history = thread
        .exec_recovery_history()
        .await
        .map_err(|e| e.to_string())?;
    if super::ephemeral::open_turn(&history.items).is_some() {
        return Err("idle tree member retains an open turn".into());
    }
    let configuration = capture_configuration(thread, invocation).await?;
    let mut settings = thread.restorable_thread_settings().await;
    settings.turn_extension_init = None;
    let root_capabilities_restricted = thread
        .session
        .state
        .lock()
        .await
        .session_configuration
        .root_capabilities_restricted;
    Ok(ExecCapturedThread {
        thread: thread.exec_thread_id(),
        parent,
        source: thread.session_source.clone(),
        root_capabilities_restricted,
        configuration,
        settings,
        dynamic_tools: thread.session.dynamic_tools().await.to_vec(),
        capability_roots: thread.session.services.selected_capability_roots.clone(),
        ephemeral_history: thread.exec_idle_ephemeral_history().await?,
        helper: crate::agent::helper_daybreak::turn::alab_exec_helper::Snapshot::capture(
            &thread.session,
        ),
        recovery: None,
        review: None,
        history,
    })
}

#[path = "exec_upgrade_forest.rs"]
mod upgrade_forest;
pub use upgrade_forest::CommittedUpgradeForest;
pub use upgrade_forest::DecodedUpgradeForest;
pub use upgrade_forest::PreparedUpgradeForest;
pub use upgrade_forest::UpgradeForestSnapshot;

#[cfg(test)]
#[path = "exec_tree_capture_tests.rs"]
mod tests;
