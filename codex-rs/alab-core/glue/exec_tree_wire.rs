// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Portable source data, never authority to select accounts, attach environments or resume turns.
use super::ExecCapturedTree;
use super::ExecEphemeralHistory;
use super::ExecInputBatch;
use super::ExecRecoveryOptions;
use super::ephemeral::HistoryEvidence;
use crate::CodexThreadSettingsOverrides;
use crate::config::ExecConfiguration;
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::protocol::SessionSource;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;

#[path = "exec_tree_environments.rs"]
mod environments;
#[path = "exec_tree_settings.rs"]
mod settings;
use settings::Settings;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecTreeSnapshot {
    version: u32,
    root: ThreadId,
    turn: Option<String>,
    threads: Vec<ExecThreadSnapshot>,
    inputs: ExecInputBatch,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecThreadSnapshot {
    thread: ThreadId,
    parent: Option<ThreadId>,
    source: SessionSource,
    root_capabilities_restricted: bool,
    configuration: ExecConfiguration,
    settings: Settings,
    tools: Vec<DynamicToolSpec>,
    capability_roots: Vec<codex_protocol::capabilities::SelectedCapabilityRoot>,
    ephemeral: Option<ExecEphemeralHistory>,
    helper: Option<crate::agent::helper_daybreak::turn::alab_exec_helper::Snapshot>,
    recovery: Option<ExecRecoveryOptions>,
    review: Option<crate::ExecReviewSnapshot>,
    history: HistoryEvidence,
}

pub(super) fn preflight_settings(source: &CodexThreadSettingsOverrides) -> Result<(), String> {
    let settings = Settings::capture(source)?;
    let value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    serde_json::from_value::<Settings>(value)
        .map_err(|e| e.to_string())?
        .validate()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecTreeRecords {
    version: u32,
    root: ThreadId,
    turn: Option<String>,
    threads: Vec<ExecThreadSnapshot>,
}
impl ExecTreeRecords {
    pub(crate) fn root(&self) -> ThreadId {
        self.root
    }
    pub(crate) fn threads(&self) -> Vec<ThreadId> {
        self.threads.iter().map(|node| node.thread).collect()
    }
    pub(crate) fn startup_request(
        &self,
    ) -> Result<(ThreadId, crate::config::ConfigOverrides), String> {
        let root = self
            .threads
            .first()
            .filter(|node| node.thread == self.root && node.parent.is_none())
            .ok_or("runtime root configuration is absent")?;
        Ok((
            self.root,
            root.configuration
                .startup_request(root.ephemeral.is_some())
                .map_err(|error| error.to_string())?,
        ))
    }
    pub(crate) fn with_inputs(self, inputs: ExecInputBatch) -> Result<ExecTreeSnapshot, String> {
        let tree = ExecTreeSnapshot {
            version: self.version,
            root: self.root,
            turn: self.turn,
            threads: self.threads,
            inputs,
        };
        tree.validate(tree.root, None)?;
        Ok(tree)
    }
}

impl ExecTreeSnapshot {
    pub(crate) fn into_records(self) -> ExecTreeRecords {
        ExecTreeRecords {
            version: self.version,
            root: self.root,
            turn: self.turn,
            threads: self.threads,
        }
    }

    pub(super) fn preflight_idle_nodes(nodes: &[super::ExecCapturedThread]) -> Result<(), String> {
        let saved = Self::capture_threads(nodes)?;
        let bytes = serde_json::to_vec(&saved).map_err(|e| e.to_string())?;
        let nodes: Vec<ExecThreadSnapshot> =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        for node in nodes {
            node.settings.validate()?;
            if super::ephemeral::open_turn(&node.history()?.items).is_some() {
                return Err("idle preflight history retains an unfinished turn".into());
            }
            if let Some(memory) = node.ephemeral {
                memory.restore_for(node.thread, None)?;
            }
            if let Some(helper) = node.helper {
                helper.validate(&node.source)?;
            }
        }
        Ok(())
    }

    pub fn capture(source: &ExecCapturedTree) -> Result<Self, String> {
        let threads = Self::capture_threads(source.threads())?;
        let snapshot = Self {
            version: 4,
            root: source.root(),
            turn: Some(source.turn().into()),
            threads,
            inputs: source.inputs().clone(),
        };
        // Exercise the same codec used by the native capsule before reporting success.
        let encoded = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
        let snapshot: Self = serde_json::from_slice(&encoded).map_err(|e| e.to_string())?;
        snapshot.validate(source.root(), Some(source.turn()))?;
        Ok(snapshot)
    }
    fn capture_threads(
        nodes: &[super::ExecCapturedThread],
    ) -> Result<Vec<ExecThreadSnapshot>, String> {
        let mut threads = Vec::new();
        for node in nodes {
            threads.push(ExecThreadSnapshot {
                thread: node.thread(),
                parent: node.parent(),
                source: node.source().clone(),
                root_capabilities_restricted: node.root_capabilities_restricted(),
                configuration: node.configuration().clone(),
                settings: Settings::capture(node.settings())?,
                tools: node.dynamic_tools().to_vec(),
                capability_roots: node.capability_roots().to_vec(),
                ephemeral: node.ephemeral_history().cloned(),
                helper: node.helper().cloned(),
                recovery: node.recovery().cloned(),
                review: node.review().cloned(),
                history: HistoryEvidence::capture(node.history())?,
            });
        }
        Ok(threads)
    }

    /// Capture an idle tree only from original committed, writer-closed members.
    /// This serializes data; destination admission remains independent.
    pub async fn capture_idle(
        root: &crate::CodexThread,
        inventory: &crate::CommittedUpgradeInventory,
        invocation: &ExecConfiguration,
    ) -> Result<Self, String> {
        let (nodes, inputs) =
            super::tree_capture::capture_idle(root, inventory, invocation).await?;
        let snapshot = Self {
            version: 4,
            root: root.exec_thread_id(),
            turn: None,
            threads: Self::capture_threads(&nodes)?,
            inputs,
        };
        let encoded = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
        let snapshot: Self = serde_json::from_slice(&encoded).map_err(|e| e.to_string())?;
        snapshot.validate(root.exec_thread_id(), None)?;
        Ok(snapshot)
    }

    pub fn root(&self) -> ThreadId {
        self.root
    }
    pub fn turn(&self) -> Option<&str> {
        self.turn.as_deref()
    }
    pub fn threads(&self) -> &[ExecThreadSnapshot] {
        &self.threads
    }
    pub fn inputs(&self) -> &ExecInputBatch {
        &self.inputs
    }

    pub fn validate(&self, root: ThreadId, turn: Option<&str>) -> Result<(), String> {
        if self.version != 4
            || self.root != root
            || self.turn.as_deref() != turn
            || turn.is_some_and(str::is_empty)
            || self.threads.is_empty()
        {
            return Err("tree snapshot does not match its original root and turn".into());
        }
        let mut seen = HashSet::new();
        let mut active = Vec::new();
        let mut idle = Vec::new();
        for (index, node) in self.threads.iter().enumerate() {
            if index == 0 {
                if node.thread != root
                    || node.source.is_non_root_agent()
                    || node.parent.is_some()
                    || node.recovery.as_ref().map(ExecRecoveryOptions::turn) != turn
                {
                    return Err("tree snapshot has an invalid root frame".into());
                }
            } else if !node.source.is_non_root_agent()
                || !node.parent.is_some_and(|parent| seen.contains(&parent))
            {
                return Err("tree snapshot has a missing or unordered parent".into());
            }
            if node
                .source
                .parent_thread_id()
                .is_some_and(|parent| Some(parent) != node.parent)
                || !seen.insert(node.thread)
            {
                return Err("tree snapshot repeats a thread or changes its parent".into());
            }
            node.settings.validate()?;
            if let Some(helper) = &node.helper {
                helper.validate(&node.source)?;
            }
            if node
                .recovery
                .as_ref()
                .is_some_and(|saved| saved.helper_choice().is_some())
                && !matches!(
                    &node.helper,
                    Some(
                        crate::agent::helper_daybreak::turn::alab_exec_helper::Snapshot::Helper { .. }
                    )
                )
            {
                return Err("implicit helper recovery lacks its native helper data".into());
            }
            if node
                .recovery
                .as_ref()
                .is_some_and(ExecRecoveryOptions::is_review)
                != node.review.is_some()
            {
                return Err("tree review task is missing its consumer state".into());
            }
            if let Some(review) = &node.review {
                review.validate(
                    node.thread,
                    node.recovery
                        .as_ref()
                        .ok_or("review has no active turn")?
                        .turn(),
                )?;
                let delegate = self
                    .threads
                    .iter()
                    .find(|child| child.thread == review.delegate())
                    .ok_or("review snapshot has no captured delegate")?;
                if delegate.parent != Some(node.thread)
                    || delegate.source
                        != SessionSource::Internal(
                            codex_protocol::protocol::InternalSessionSource::Review,
                        )
                    || match &delegate.recovery {
                        Some(options) => {
                            options.turn() != review.delegate_turn() || options.is_review()
                        }
                        None => !review.retains_completed_delegate(&delegate.history()?),
                    }
                {
                    return Err("review snapshot changed its delegate identity or turn".into());
                }
            }
            let history = node.history.restore(node.thread)?;
            let expected = match &node.recovery {
                Some(options) => {
                    if options.thread() != node.thread || options.turn().is_empty() {
                        return Err("tree recovery options changed thread identity".into());
                    }
                    active.push((node.thread, options.turn()));
                    Some(options.turn())
                }
                None => {
                    idle.push(node.thread);
                    None
                }
            };
            if super::ephemeral::open_turn(&history.items) != expected {
                return Err("tree history does not match its captured turn state".into());
            }
            if let Some(ephemeral) = &node.ephemeral {
                let items = ephemeral.clone().restore_for(node.thread, expected)?;
                node.history
                    .verify(&codex_thread_store::StoredThreadHistory {
                        thread_id: node.thread,
                        items,
                        revision: None,
                    })?;
            }
        }
        self.inputs.validate_members(&active, &idle)?;
        match turn {
            Some(turn) => self.inputs.validate(root, turn),
            None => self.inputs.validate_idle(root),
        }
    }
}

impl ExecThreadSnapshot {
    /// Retain source restrictions before startup without relaxing fresh host policy.
    pub fn restrict_root_capabilities(&self, init: &mut codex_extension_api::ExtensionDataInit) {
        if self.root_capabilities_restricted {
            init.insert(crate::RestrictRootCapabilities);
        }
    }

    /// Stage data only after the caller has validated the receipt-bound tree.
    pub fn stage_helper_restore(&self, init: &mut codex_extension_api::ExtensionDataInit) {
        if let Some(helper) = &self.helper {
            helper.stage(init);
        }
    }

    pub fn thread(&self) -> ThreadId {
        self.thread
    }
    pub fn parent(&self) -> Option<ThreadId> {
        self.parent
    }
    pub fn source(&self) -> &SessionSource {
        &self.source
    }
    pub fn configuration(&self) -> &ExecConfiguration {
        &self.configuration
    }
    pub fn recovery(&self) -> Option<&ExecRecoveryOptions> {
        self.recovery.as_ref()
    }
    pub fn review(&self) -> Option<&crate::ExecReviewSnapshot> {
        self.review.as_ref()
    }
    pub fn dynamic_tools(&self) -> &[DynamicToolSpec] {
        &self.tools
    }
    pub fn capability_roots(&self) -> &[codex_protocol::capabilities::SelectedCapabilityRoot] {
        &self.capability_roots
    }
    pub fn ephemeral_history(&self) -> Option<&ExecEphemeralHistory> {
        self.ephemeral.as_ref()
    }
    pub fn history(&self) -> Result<codex_thread_store::StoredThreadHistory, String> {
        self.history.restore(self.thread)
    }
    pub fn verify_history(
        &self,
        current: &codex_thread_store::StoredThreadHistory,
    ) -> Result<(), String> {
        self.history.verify(current)
    }
    /// Decode requested settings against freshly acquired owner bindings. The
    /// result still needs normal Core policy validation and native admission.
    pub fn restore_settings(
        &self,
        fresh: &CodexThreadSettingsOverrides,
        fresh_roots: &[codex_protocol::capabilities::SelectedCapabilityRoot],
    ) -> Result<CodexThreadSettingsOverrides, String> {
        if self.capability_roots != fresh_roots {
            return Err(
                "tree capability selection or alias order changed; revalidation is required".into(),
            );
        }
        self.settings.restore(fresh)
    }
}

#[cfg(test)]
#[path = "exec_tree_wire_tests.rs"]
mod tests;
