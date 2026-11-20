// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Frozen Source request/offer DATA shared by real client and server. Every
//! actual prepare/accept/replay independently requires the same Native origin.
use super::ClientConstructionSelection;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub(crate) const KEY: &str = "alab.clientConstructionPrepare";
pub(crate) const SAVED_OPENING_KEY: &str = "alab.savedResumeOpening";
pub(crate) const RESULT_KEY: &str = "alab.clientConstructionPrepared";
pub(crate) const MAX_ARGUMENT_BYTES: usize = 2 * 1024 * 1024;
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Method {
    #[serde(rename = "thread/start")]
    Start,
    #[serde(rename = "thread/fork")]
    Fork,
    #[serde(rename = "thread/resume")]
    Resume,
}
impl Method {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Start => "thread/start",
            Self::Fork => "thread/fork",
            Self::Resume => "thread/resume",
        }
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Prepare {
    /// Client-owned PRE-SEND correlation DATA. Retain in the actual pending
    /// UI operation checkpoint; this nonce never identifies a Core or role.
    pub preparation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_opening: Option<String>,
    pub method: Method,
    pub parameters: Value,
    pub selection: ClientConstructionSelection,
}
impl Prepare {
    pub(crate) fn saved_resume_child(&self, offer: &Offer) -> Option<&str> {
        if self.preparation != offer.preparation
            || !offer.valid()
            || !self.valid()
            || self.saved_opening.is_none()
        {
            return None;
        }
        self.parameters.get("threadId")?.as_str()
    }
    fn tagged_child(&self) -> Option<&str> {
        if self.saved_opening.as_ref()? != &self.preparation
            || self.method != Method::Resume
            || self.selection.presentation != super::ClientPresentation::SideConversation
        {
            return None;
        }
        let child = self.parameters.get("threadId")?.as_str()?;
        let id = codex_protocol::ThreadId::from_string(child).ok()?;
        let params =
            codex_app_server_protocol::ThreadResumeParams::deserialize(&self.parameters).ok()?;
        (id.to_string() == child
            && params.path.is_none()
            && params.history.is_none()
            && serde_json::to_value(&params).ok()? == self.parameters)
            .then_some(child)
    }

    pub(crate) fn valid(&self) -> bool {
        token(&self.preparation)
            && self.parameters.is_object()
            && self.selection.prepared.is_none()
            && (self.saved_opening.is_none() || self.tagged_child().is_some())
            && (self.method != Method::Start
                || (self.selection.presentation == super::ClientPresentation::Primary
                    && serde_json::from_value::<codex_app_server_protocol::ThreadStartParams>(
                        self.parameters.clone(),
                    )
                    .is_ok_and(|params| params == self.selection.prospective_create)))
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= MAX_ARGUMENT_BYTES)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Offer {
    pub preparation: String,
    /// Backend-issued immutable operation selection, never permission.
    pub claim: String,
    /// Exact Native reservation selectors observed by the actual backend SDK.
    /// They are DATA; SDK current-A, reservation and S2 checks remain required.
    pub source_operation: String,
    pub selection: String,
}
impl Offer {
    pub(crate) fn valid(&self) -> bool {
        [
            &self.preparation,
            &self.claim,
            &self.source_operation,
            &self.selection,
        ]
        .into_iter()
        .all(|value| token(value))
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    Prepare(Box<Prepare>),
    Retire {
        preparation: String,
        claim: String,
    },
    ReadSavedResume {
        original: Arc<Prepare>,
        offer: Offer,
    },
}
impl Request {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Prepare(value) => value.valid(),
            Self::Retire { preparation, claim } => token(preparation) && token(claim),
            Self::ReadSavedResume { original, offer } => {
                original.saved_resume_child(offer).is_some()
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "camelCase", deny_unknown_fields)]
pub enum Completion {
    Prepared(Offer),
    SavedResume {
        offer: Offer,
        #[serde(rename = "threadId")]
        thread_id: String,
        outcome: SavedResumeOutcome,
    },
    /// Actual Source registry never accepted a Core worker, and actual Native
    /// unaccepted retirement completed. This is NOT all-effects settlement.
    Retired {
        preparation: String,
        claim: String,
    },
    /// Accepted work is retained and cannot be canceled by an HTTP/UI observer.
    Accepted {
        preparation: String,
        claim: String,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SavedResumeOutcome {
    Resumed {},
    Rejected {
        code: i64,
        message: String,
        #[serde(rename = "hadData")]
        had_data: bool,
        value: Value,
    },
    Pending {},
    Unavailable {},
}
pub(crate) fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
