// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Closed actual request DATA; initial replay and later accepted calls remain
//! distinct. No Source request id/projection is a Native or tool-call identity.
use crate::outgoing_message::alab_initial_claim::Claim;
use crate::outgoing_message::alab_initial_claim::ClientClaim;
use crate::outgoing_message::alab_initial_claim::ClientConstructionSelection;
use crate::outgoing_message::alab_initial_claim::ClientPresentation;
use crate::outgoing_message::alab_initial_claim::InitialSessionSelection;
use crate::outgoing_message::alab_initial_claim::PreparationOffer;
use std::sync::Arc;
enum Selection {
    Initial(Arc<InitialSessionSelection>),
    Client(Arc<ClientConstructionSelection>),
}
pub(super) struct FrozenRequest {
    parameters: Arc<serde_json::Value>,
    method: Arc<str>,
    selection: Selection,
    saved_opening: bool,
}
impl FrozenRequest {
    pub(super) fn new(claim: Claim) -> Self {
        Self {
            parameters: claim.arguments,
            method: claim.method,
            selection: Selection::Initial(claim.selection),
            saved_opening: false,
        }
    }
    pub(super) fn client(claim: ClientClaim, saved_opening: bool) -> Self {
        Self {
            parameters: claim.arguments,
            method: claim.method,
            selection: Selection::Client(claim.selection),
            saved_opening,
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
    pub(super) fn matches_saved_resume(
        &self,
        original: &crate::outgoing_message::alab_initial_claim::PreparedArguments,
        offer: &crate::outgoing_message::alab_initial_claim::PreparationOffer,
    ) -> bool {
        self.saved_opening
            && original.preparation == offer.preparation
            && original.saved_opening.as_ref() == Some(&offer.preparation)
            && original.selection.prepared.is_none()
            && matches!(&self.selection, Selection::Client(actual)
            if actual.prepared.as_ref() == Some(offer)
                && actual.presentation == original.selection.presentation
                && actual.prospective_create == original.selection.prospective_create)
            && self.method.as_ref() == original.method.name()
            && self.parameters.as_ref() == &original.parameters
    }
    /// Select only the exact tagged resume admitted with this backend offer.
    pub(super) fn saved_resume_key(
        &self,
        offer: &PreparationOffer,
    ) -> Option<(super::saved_resume_receipts::Key, &str)> {
        let Selection::Client(selection) = &self.selection else {
            return None;
        };
        if !self.saved_opening || selection.prepared.as_ref() != Some(offer) {
            return None;
        }
        let child = self.parameters.get("threadId")?.as_str()?;
        let key = super::saved_resume_receipts::key_for_fields(
            offer,
            self.saved_opening,
            &self.method,
            &self.parameters,
            &selection.prospective_create,
            selection.presentation,
        )?;
        Some((key, child))
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
