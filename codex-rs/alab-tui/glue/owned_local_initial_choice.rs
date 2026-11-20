// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original UI choice and backend-issued claim DATA only. Controller/F1/Go,
//! physical origin, Core identity and actual UI frontier remain independent.
use super::*;
use codex_app_server_client::InitialAction as Action;
use codex_app_server_client::InitialConstructionCheckpoint;
use codex_app_server_client::InitialConstructionOperation;
use codex_app_server_client::InitialSessionSelection;
use codex_app_server_client::NextSelection;
use codex_app_server_protocol::ThreadStartParams;

struct Chosen {
    action: Action,
    selection: InitialSessionSelection,
    operation: InitialConstructionOperation,
}
impl Chosen {
    fn new(action: Action, selection: InitialSessionSelection) -> io::Result<Self> {
        let operation = InitialConstructionOperation::new(selection.clone(), action.clone())?;
        Ok(Self {
            action,
            selection,
            operation,
        })
    }
}
#[derive(Default)]
pub(super) struct State {
    current: Option<Chosen>,
    next: Option<NextSelection>,
    constructions: Vec<Arc<ConstructionRecord>>,
    admissions: std::collections::BTreeSet<String>,
}
impl State {
    fn admission_full(&self) -> bool {
        if self.constructions.len() + self.admissions.len() < MAX_CONSTRUCTIONS {
            return false;
        }
        let active = self
            .constructions
            .iter()
            .filter(|record| {
                !matches!(
                    *record.output.lock().unwrap_or_else(PoisonError::into_inner),
                    Output::SavedResumeHistory
                )
            })
            .count();
        active + self.admissions.len() >= MAX_CONSTRUCTIONS
    }
}
fn changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "initial choice requires its actual backend-issued selection claim",
    )
}
impl OwnedLocalTarget {
    /// Called only at the original explicit Start/Fork/Resume/picker boundary.
    /// The selected protocol ID is frozen request DATA, never a Core lookup or
    /// permission witness. C2 runtime reconciliation must use its actual bridge.
    pub(crate) fn original_initial_choice(
        &self,
        action: Action,
        prospective_create: Option<ThreadStartParams>,
    ) -> io::Result<Option<InitialSessionSelection>> {
        let snapshot = self.snapshot()?;
        let initial = snapshot
            .initial_session_claim
            .as_ref()
            .ok_or_else(changed)?;
        let mut state = self
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(current) = &state.current {
            if let Some(next) = &state.next {
                if !next.valid() || next.previous_claim != current.selection.claim {
                    return Err(changed());
                }
                let selection = InitialSessionSelection {
                    claim: next.claim.clone(),
                    prospective_create,
                };
                state.current = Some(Chosen::new(action, selection.clone())?);
                state.next = None;
                return Ok(Some(selection));
            }
            if current.action != action
                || current.selection.prospective_create != prospective_create
            {
                return Err(changed());
            }
            return Ok(Some(current.selection.clone()));
        }
        let selection = InitialSessionSelection {
            claim: initial.clone(),
            prospective_create,
        };
        if !selection.valid() {
            return Err(changed());
        }
        state.current = Some(Chosen::new(action, selection.clone())?);
        Ok(Some(selection))
    }
    /// The target retains the actual SDK operation before its original future
    /// is polled. Dropping an observer cannot replace it with a new scope.
    pub(crate) fn original_initial_operation(
        &self,
        selection: &InitialSessionSelection,
    ) -> io::Result<InitialConstructionOperation> {
        let state = self
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let current = state.current.as_ref().ok_or_else(changed)?;
        if &current.selection != selection {
            return Err(changed());
        }
        Ok(current.operation.clone())
    }
    /// Immutable checkpoint DATA from the SAME retained operation. This is
    /// neither a Native receipt nor evidence of Core/UI installation.
    pub(crate) fn initial_operation_checkpoint(&self) -> Option<InitialConstructionCheckpoint> {
        self.0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .current
            .as_ref()
            .map(|current| current.operation.checkpoint())
    }
    /// Restore the exact original checkpoint without issuing a claim, polling
    /// its original work, or replacing any already-owned choice. The entered
    /// target and actual Source/native frontier must still be established.
    pub(crate) fn restore_initial_operation(
        &self,
        checkpoint: InitialConstructionCheckpoint,
    ) -> io::Result<()> {
        let operation = InitialConstructionOperation::restore(checkpoint)?;
        let action = operation.action_data().cloned().ok_or_else(changed)?;
        let selection = operation.selection_data().clone();
        let next = operation.next_selection();
        let mut state = self
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if state.current.is_some() || state.next.is_some() {
            return Err(changed());
        }
        state.current = Some(Chosen {
            action,
            selection,
            operation,
        });
        state.next = next;
        Ok(())
    }
    /// Only records the exact response's closed Source offer. No UI choice,
    /// retry, controller issue, Native permission or primary swap happens here.
    pub(crate) fn observe_next_initial_choice(&self, next: NextSelection) -> io::Result<()> {
        let mut state = self
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let current = state.current.as_ref().ok_or_else(changed)?;
        if !next.valid() || next.previous_claim != current.selection.claim {
            return Err(changed());
        }
        if let Some(previous) = &state.next
            && previous != &next
        {
            return Err(changed());
        }
        state.next = Some(next);
        Ok(())
    }
}

