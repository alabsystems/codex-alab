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
    turn: String,
    threads: Vec<ExecThreadSnapshot>,
    inputs: ExecInputBatch,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecThreadSnapshot {
    thread: ThreadId,
    parent: Option<ThreadId>,
    source: SessionSource,
    configuration: ExecConfiguration,
    settings: Settings,
    tools: Vec<DynamicToolSpec>,
    capability_roots: Vec<codex_protocol::capabilities::SelectedCapabilityRoot>,
    ephemeral: Option<ExecEphemeralHistory>,
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

impl ExecTreeSnapshot {
    pub fn capture(source: &ExecCapturedTree) -> Result<Self, String> {
        let mut threads = Vec::new();
        for node in source.threads() {
            threads.push(ExecThreadSnapshot {
                thread: node.thread(),
                parent: node.parent(),
                source: node.source().clone(),
                configuration: node.configuration().clone(),
                settings: Settings::capture(node.settings())?,
                tools: node.dynamic_tools().to_vec(),
                capability_roots: node.capability_roots().to_vec(),
                ephemeral: node.ephemeral_history().cloned(),
                recovery: node.recovery().cloned(),
                review: node.review().cloned(),
                history: HistoryEvidence::capture(node.history())?,
            });
        }
        let snapshot = Self {
            version: 2,
            root: source.root(),
            turn: source.turn().into(),
            threads,
            inputs: source.inputs().clone(),
        };
        // Exercise the same codec used by the native capsule before reporting success.
        let encoded = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
        let snapshot: Self = serde_json::from_slice(&encoded).map_err(|e| e.to_string())?;
        snapshot.validate(source.root(), source.turn())?;
        Ok(snapshot)
    }
    pub fn root(&self) -> ThreadId {
        self.root
    }
    pub fn turn(&self) -> &str {
        &self.turn
    }
    pub fn threads(&self) -> &[ExecThreadSnapshot] {
        &self.threads
    }
    pub fn inputs(&self) -> &ExecInputBatch {
        &self.inputs
    }

    pub fn validate(&self, root: ThreadId, turn: &str) -> Result<(), String> {
        if self.version != 2
            || self.root != root
            || self.turn != turn
            || turn.is_empty()
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
                    || node.recovery.as_ref().map(ExecRecoveryOptions::turn) != Some(turn)
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
        self.inputs.validate(root, turn)
    }
}

impl ExecThreadSnapshot {
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
