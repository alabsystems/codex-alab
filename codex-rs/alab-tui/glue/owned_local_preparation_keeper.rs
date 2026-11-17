// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual operation-bound original owner before App/session/native preflight.
//! Failed/Unknown retains the same payload. No Source or Native permission is
//! created by this local slot, its phases, or an unchanged original error.
use super::*;

pub(crate) struct CapturedOriginal {
    pub(super) record: Weak<ConstructionRecord>,
    pub(super) original: Arc<Payload<OriginalParts>>,
}
pub(crate) enum ExistingSlot {
    Preparing(Arc<CapturedOriginal>),
    Work {
        _captured: Arc<CapturedOriginal>,
        _work: Arc<Work>,
    },
    Failed(Arc<CapturedOriginal>),
}
impl CapturedOriginal {
    pub(crate) fn belongs_to(&self, record: &Arc<ConstructionRecord>) -> bool {
        self.record
            .upgrade()
            .is_some_and(|actual| Arc::ptr_eq(&actual, record))
    }
}
/// The actual primary original is already retained by its same Record before
/// this token is returned. Its destructor records Failed, never discards it.
pub(crate) struct PrimaryPreparation {
    record: Arc<ConstructionRecord>,
    captured: Arc<CapturedOriginal>,
    complete: bool,
}
pub(crate) struct SidePreparation {
    record: Arc<ConstructionRecord>,
    captured: Arc<CapturedOriginal>,
    complete: bool,
}
pub(crate) struct SideAttachmentPreparation {
    record: Arc<ConstructionRecord>,
    captured: Arc<CapturedOriginal>,
    complete: bool,
}