use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::alab_retained_ui::ConstructionOwner;
use codex_app_server_client::ClientConstructionOperation;
use codex_app_server_client::ClientConstructionSelection;
use codex_app_server_client::ClientPresentation;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

enum Output {
    Running,
    Converted(Box<AppServerStartedThread>),
    Failed,
    SavedResumeHistory,
    Unknown,
    Applied,
}

/// Actual caller custody. Its weak target/delivery references avoid a cycle
/// with the target's registry. It contains no Native permission or UI receipt.
pub(crate) struct ConstructionRecord {
    target: std::sync::Weak<Selection>,
    delivery: std::sync::Weak<DeliveryOwner>,
    owner: ConstructionOwner,
    presentation: ClientPresentation,
    output: Mutex<Output>,
    side_creation: Mutex<Option<Arc<crate::app_server_session::alab_side_creation::SideCreation>>>,
    existing_original: Mutex<Option<crate::app_server_session::alab_retained_ui::original_resume::existing::original::preparation::ExistingSlot>>,
    // One refused PRIMARY DTO per actual operation. Separate from accepted
    // conversion/Unknown status and from its live preparation payload.
    rejected_primary: Mutex<Option<AppServerStartedThread>>,
}
impl std::fmt::Debug for ConstructionRecord {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("ConstructionRecord(<original operation/result custody; no grant>)")
    }
}
impl FrontendTenure {
    pub(crate) fn begin_original_construction(
        &self,
        action: Action,
        presentation: ClientPresentation,
        prospective: Option<ThreadStartParams>,
        saved: Option<(uuid::Uuid, ThreadId)>,
    ) -> io::Result<ConstructionObservation> {
        if self.delivery.closing() {
            return Err(unavailable());
        }
        if let Some((_, child)) = saved
            && (presentation != ClientPresentation::SideConversation
                || !matches!(&action, Action::Resume(id) if id == &child.to_string()))
        {
            return Err(changed());
        }
        let admission = ConstructionAdmission::reserve(&self.target.0)?;
        let initial = {
            let state = self
                .target
                .0
                .initial_choice
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            presentation == ClientPresentation::Primary
                && state
                    .current
                    .as_ref()
                    .is_none_or(|chosen| chosen.operation.observed_result().is_none())
        };
        let owner = if initial {
            let selection = self
                .target
                .original_initial_choice(action, prospective)?
                .ok_or_else(changed)?;
            ConstructionOwner::Initial(self.target.original_initial_operation(&selection)?)
        } else {
            let operation = match action {
                Action::Start => ClientConstructionOperation::for_start(),
                Action::Fork(_) | Action::Resume(_) => {
                    let selection = ClientConstructionSelection {
                        prospective_create: prospective.ok_or_else(changed)?,
                        presentation,
                        prepared: None,
                    };
                    if let Some((opening, child)) = saved {
                        ClientConstructionOperation::for_saved_resume(
                            selection,
                            &opening.simple().to_string(),
                            &child.to_string(),
                        )?
                    } else {
                        ClientConstructionOperation::new(selection)?
                    }
                }
            };
            ConstructionOwner::Later(operation)
        };
        let record = Arc::new(ConstructionRecord {
            target: Arc::downgrade(&self.target.0),
            delivery: Arc::downgrade(&self.delivery),
            owner,
            presentation,
            output: Mutex::new(Output::Running),
            side_creation: Mutex::new(None),
            existing_original: Mutex::new(None),
            rejected_primary: Mutex::new(None),
        });
        // The cancellation guard exists before registry publication and before
        // any returned future can be discarded without its first poll.
        let observation = ConstructionObservation {
            actual: record.clone(),
            complete: false,
        };
        admission.publish(record)?;
        Ok(observation)
    }
}
pub(crate) struct ExistingCaptureReservation<'a> {
    slot: std::sync::MutexGuard<'a, Option<crate::app_server_session::alab_retained_ui::original_resume::existing::original::preparation::ExistingSlot>>,
}
impl ExistingCaptureReservation<'_> {
    pub(crate) fn publish(
        mut self,
        captured: Arc<crate::app_server_session::alab_retained_ui::original_resume::existing::original::preparation::CapturedOriginal>,
    ) {
        *self.slot = Some(crate::app_server_session::alab_retained_ui::original_resume::existing::original::preparation::ExistingSlot::Preparing(captured));
    }
}
impl ConstructionRecord {
    #[cfg(test)]
    pub(crate) fn preparation_fixture() -> Result<(OwnedLocalTarget, Arc<Self>)> {
        let target = OwnedLocalTarget::new("a".repeat(32), None, None);
        let record = construction_tests::fixture(&target)?;
        Ok((target, record))
    }
    #[cfg(test)]
    pub(crate) fn side_preparation_fixture() -> Result<(OwnedLocalTarget, Arc<Self>)> {
        let target = OwnedLocalTarget::new("a".repeat(32), None, None);
        let record = construction_tests::fixture_with_presentation(
            &target,
            ClientPresentation::SideConversation,
        )?;
        Ok((target, record))
    }
    #[cfg(test)]
    pub(crate) fn rejected_primary_for_test(&self) -> Option<AppServerStartedThread> {
        self.rejected_primary
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    #[cfg(test)]
    pub(crate) fn converted_cache_for_test(&self, mut original: AppServerStartedThread) {
        original.alab_construction = None;
        *self.output.lock().unwrap_or_else(PoisonError::into_inner) =
            Output::Converted(Box::new(original));
    }
    pub(crate) fn reserve_existing_capture(&self) -> Result<ExistingCaptureReservation<'_>> {
        let slot = self
            .existing_original
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            bail!("original operation already owns its preparation");
        }
        Ok(ExistingCaptureReservation { slot })
    }
    pub(crate) fn start_existing_capture(
        self: &Arc<Self>,
        captured: &Arc<crate::app_server_session::alab_retained_ui::original_resume::existing::original::preparation::CapturedOriginal>,
        work: Arc<
            crate::app_server_session::alab_retained_ui::original_resume::existing::original::Work,
        >,
    ) -> Result<()> {
        use crate::app_server_session::alab_retained_ui::original_resume::existing::original::preparation::ExistingSlot;
        let retired = {
            let mut slot = self
                .existing_original
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !captured.belongs_to(self)
                || !matches!(&*slot, Some(ExistingSlot::Preparing(actual)) if Arc::ptr_eq(actual, captured))
            {
                bail!("original preparation failed/changed before Work publication");
            }
            slot.replace(ExistingSlot::Work {
                _captured: captured.clone(),
                _work: work,
            })
        };
        drop(retired);
        Ok(())
    }
    pub(crate) fn fail_existing_capture(
        self: &Arc<Self>,
        captured: &Arc<crate::app_server_session::alab_retained_ui::original_resume::existing::original::preparation::CapturedOriginal>,
    ) {
        use crate::app_server_session::alab_retained_ui::original_resume::existing::original::preparation::ExistingSlot;
        let retired = {
            let mut slot = self
                .existing_original
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !captured.belongs_to(self) {
                return;
            }
            match &*slot {
                Some(ExistingSlot::Preparing(actual)) if Arc::ptr_eq(actual, captured) => {
                    slot.replace(ExistingSlot::Failed(captured.clone()))
                }
                Some(ExistingSlot::Failed(actual)) if Arc::ptr_eq(actual, captured) => None,
                _ => return, // Never overwrite/drop another or a live Work payload.
            }
        };
        drop(retired);
        self.original_projection_unknown();
    }
    /// Exactly one refused PRIMARY DTO per actual operation. It holds no
    /// running resource after removing its Record backlink. Replacing this
    /// rejected copy never changes accepted conversion/Unknown status or the
    /// live preparation slot, so it cannot sabotage a real pending commit.
    pub(crate) fn retain_rejected_primary(
        &self,
        mut original: AppServerStartedThread,
        error: color_eyre::eyre::Report,
    ) -> color_eyre::eyre::Report {
        original.alab_construction = None;
        let retired = self
            .rejected_primary
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(original);
        drop(retired); // only prior rejected DTO, outside its lock; no Work/status touched.
        error
    }
    pub(crate) fn original_projection_unknown(&self) {
        let retired = {
            let mut output = self.output.lock().unwrap_or_else(PoisonError::into_inner);
            std::mem::replace(&mut *output, Output::Unknown)
        };
        drop(retired);
    }
    pub(crate) fn owner_for_projection(self: &Arc<Self>) -> ConstructionOwner {
        ConstructionOwner::Caller(self.clone())
    }
    /// Immutable actual caller presentation DATA recorded before preparation/S1.
    /// This is neither a Native role nor a selected-primary installation grant.
    pub(crate) fn presentation_data(&self) -> ClientPresentation {
        self.presentation
    }
    pub(crate) fn prepared_offer(
        &self,
    ) -> Option<codex_app_server_client::ClientConstructionOffer> {
        self.owner.prepared_offer()
    }
    pub(crate) fn checkpoint(
        &self,
    ) -> crate::app_server_session::alab_retained_ui::ConstructionCheckpoint {
        self.owner.checkpoint()
    }
    pub(crate) fn belongs_to(&self, tenure: &FrontendTenure) -> bool {
        self.target
            .upgrade()
            .is_some_and(|actual| Arc::ptr_eq(&actual, &tenure.target.0))
            && self
                .delivery
                .upgrade()
                .is_some_and(|actual| Arc::ptr_eq(&actual, &tenure.delivery))
    }
    pub(crate) fn original_started(&self) -> Option<AppServerStartedThread> {
        match &*self.output.lock().unwrap_or_else(PoisonError::into_inner) {
            Output::Converted(actual) => Some((**actual).clone()),
            _ => None,
        }
    }
}
pub(crate) struct ConstructionObservation {
    actual: Arc<ConstructionRecord>,
    complete: bool,
}
impl ConstructionObservation {
    pub(crate) fn saved_resume_record(&self) -> Arc<ConstructionRecord> {
        self.actual.clone()
    }
    pub(crate) async fn run(
        mut self,
        original: impl std::future::Future<Output = Result<AppServerStartedThread>>,
    ) -> Result<AppServerStartedThread> {
        let actual = self.actual.clone();
        let output = match &actual.owner {
            ConstructionOwner::Initial(actual) => actual.run(original).await,
            ConstructionOwner::Later(actual) => actual.run(original).await,
            ConstructionOwner::Caller(_) => bail!("construction caller cannot wrap another caller"),
        };
        match output {
            Ok(mut started) => {
                if started.alab_construction.is_some() {
                    bail!("original result already belongs to another construction");
                }
                let observed = match &actual.owner {
                    ConstructionOwner::Initial(actual) => actual
                        .observed_result()
                        .map(|result| (result.thread_id, result.backend_task_tools)),
                    ConstructionOwner::Later(actual) => actual
                        .observed_result()
                        .map(|result| (result.thread_id, result.backend_task_tools)),
                    ConstructionOwner::Caller(_) => None,
                }
                .ok_or_else(|| eyre!("original construction has no actual response observation"))?;
                if observed.0 != started.session.thread_id.to_string() {
                    bail!("original construction changed its converted thread");
                }
                started.task_tools_available = observed.1;
                // Store the WHOLE original converted result before adding its
                // ownership link. The stored clone has no back-reference.
                *actual.output.lock().unwrap_or_else(PoisonError::into_inner) =
                    Output::Converted(Box::new(started.clone()));
                started.alab_construction = Some(actual.clone());
                self.complete = true;
                Ok(started)
            }
            Err(error) => {
                let next_recorded = match &actual.owner {
                    ConstructionOwner::Initial(owner) => match owner.next_selection() {
                        Some(next) => actual.target.upgrade().is_some_and(|target| {
                            OwnedLocalTarget(target)
                                .observe_next_initial_choice(next)
                                .is_ok()
                        }),
                        None => true,
                    },
                    _ => true,
                };
                *actual.output.lock().unwrap_or_else(PoisonError::into_inner) = if next_recorded {
                    Output::Failed
                } else {
                    Output::Unknown
                };
                let no_original_request = match &actual.owner {
                    ConstructionOwner::Initial(owner) => !owner.original_request_started_data(),
                    ConstructionOwner::Later(owner) => !owner.original_request_started_data(),
                    ConstructionOwner::Caller(_) => false,
                };
                if no_original_request {
                    // The original validation/preflight failed before this
                    // operation froze any wire. No Native retirement is claimed.
                    actual.forget_before_request();
                }
                self.complete = true;
                Err(error)
            }
        }
    }
}
impl ConstructionRecord {
    /// Invoked only by the actual Pending commit after independent Native UI,
    /// Source activation and delivery completion. This retires local custody,
    /// not a Native/Source Hold, callback or effect.
    pub(crate) fn applied(self: &Arc<Self>, tenure: &FrontendTenure) -> Result<()> {
        if !self.belongs_to(tenure) {
            bail!("construction application changed physical delivery");
        }
        {
            let mut output = self.output.lock().unwrap_or_else(PoisonError::into_inner);
            if !matches!(*output, Output::Converted(_)) {
                bail!("original construction result was not observed");
            }
            *output = Output::Applied;
        }
        self.forget_local_record();
        Ok(())
    }
    pub(crate) fn saved_resume_query(
        &self,
        child: ThreadId,
    ) -> Option<codex_app_server_client::SavedResumeQuery> {
        let ConstructionOwner::Later(operation) = &self.owner else {
            return None;
        };
        operation.saved_resume_query(&child.to_string()).ok()
    }
    /// The chooser retains this failed caller as historical data. It remains
    /// present for handoff but can no longer run or consume live admission.
    pub(crate) fn retain_saved_resume_history(&self) {
        let mut output = self.output.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(*output, Output::Failed) {
            *output = Output::SavedResumeHistory;
        }
    }
    /// A terminal history read releases only this frontend record. It grants
    /// no Native settlement, subscription effect or UI installation.
    pub(crate) fn finish_saved_resume_history(self: &Arc<Self>) {
        let historical = matches!(
            *self.output.lock().unwrap_or_else(PoisonError::into_inner),
            Output::SavedResumeHistory
        );
        if historical {
            self.forget_local_record();
        }
    }
    fn forget_local_record(self: &Arc<Self>) {
        if let Some(target) = self.target.upgrade() {
            target
                .initial_choice
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .constructions
                .retain(|record| !Arc::ptr_eq(record, self));
        }
    }
    fn forget_before_request(self: &Arc<Self>) {
        let Some(target) = self.target.upgrade() else {
            return;
        };
        let mut state = target
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state
            .constructions
            .retain(|record| !Arc::ptr_eq(record, self));
        if let ConstructionOwner::Initial(owner) = &self.owner
            && state
                .current
                .as_ref()
                .is_some_and(|current| current.operation.same_operation(owner))
            && !state.constructions.iter().any(|record| {
                matches!(&record.owner,
                ConstructionOwner::Initial(other) if other.same_operation(owner))
            })
        {
            // This original preflight returned an error before any request
            // freeze, and no other caller still owns this initial attempt.
            // Keep the same backend claim available for the next real choice.
            state.current = None;
            state.next = None;
        }
    }
}
impl Drop for ConstructionObservation {
    fn drop(&mut self) {
        if !self.complete {
            *self
                .actual
                .output
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Output::Unknown;
        }
    }
}

