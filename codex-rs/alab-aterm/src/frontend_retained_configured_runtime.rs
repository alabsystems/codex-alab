// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Genuine physical runtime receipts for the configured/empty-selected-slot
//! consumer. These opaque views deliberately expose NO scheduling release.
//! Source must own its actual original ConfigRead result and unchanged Empty
//! slot. Frontend UI acknowledgment still requires its real startup view/input.
use super::*;

#[derive(Clone)]
pub struct ConfiguredRuntimeFrontier(RuntimeFrontier);
#[derive(Clone)]
pub struct ConfiguredRuntimeAck {
    actual: RuntimeAck,
    frontier: ConfiguredRuntimeFrontier,
}
#[derive(Clone)]
pub struct ConfiguredRuntimeActivation(RuntimeActivation);
#[derive(Clone)]
pub struct ConfiguredRuntimeCommit(RuntimeCommit);

impl ConfiguredRuntimeFrontier {
    pub(in crate::frontend_control) fn from_actual(actual: RuntimeFrontier) -> Self {
        Self(actual)
    }
    pub fn frontier_data(&self) -> &str {
        self.0.frontier_data()
    }
    pub fn go_data(&self) -> Option<&str> {
        self.0.go_data()
    }
    pub fn goal_gate(&self) -> Gate {
        self.0.goal_gate()
    }
    pub fn same_receipt(&self, origin: &AcceptedOrigin) -> bool {
        self.0.same_receipt(origin)
    }

    /// BACKEND projection acknowledgment only, after the original config/read
    /// and the actual unchanged Empty selected-slot check. No Core/history is
    /// invented. This neither acknowledges frontend UI nor releases a Hold.
    /// The Source caller rechecks its real slot after this last Native await.
    pub async fn acknowledge_configured(
        &self,
        origin: &AcceptedOrigin,
    ) -> Result<ConfiguredRuntimeAck, RuntimeObservationError> {
        let actual = self.0.acknowledge_projection(origin).await?;
        Ok(ConfiguredRuntimeAck {
            actual,
            frontier: self.clone(),
        })
    }
}

impl ConfiguredRuntimeAck {
    pub fn same_frontier(&self, actual: &ConfiguredRuntimeFrontier) -> bool {
        self.actual.same_frontier(&actual.0)
    }
    /// The caller has observed the genuine startup-view/draw/input Native UI
    /// ACK on the SAME final physical frontend. This remains an observation,
    /// not a lasting role/currentness grant or permission to release Source.
    pub async fn observe_activation(
        &self,
    ) -> Result<ConfiguredRuntimeActivation, RuntimeObservationError> {
        Ok(ConfiguredRuntimeActivation(
            self.actual.observe_activation().await?,
        ))
    }
    /// Replacement requires actual App promotion/currentness between the
    /// Native UI observation and this final query. Initial birth has no fake
    /// promotion. Even the successful result CANNOT open automatic work.
    pub async fn observe_commit(
        &self,
        observed: &ConfiguredRuntimeActivation,
    ) -> Result<ConfiguredRuntimeCommit, RuntimeObservationError> {
        if !observed.same_frontier(&self.frontier) {
            return Err(unavailable().into());
        }
        Ok(ConfiguredRuntimeCommit(
            self.actual.observe_commit(&observed.0).await?,
        ))
    }
    pub async fn pending_hold(&self) -> io::Result<PendingHold> {
        self.actual.pending_hold().await
    }
    /// Only the continuously CLOSED same-Hold transfer is available here.
    /// The actual Source EmptyGuard is created inside this callback after the
    /// SDK mutation semaphore; it never crosses an await or grants Native use.
    pub async fn transfer_closed_hold<G>(
        &self,
        committed: &ConfiguredRuntimeCommit,
        pending: &PendingHold,
        unchanged_empty: impl FnOnce() -> io::Result<G>,
    ) -> io::Result<()> {
        if !committed.same_frontier(&self.frontier) {
            return Err(unavailable());
        }
        self.actual
            .transfer_closed_hold(&committed.0, pending, unchanged_empty)
            .await
    }
}
impl ConfiguredRuntimeActivation {
    pub fn same_frontier(&self, actual: &ConfiguredRuntimeFrontier) -> bool {
        self.0.same_frontier(&actual.0)
    }
}
impl ConfiguredRuntimeCommit {
    pub fn same_frontier(&self, actual: &ConfiguredRuntimeFrontier) -> bool {
        self.0.same_frontier(&actual.0)
    }
}
macro_rules! opaque_debug {
    ($($kind:ty),* $(,)?) => {$ (
        impl std::fmt::Debug for $kind {
            fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                out.write_str(concat!(stringify!($kind), "(<actual physical/Native observation; no Source release>)"))
            }
        }
    )* };
}
opaque_debug!(
    ConfiguredRuntimeFrontier,
    ConfiguredRuntimeAck,
    ConfiguredRuntimeActivation,
    ConfiguredRuntimeCommit
);