impl ConstructionRecord {
    pub(crate) fn capture_existing_primary(
        self: &Arc<Self>,
        mut original: AppServerStartedThread,
    ) -> std::result::Result<
        PrimaryPreparation,
        Box<ExistingPreparationFailure<AppServerStartedThread>>,
    > {
        if self.presentation_data() != ClientPresentation::Primary
            || !original
                .alab_construction
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, self))
        {
            return Err(Box::new(ExistingPreparationFailure {
                original,
                error: eyre!("primary result changed its actual construction allocation"),
            }));
        }
        let reservation = match self.reserve_existing_capture() {
            Ok(actual) => actual,
            Err(error) => return Err(Box::new(ExistingPreparationFailure { original, error })),
        };
        original.alab_construction = None;
        let captured = Arc::new(CapturedOriginal {
            record: Arc::downgrade(self),
            original: Arc::new(Payload::new(OriginalParts {
                started: original,
                presentation: ClientPresentation::Primary,
                side: None,
            })),
        });
        reservation.publish(captured.clone());
        Ok(PrimaryPreparation {
            record: self.clone(),
            captured,
            complete: false,
        })
    }
    /// Valid side callers retain all outer fields before any resource lookup.
    /// Admission refusal is typed ownership; it has no automatic Report cast.
    pub(crate) fn capture_existing_side(
        self: &Arc<Self>,
        original: SidebarFork,
    ) -> std::result::Result<SidePreparation, Box<ExistingPreparationFailure<SidebarFork>>> {
        if self.presentation_data() != ClientPresentation::SideConversation
            || !original
                .thread
                .alab_construction
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, self))
            || original.alab_retained_replay.is_some()
        {
            return Err(Box::new(ExistingPreparationFailure {
                original,
                error: eyre!("side result lost its original construction/presentation"),
            }));
        }
        let reservation = match self.reserve_existing_capture() {
            Ok(actual) => actual,
            Err(error) => return Err(Box::new(ExistingPreparationFailure { original, error })),
        };
        let SidebarFork {
            mut thread,
            boundary,
            inherited_profile,
            storage,
            legacy,
            alab_retained_replay: _,
        } = original;
        thread.alab_construction = None;
        let captured = Arc::new(CapturedOriginal {
            record: Arc::downgrade(self),
            original: Arc::new(Payload::new(OriginalParts {
                started: thread,
                presentation: ClientPresentation::SideConversation,
                side: Some(SideFields {
                    boundary,
                    inherited_profile,
                    storage,
                    legacy,
                }),
            })),
        });
        reservation.publish(captured.clone());
        Ok(SidePreparation {
            record: self.clone(),
            captured,
            complete: false,
        })
    }
    /// Actual original Resume/attachment result. Its pre-S1 caller declaration
    /// must already be SideConversation; no primary/side inference by path/id.
    pub(crate) fn capture_existing_side_attachment(
        self: &Arc<Self>,
        mut original: AppServerStartedThread,
    ) -> std::result::Result<
        SideAttachmentPreparation,
        Box<ExistingPreparationFailure<AppServerStartedThread>>,
    > {
        if self.presentation_data() != ClientPresentation::SideConversation
            || !original
                .alab_construction
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, self))
        {
            return Err(Box::new(ExistingPreparationFailure {
                original,
                error: eyre!("side attachment changed actual operation/presentation"),
            }));
        }
        let reservation = match self.reserve_existing_capture() {
            Ok(actual) => actual,
            Err(error) => return Err(Box::new(ExistingPreparationFailure { original, error })),
        };
        original.alab_construction = None;
        let captured = Arc::new(CapturedOriginal {
            record: Arc::downgrade(self),
            original: Arc::new(Payload::new(OriginalParts {
                started: original,
                presentation: ClientPresentation::SideConversation,
                side: None,
            })),
        });
        reservation.publish(captured.clone());
        Ok(SideAttachmentPreparation {
            record: self.clone(),
            captured,
            complete: false,
        })
    }
}
impl PrimaryPreparation {
    pub(crate) fn fail(mut self, error: color_eyre::eyre::Report) -> color_eyre::eyre::Report {
        self.record.fail_existing_capture(&self.captured);
        self.complete = true;
        error
    }
    pub(crate) fn prepare(mut self, session: &AppServerSession) -> Result<ExistingPrimary> {
        let work = Work::from_capture(session, &self.record, &self.captured)?;
        self.complete = true;
        Ok(ExistingPrimary {
            work,
            complete: false,
        })
    }
}
impl SidePreparation {
    pub(crate) fn fail(mut self, error: color_eyre::eyre::Report) -> color_eyre::eyre::Report {
        self.record.fail_existing_capture(&self.captured);
        self.complete = true;
        error
    }
    pub(crate) fn prepare(mut self, session: &AppServerSession) -> Result<ExistingSide> {
        let work = Work::from_capture(session, &self.record, &self.captured)?;
        self.complete = true;
        Ok(ExistingSide {
            work,
            complete: false,
        })
    }
}
impl SideAttachmentPreparation {
    pub(crate) fn fail(mut self, error: color_eyre::eyre::Report) -> color_eyre::eyre::Report {
        self.record.fail_existing_capture(&self.captured);
        self.complete = true;
        error
    }
    pub(crate) fn prepare(mut self, session: &AppServerSession) -> Result<ExistingSideAttachment> {
        let work = Work::from_capture(session, &self.record, &self.captured)?;
        self.complete = true;
        Ok(ExistingSideAttachment {
            work,
            complete: false,
        })
    }
}
impl Drop for SideAttachmentPreparation {
    fn drop(&mut self) {
        if !self.complete {
            self.record.fail_existing_capture(&self.captured);
        }
    }
}
impl Drop for PrimaryPreparation {
    fn drop(&mut self) {
        if !self.complete {
            self.record.fail_existing_capture(&self.captured);
        }
    }
}
impl Drop for SidePreparation {
    fn drop(&mut self) {
        if !self.complete {
            self.record.fail_existing_capture(&self.captured);
        }
    }
}

#[cfg(test)]
#[path = "owned_local_preparation_keeper_tests.rs"]
mod tests;
