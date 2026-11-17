// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Distinct actual S2 resources on the SAME original listener/history path.
//! No R0/R1 receipt or RPC completion can stand in for a SelectionCommit.
use super::super::alab_initial_session::selected_primary::PreparedSelectionInstall;
use super::super::alab_initial_session::selected_primary::PrimaryCandidate;
use super::super::alab_initial_session::selected_primary::SelectedPrimary;
use super::*;
use crate::outgoing_message::alab_source_selection_claim as claim;
use codex_alab_aterm::frontend_control::retained_service::HeldSourceTransition;
use codex_alab_aterm::frontend_control::retained_service::SelectionAck;
use codex_alab_aterm::frontend_control::retained_service::SelectionFrontier;
use codex_alab_aterm::frontend_control::retained_service::SelectionObservation;
use codex_alab_aterm::frontend_control::retained_service::SelectionReleased;

pub(super) enum PrimaryResource {
    Candidate(Arc<PrimaryCandidate>),
    Selected(Arc<SelectedPrimary>),
}
impl PrimaryResource {
    pub(super) fn thread_id(&self) -> ThreadId {
        match self {
            Self::Candidate(x) => x.thread_id(),
            Self::Selected(x) => x.thread_id(),
        }
    }
    pub(super) fn thread(&self) -> &Arc<CodexThread> {
        match self {
            Self::Candidate(x) => x.thread(),
            Self::Selected(x) => x.thread(),
        }
    }
    pub(super) fn lease_is_for(&self, actual: &Arc<CodexThread>) -> bool {
        match self {
            Self::Candidate(x) => x.lease().is_some_and(|lease| lease.is_for(actual)),
            Self::Selected(x) => x.lease().is_some_and(|lease| lease.is_for(actual)),
        }
    }
    pub(super) fn is_exact(&self, actual: &Arc<CodexThread>) -> bool {
        Arc::ptr_eq(self.thread(), actual)
    }
    pub(super) fn belongs_to(&self, input: &Incoming) -> bool {
        match self {
            Self::Candidate(x) => x.belongs_to(input),
            Self::Selected(x) => x.belongs_to(input),
        }
    }
    pub(super) fn recheck_owned(&self) -> Result<(), JSONRPCErrorError> {
        match self {
            Self::Candidate(x) => x.recheck_owned(),
            Self::Selected(x) => x.recheck_owned(),
        }
    }
    pub(super) async fn recheck(&self) -> Result<(), JSONRPCErrorError> {
        match self {
            Self::Candidate(x) => x.recheck().await,
            Self::Selected(x) => x.recheck().await,
        }
    }
    pub(super) fn backend_task_tools(&self) -> Result<bool, JSONRPCErrorError> {
        match self {
            Self::Candidate(x) => x.backend_task_tools(),
            Self::Selected(x) => x.backend_task_tools(),
        }
    }
}
pub(super) enum PhysicalFrontier {
    Runtime(RuntimeFrontier),
    Source(Arc<SourceProjection>),
}
pub(super) enum ProjectionAck {
    Runtime(RuntimeAck),
    Source(SelectionAck),
}
impl PhysicalFrontier {
    pub(super) fn same_receipt(&self, origin: &AcceptedOrigin) -> bool {
        match self {
            Self::Runtime(x) => x.same_receipt(origin),
            Self::Source(x) => x.frontier.same_receipt(origin),
        }
    }
    pub(super) fn metadata(
        &self,
        receipt: &Receipt,
    ) -> Result<(&'static str, serde_json::Value), JSONRPCErrorError> {
        let (key, value) = match self {
            Self::Runtime(_) => (
                alab_frontier_claim::RESULT_KEY,
                serde_json::to_value(receipt),
            ),
            Self::Source(source) => (claim::RESULT_KEY, serde_json::to_value(&source.receipt)),
        };
        value
            .map(|value| (key, value))
            .map_err(|_| internal_error("actual projection receipt encode failed"))
    }
    pub(super) async fn acknowledge_reconciled(
        &self,
        origin: &AcceptedOrigin,
    ) -> Result<ProjectionAck, RuntimeObservationError> {
        match self {
            Self::Runtime(frontier) => frontier
                .acknowledge_reconciled(origin)
                .await
                .map(ProjectionAck::Runtime),
            Self::Source(source) => source
                .frontier
                .acknowledge_reconciled(origin)
                .await
                .map(ProjectionAck::Source),
        }
    }
}
pub(super) struct SourceProjection {
    candidate: Arc<PrimaryCandidate>,
    boundary: Option<codex_core::RecordedSideBoundary>,
    side_display: StdMutex<Option<claim::side_display::SideDisplayBoundary>>,
    listener_cut: StdMutex<Option<Arc<crate::thread_state::alab_side_boundary::Cut>>>,
    held: Arc<HeldSourceTransition>,
    frontier: SelectionFrontier,
    receipt: claim::Receipt,
    install: Option<PreparedSelectionInstall>,
    ack: StdMutex<Option<SelectionAck>>,
    observed: StdMutex<Option<SelectionObservation>>,
    released: StdMutex<Option<SelectionReleased>>,
}
impl SourceProjection {
    pub(super) fn store_ack(&self, ack: SelectionAck) {
        *self.ack.lock().unwrap_or_else(PoisonError::into_inner) = Some(ack);
    }
    pub(super) async fn activate(
        &self,
        binding: &Binding,
        work: &Projection,
        activation: &ActivationWork,
    ) -> Result<(), JSONRPCErrorError> {
        // The caller owns the ORIGINAL goal-resume guard through this whole
        // Native observation -> last Current -> exact Source-release window.
        let ack = self
            .ack
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| invalid_request("actual Source listener ACK missing"))?;
        if !ack.same_frontier(&self.frontier) || !self.frontier.same_transition(&self.held) {
            return Err(invalid_request("actual Source frontier resource disagrees"));
        }
        activation.uncertain.store(true, Ordering::Release);
        match activation.phase {
            ActivationPhase::Observe => {
                let observed = match ack.observe_selection().await {
                    Ok(value) => value,
                    Err(RuntimeObservationError::Refused(error)) => {
                        activation.uncertain.store(false, Ordering::Release);
                        return Err(native_error(error));
                    }
                    Err(RuntimeObservationError::Uncertain(error)) => {
                        return Err(native_error(error));
                    }
                };
                *self.observed.lock().unwrap_or_else(PoisonError::into_inner) = Some(observed);
                self.candidate.recheck().await?;
                binding.current(work)?;
                // SAME independent Source ticket and initial/H1 Hold stay closed.
            }
            ActivationPhase::Commit => {
                let observed = self
                    .observed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                    .ok_or_else(|| invalid_request("actual Source Native observation missing"))?;
                let committed = match ack.observe_commit(&observed).await {
                    Ok(value) => value,
                    Err(RuntimeObservationError::Refused(error)) => {
                        activation.uncertain.store(false, Ordering::Release);
                        return Err(native_error(error));
                    }
                    Err(RuntimeObservationError::Uncertain(error)) => {
                        return Err(native_error(error));
                    }
                };
                // Real Native commit is followed by the LAST actual Current
                // query under the original lifecycle serialization.
                self.candidate.recheck().await?;
                binding.current(work)?;
                let released = match &self.install {
                    Some(install) => {
                        install
                            .release_committed(&ack, &committed, || {
                                binding
                                    .current(work)
                                    .map_err(|_| "actual Source primary changed".into())
                            })
                            .await?
                    }
                    None => ack
                        .release_ticket_committed(&self.held, &committed, || {
                            binding
                                .current(work)
                                .map_err(|_| "actual side conversation changed".into())
                        })
                        .await
                        .map_err(native_error)?,
                };
                // Store the exact typed release BEFORE Native bookkeeping.
                // A lost observer never resubmits Core, release or initial turn.
                *self.released.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(released.clone());
                self.candidate.finish_source_selection(&released).await?;
            }
        }
        activation.uncertain.store(false, Ordering::Release);
        Ok(())
    }
}
impl Binding {
    async fn handle_source(
        self: &Arc<Self>,
        request_id: ConnectionRequestId,
        context: RequestContext,
        params: ThreadResumeParams,
        selection: claim::Selection,
        client_name: Option<String>,
        client_version: Option<String>,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        if context.alab_request_id() != &request_id || params.thread_id != selection.thread_id() {
            return Err(invalid_request(
                "Source projection original request disagrees",
            ));
        }
        let origin =
            context
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| {
                    invalid_request("Source selection requires actual physical A origin")
                })?;
        origin.check_current().await.map_err(native_error)?;
        self.open()?;
        let activation_phase = match &selection {
            claim::Selection::Observe(_) => ActivationPhase::Observe,
            claim::Selection::Activate(_) => ActivationPhase::Commit,
            claim::Selection::Project(_) => ActivationPhase::Observe,
        };
        match selection {
            claim::Selection::Project(selection) => {
                let candidate = Arc::new(
                    self.processor
                        .alab_initial_session
                        .source_candidate(&selection.claim)
                        .await?,
                );
                if !candidate.belongs_to(&self.input)
                    || candidate.thread_id().to_string() != params.thread_id
                {
                    return Err(invalid_request(
                        "Source candidate belongs to another actual runtime",
                    ));
                }
                let offer = candidate.source_offer()?;
                if offer.claim != selection.claim
                    || offer.source_operation != selection.source_operation
                    || offer.selection != selection.selection
                {
                    return Err(invalid_request("Source reservation selection disagrees"));
                }
                let boundary = candidate.side_boundary()?;
                let held = candidate.source_selection()?;
                let frontier = held
                    .frontier(&selection.frontier, selection.go.as_deref(), &origin)
                    .await
                    .map_err(native_error)?;
                candidate.recheck().await?;
                let actual_gate = candidate
                    .thread()
                    .thread_extension_data()
                    .get::<codex_alab_core::retained_goal_gate::Gate>()
                    .ok_or_else(|| invalid_request("Source candidate has no actual goal gate"))?;
                if !frontier.same_transition(&held) || !actual_gate.same(&frontier.goal_gate()) {
                    return Err(invalid_request(
                        "Source candidate belongs to another actual Source owner",
                    ));
                }
                let install = candidate
                    .is_primary_presentation()
                    .then(|| candidate.prepare_source_install(&frontier))
                    .transpose()?;
                let snapshot = token();
                let tools = candidate.backend_task_tools()?;
                let source = Arc::new(SourceProjection {
                    receipt: claim::Receipt {
                        schema: 1,
                        claim: offer.claim,
                        source_operation: offer.source_operation,
                        selection: offer.selection,
                        frontier: frontier.frontier_data().into(),
                        snapshot: snapshot.clone(),
                        thread_id: params.thread_id.clone(),
                        task_tools_available: tools,
                        presentation: if candidate.is_primary_presentation() {
                            claim::Presentation::Primary
                        } else {
                            claim::Presentation::SideConversation
                        },
                    },
                    boundary,
                    side_display: StdMutex::new(None),
                    listener_cut: StdMutex::new(None),
                    candidate: candidate.clone(),
                    held,
                    frontier,
                    install,
                    ack: StdMutex::new(None),
                    observed: StdMutex::new(None),
                    released: StdMutex::new(None),
                });
                let work = Arc::new(Projection {
                    current_display: selection.history == claim::HistoryIntent::CurrentDisplay,
                    binding: Arc::downgrade(self),
                    receipt: Receipt {
                        frontier: source.receipt.frontier.clone(),
                        snapshot,
                        thread_id: params.thread_id.clone(),
                        task_tools_available: tools,
                    },
                    request_id,
                    client_name,
                    client_version,
                    origin,
                    primary: PrimaryResource::Candidate(candidate),
                    frontier: PhysicalFrontier::Source(source),
                    _context: context,
                    listener_started: AtomicBool::new(false),
                    uncertain: AtomicBool::new(false),
                    response_attempted: AtomicBool::new(false),
                    tail_finished: AtomicBool::new(false),
                    ack: StdMutex::new(None),
                    stage: StdMutex::new(None),
                    memory_history_mode: StdMutex::new(None),
                    result: watch::channel(None).0,
                });
                self.start_projection(work, params)
            }
            claim::Selection::Observe(receipt) | claim::Selection::Activate(receipt) => {
                let work = self
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .projections
                    .get(&receipt.snapshot)
                    .cloned()
                    .ok_or_else(|| {
                        invalid_request("actual Source projection receipt unavailable")
                    })?;
                let PhysicalFrontier::Source(source) = &work.frontier else {
                    return Err(invalid_request(
                        "R projection cannot accept a Source selection receipt",
                    ));
                };
                if receipt != source.receipt || !source.frontier.same_receipt(&origin) {
                    return Err(invalid_request(
                        "Source receipt belongs to another actual delivery",
                    ));
                }
                self.accept_activation(
                    request_id,
                    context,
                    origin,
                    work.receipt.clone(),
                    activation_phase,
                )
                .await
            }
        }
    }
}
impl ThreadRequestProcessor {
    pub(crate) async fn alab_source_selection_frontier(
        &self,
        request: ConnectionRequestId,
        context: RequestContext,
        params: ThreadResumeParams,
        claim: claim::Claim,
        client_name: Option<String>,
        client_version: Option<String>,
    ) -> Result<(), JSONRPCErrorError> {
        self.alab_retained_frontier
            .actual()?
            .handle_source(
                request,
                context,
                params,
                claim.0,
                client_name,
                client_version,
            )
            .await
    }
}