const MAX_CONSTRUCTIONS: usize = 128;
struct ConstructionAdmission {
    target: Arc<Selection>,
    id: String,
    published: bool,
}
impl ConstructionAdmission {
    fn reserve(target: &Arc<Selection>) -> io::Result<Self> {
        let id = operation();
        let mut state = target
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if state.admission_full() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "original construction custody capacity unavailable",
            ));
        }
        if !state.admissions.insert(id.clone()) {
            return Err(changed());
        }
        Ok(Self {
            target: target.clone(),
            id,
            published: false,
        })
    }
    fn publish(mut self, record: Arc<ConstructionRecord>) -> io::Result<()> {
        {
            let mut state = self
                .target
                .initial_choice
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !state.admissions.remove(&self.id) {
                return Err(changed());
            }
            state.constructions.push(record);
            self.published = true;
        }
        Ok(())
    }
}
impl Drop for ConstructionAdmission {
    fn drop(&mut self) {
        if !self.published {
            self.target
                .initial_choice
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .admissions
                .remove(&self.id);
        }
    }
}

#[cfg(test)]
#[path = "owned_local_construction_tests.rs"]
mod construction_tests;

impl ConstructionRecord {
    pub(crate) fn capture_side_creation(
        self: &Arc<Self>,
        original: AppServerStartedThread,
        storage: crate::app_server_session::SideChatStorage,
        inherited_profile: bool,
        legacy: bool,
        items: Vec<serde_json::Value>,
    ) -> Result<Arc<crate::app_server_session::alab_side_creation::SideCreation>> {
        if self.presentation != ClientPresentation::SideConversation {
            bail!("original caller is not Side presentation")
        }
        let ConstructionOwner::Later(operation) = &self.owner else {
            bail!("Side fork must retain its actual later construction owner")
        };
        let mut slot = self
            .side_creation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(actual) = slot.as_ref() {
            if !actual.matches(&original, storage, inherited_profile, legacy, &items) {
                bail!("pending Side creation changed original payload or outer fields")
            }
            return Ok(actual.clone());
        }
        let actual = crate::app_server_session::alab_side_creation::SideCreation::capture(
            self,
            operation.clone(),
            original,
            storage,
            inherited_profile,
            legacy,
            items,
        );
        *slot = Some(actual.clone());
        drop(slot); // Whole original payload is retained before fallible close DATA admission.
        if let Some(data) = operation.side_view_data() {
            OwnedLocalTarget(
                self.target
                    .upgrade()
                    .ok_or_else(|| eyre!("actual Side target owner lost"))?,
            )
            .retain_side_view_data(data)?;
        }
        Ok(actual)
    }
    pub(crate) fn side_creation(
        &self,
    ) -> Option<Arc<crate::app_server_session::alab_side_creation::SideCreation>> {
        self.side_creation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub(crate) fn restore_side_creation(
        self: &Arc<Self>,
        data: crate::app_server_session::alab_side_creation::SideCreationCheckpoint,
    ) -> Result<()> {
        if self.presentation != ClientPresentation::SideConversation {
            bail!("restored Side operation changed original presentation")
        }
        let ConstructionOwner::Later(operation) = &self.owner else {
            bail!("restored Side construction has no actual owner")
        };
        let actual = crate::app_server_session::alab_side_creation::SideCreation::restore(
            self,
            operation.clone(),
            data,
        )?;
        let mut slot = self
            .side_creation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            bail!("actual Side creation already owns its payload")
        }
        *slot = Some(actual);
        Ok(())
    }
}
impl OwnedLocalTarget {
    pub(crate) fn pending_side_creation_checkpoints(
        &self,
    ) -> Vec<crate::app_server_session::alab_side_creation::SideCreationCheckpoint> {
        let records = self
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .constructions
            .clone();
        records
            .into_iter()
            .filter_map(|record| record.side_creation())
            .map(|actual| actual.checkpoint())
            .collect()
    }
}

