// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Shared Source selection DATA, never Native/currentness/Go authority.
use codex_app_server_protocol::ThreadStartParams;
use serde::Deserialize;
use serde::Serialize;
pub(crate) const META: &str = "_meta";
pub(crate) const KEY: &str = "alab.initialSession";
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitialSessionSelection {
    pub claim: String,
    /// Existing full UI projection for a NEW future CREATE, never a claim that
    /// the fork was historically constructed by ThreadStartParams.
    pub prospective_create: Option<ThreadStartParams>,
}
impl InitialSessionSelection {
    pub fn valid(&self) -> bool {
        self.claim.len() == 32
            && self
                .claim
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }
}

pub(crate) const RESULT_KEY: &str = "alab.initialSessionResult";
/// Typed Source observation of the SAME logical startup. It is neither Native
/// currentness/Go nor advancing runtime/UI reconciliation evidence.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitialSessionResult {
    pub source_selection: preparation::Offer,
    pub claim: String,
    pub thread_id: String,
    pub history_fallback: bool,
    pub dynamic_tools_fallback: bool,
    /// Set only after the actual configured host binding accepted this Core Arc.
    pub backend_task_tools: bool,
    /// Opaque accepted Scope DATA. Initial and later construction use genuine
    /// S2/UI/SelectionCommit; R only reconciles an already-selected primary.
    pub primary_candidate: Option<String>,
}

pub(crate) const NEXT_KEY: &str = "alab.initialSessionNext";
/// A fresh backend-issued selection offer after observed normal no-entry/no-
/// capture failure. It is Source DATA, never permission or automatic retry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NextSelection {
    pub previous_claim: String,
    pub claim: String,
}
impl NextSelection {
    pub fn valid(&self) -> bool {
        [&self.previous_claim, &self.claim]
            .into_iter()
            .all(|claim| {
                claim.len() == 32
                    && claim
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            && self.previous_claim != self.claim
    }
}

pub(crate) const CLIENT_KEY: &str = "alab.clientConstruction";
pub(crate) const CLIENT_RESULT_KEY: &str = "alab.clientConstructionResult";
/// The actual original UI call distinguishes a pending main selection from a
/// side conversation. This is Source presentation DATA, never Native authority.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientPresentation {
    Primary,
    SideConversation,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientConstructionSelection {
    pub prospective_create: ThreadStartParams,
    pub presentation: ClientPresentation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared: Option<preparation::Offer>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientConstructionResult {
    pub prepared: preparation::Offer,
    pub thread_id: String,
    pub history_fallback: bool,
    pub dynamic_tools_fallback: bool,
    pub backend_task_tools: bool,
    /// Opaque accepted Scope DATA. Initial and later construction use genuine
    /// S2/UI/SelectionCommit; R only reconciles an already-selected primary.
    pub primary_candidate: Option<String>,
}

#[path = "client_construction_preparation_metadata.rs"]
pub(crate) mod preparation;
