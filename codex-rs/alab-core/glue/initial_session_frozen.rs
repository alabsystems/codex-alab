// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Closed actual request DATA; initial replay and later accepted calls remain
//! distinct. No Source request id/projection is a Native or tool-call identity.
use crate::outgoing_message::alab_initial_claim::Claim;
use crate::outgoing_message::alab_initial_claim::ClientClaim;
use crate::outgoing_message::alab_initial_claim::ClientConstructionSelection;
use crate::outgoing_message::alab_initial_claim::ClientPresentation;
use crate::outgoing_message::alab_initial_claim::InitialSessionSelection;
use std::sync::Arc;
enum Selection {
    Initial(Arc<InitialSessionSelection>),
    Client(Arc<ClientConstructionSelection>),
}
pub(super) struct FrozenRequest {
    parameters: Arc<serde_json::Value>,
    method: Arc<str>,
    selection: Selection,
}
impl FrozenRequest {
    pub(super) fn new(claim: Claim) -> Self {
        Self {
            parameters: claim.arguments,
            method: claim.method,
            selection: Selection::Initial(claim.selection),
        }
    }
    pub(super) fn client(claim: ClientClaim) -> Self {
        Self {
            parameters: claim.arguments,
            method: claim.method,
            selection: Selection::Client(claim.selection),
        }
    }
    pub(super) fn matches(&self, claim: &Claim) -> bool {
        matches!(&self.selection, Selection::Initial(actual) if actual == &claim.selection)
            && self.method == claim.method
            && self.parameters == claim.arguments
    }
    pub(super) fn matches_client(&self, claim: &ClientClaim) -> bool {
        matches!(&self.selection, Selection::Client(actual) if actual == &claim.selection)
            && self.method == claim.method
            && self.parameters == claim.arguments
    }
    pub(super) fn claim(&self) -> Option<&str> {
        match &self.selection {
            Selection::Initial(actual) => Some(&actual.claim),
            Selection::Client(_) => None,
        }
    }
    pub(super) fn prospective(&self) -> Option<&codex_app_server_protocol::ThreadStartParams> {
        match &self.selection {
            Selection::Initial(actual) => actual.prospective_create.as_ref(),
            Selection::Client(actual) => Some(&actual.prospective_create),
        }
    }
    pub(super) fn retains_candidate(&self) -> bool {
        match &self.selection {
            Selection::Initial(_) => true,
            Selection::Client(actual) => actual.presentation == ClientPresentation::Primary,
        }
    }
}