impl FrontendTenure {
    /// Local UI reconstruction on the ACTUAL C2 delivery. Caller retains the
    /// whole entered E1 owner; this constructs no Core/Native permission. Same
    /// restored operation is installed before any original injection replay.
    pub(crate) fn restore_pending_side_creation(
        &self,
        data: crate::app_server_session::alab_side_creation::SideCreationCheckpoint,
        operation: ClientConstructionOperation,
    ) -> Result<Arc<ConstructionRecord>> {
        if self.delivery.closing() {
            bail!("actual C2 delivery is closing")
        }
        let original = data.original_started_data()?;
        let mut state = self
            .target
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if state.admission_full() {
            bail!("original construction custody capacity unavailable")
        }
        if state
            .constructions
            .iter()
            .any(|record| record.prepared_offer() == operation.prepared_offer())
        {
            bail!("actual Side operation already has a caller")
        }
        let record = Arc::new(ConstructionRecord {
            target: Arc::downgrade(&self.target.0),
            delivery: Arc::downgrade(&self.delivery),
            owner: ConstructionOwner::Later(operation.clone()),
            presentation: ClientPresentation::SideConversation,
            output: Mutex::new(Output::Converted(Box::new(original))),
            existing_original: Mutex::new(None),
            rejected_primary: Mutex::new(None),
            side_creation: Mutex::new(None),
        });
        let side = crate::app_server_session::alab_side_creation::SideCreation::restore(
            &record, operation, data,
        )?;
        *record
            .side_creation
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(side);
        state.constructions.push(record.clone());
        Ok(record)
    }
}
