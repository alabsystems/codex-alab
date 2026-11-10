// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Immutable native ACK history is separate from current same-grant readiness.
//! These types never assert completion of local Session resource participants.
use super::super::alab_native_source::NativeSourceScope;
use super::*;
use codex_alab_core::native_target::Applied;
use codex_alab_core::native_target::ResourceReceipt;
use codex_alab_core::native_target::ResourceReport;

pub struct DestinationAckObservation {
    manager: Arc<AuthManager>,
    census: Arc<DestinationRecord>,
    operation: String,
    assignment: String,
    report: ResourceReport,
    scope: NativeSourceScope,
}
pub struct DestinationAppliedProof {
    history: Applied,
    observed: DestinationAckObservation,
    current: NativeSourceScope,
}
impl DestinationEpoch {
    /// Capture actual cache/member/Ready before sending this immutable ACK.
    /// If owner refresh already advanced, reconcile the resource report first.
    pub async fn observe_native_ack(
        &self,
        receipt: &ResourceReceipt,
    ) -> std::io::Result<DestinationAckObservation> {
        self.validate()?;
        receipt
            .report()
            .validate_context(self.census.destination().context())?;
        let scope = self
            .manager
            .alab_destination_scope(Some(receipt.report()))
            .await?;
        self.validate()?;
        Ok(DestinationAckObservation {
            manager: self.manager.clone(),
            census: self.census.clone(),
            operation: receipt.operation().into(),
            assignment: self.census.destination().context().assignment().into(),
            report: receipt.report().clone(),
            scope,
        })
    }
    /// The private transport has already correlated Applied with its exact ACK.
    /// Retain that history; never substitute latest owner C into its old report.
    pub async fn confirm_native_applied(
        &self,
        observation: DestinationAckObservation,
        applied: Applied,
    ) -> std::io::Result<DestinationAppliedProof> {
        self.validate()?;
        if !Arc::ptr_eq(&observation.manager, &self.manager)
            || !Arc::ptr_eq(&observation.census, &self.census)
            || applied.operation() != observation.operation
            || applied.report() != &observation.report
        {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        let current = self.manager.alab_destination_scope(None).await?;
        if !current.continues_scope(&observation.scope) {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        self.validate()?;
        Ok(DestinationAppliedProof {
            history: applied,
            observed: observation,
            current,
        })
    }
}
impl DestinationAppliedProof {
    pub fn assignment(&self) -> &str {
        &self.observed.assignment
    }
    pub fn acknowledged_binding(&self) -> codex_alab_core::assignment::Binding {
        self.history.report().binding()
    }
    pub fn current_binding(&self) -> codex_alab_core::assignment::Binding {
        self.current.binding()
    }
    /// A legitimate refresh may advance C/full actual4 after ACK. Revisit local
    /// resources at this real new snapshot; keep original ACK history unchanged.
    pub async fn reconcile(&mut self, context: &DestinationResourceContext) -> std::io::Result<()> {
        context.validate()?;
        if !Arc::ptr_eq(&context.manager, &self.observed.manager)
            || !Arc::ptr_eq(&context.census, &self.observed.census)
        {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        let current = context.manager.alab_destination_scope(None).await?;
        if !current.continues_scope(&self.observed.scope) || !current.continues_scope(&self.current)
        {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        context.validate()?;
        self.current = current;
        Ok(())
    }
}
macro_rules! redacted {
    ($name:ident) => {
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "(<private>)"))
            }
        }
    };
}
redacted!(DestinationAckObservation);
redacted!(DestinationAppliedProof);