impl SourceProjection {
    pub(super) fn side_boundary(&self) -> Option<codex_core::RecordedSideBoundary> {
        self.boundary.clone()
    }
    pub(super) fn side_cut(
        &self,
    ) -> Result<Option<Arc<crate::thread_state::alab_side_boundary::Cut>>, JSONRPCErrorError> {
        let actual = self
            .listener_cut
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if self.boundary.is_some() && actual.is_none() {
            return Err(invalid_request("actual ordered Side listener cut missing"));
        }
        Ok(actual)
    }
    pub(super) fn store_side_cut(
        &self,
        cut: Arc<crate::thread_state::alab_side_boundary::Cut>,
    ) -> Result<(), JSONRPCErrorError> {
        let mut slot = self
            .listener_cut
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.as_ref().is_some_and(|old| !Arc::ptr_eq(old, &cut)) {
            return Err(invalid_request("actual ordered Side listener changed"));
        }
        *slot = Some(cut);
        Ok(())
    }
}

impl SourceProjection {
    pub(super) fn store_side_display(
        &self,
        data: claim::side_display::SideDisplayBoundary,
    ) -> Result<(), JSONRPCErrorError> {
        let boundary = self
            .boundary
            .as_ref()
            .ok_or_else(|| invalid_request("actual Side occurrence missing"))?;
        if !data.valid() || boundary.marker_data().token_data() != data.occurrence {
            return Err(invalid_request(
                "Side display data changed actual occurrence",
            ));
        }
        let mut slot = self
            .side_display
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.as_ref().is_some_and(|old| old != &data) {
            return Err(invalid_request(
                "Side display snapshot was already captured",
            ));
        }
        *slot = Some(data);
        Ok(())
    }
    pub(super) fn side_display(
        &self,
    ) -> Result<Option<claim::side_display::SideDisplayBoundary>, JSONRPCErrorError> {
        let data = self
            .side_display
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if self.boundary.is_some() && data.is_none() {
            return Err(invalid_request("actual Side display cut was not captured"));
        }
        Ok(data)
    }
}
