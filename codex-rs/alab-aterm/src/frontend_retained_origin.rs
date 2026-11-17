// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact original A receipt and actual backend controller; no public constructor.
//! This resource is not an InProcess origin or a replacement-Go observation.
use super::Controller;
use codex_alab_core::managed_auth::unavailable;
use codex_alab_core::managed_auth::valid_hex;
use serde::Deserialize;
use std::io;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::watch;

#[derive(Clone)]
pub struct AcceptedOrigin(Arc<Receipt>);
struct Receipt {
    controller: Arc<Controller>,
    attachment: [u8; 32],
    withdrawal: watch::Receiver<bool>,
}
/// A real Native observation tied to the SAME physical receipt. Service is
/// selection DATA only; existence/clone supplies no lasting currentness or Go.
#[derive(Clone)]
pub struct CurrentOrigin {
    receipt: AcceptedOrigin,
    service: String,
}
impl CurrentOrigin {
    pub fn service_data(&self) -> &str {
        &self.service
    }
    pub fn same_receipt(&self, origin: &AcceptedOrigin) -> bool {
        Arc::ptr_eq(&self.receipt.0, &origin.0)
    }
}
impl std::fmt::Debug for CurrentOrigin {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("CurrentOrigin(<same physical receipt; selection data only>)")
    }
}
impl std::fmt::Debug for AcceptedOrigin {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("AcceptedOrigin(<physical receipt; requires Native check>)")
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Current {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: crate::accounts::Session,
    state: String,
    attachment: String,
    service: String,
}
pub(super) struct WeakOrigin(Weak<Receipt>);
impl WeakOrigin {
    pub(super) fn upgrade(&self) -> Option<AcceptedOrigin> {
        self.0.upgrade().map(AcceptedOrigin)
    }
}
impl AcceptedOrigin {
    pub(super) fn downgrade(&self) -> WeakOrigin {
        WeakOrigin(Arc::downgrade(&self.0))
    }
    pub(super) fn attachment(&self) -> [u8; 32] {
        self.0.attachment
    }
    pub(super) fn controlled_by(&self, controller: &Arc<Controller>) -> bool {
        Arc::ptr_eq(&self.0.controller, controller)
    }
    pub(super) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Only the physical A receive path calls this, using the original bound
    /// owner/controller and nonce decoded from its actual inherited channel.
    pub(super) fn received(
        controller: Arc<Controller>,
        attachment: [u8; 32],
        withdrawal: watch::Receiver<bool>,
    ) -> Self {
        Self(Arc::new(Receipt {
            controller,
            attachment,
            withdrawal,
        }))
    }

    fn live(&self) -> io::Result<()> {
        if *self.0.withdrawal.borrow() || self.0.withdrawal.has_changed().is_err() {
            return Err(unavailable());
        }
        Ok(())
    }
    /// Requires the genuine Native handler: the actual constructor/control,
    /// backend SDK, same active A/F1 Received and current frontend on its actual
    /// borrowed PTY. Unsupported/unknown/withdrawn results stay failures.
    /// No label, Some-resource test, nonce echo alone, or Source bool substitutes.
    pub async fn check_current(&self) -> io::Result<()> {
        self.observe_current().await.map(|_| ())
    }
    /// Require the paired exact8-field Native reply. Never accept the old
    /// schema, a label-only answer, or a service guessed from Source state.
    pub async fn observe_current(&self) -> io::Result<CurrentOrigin> {
        self.observe_retained_current()
            .await
            .map_err(crate::source_member::ObservationError::into_io)
    }
    pub(super) async fn observe_retained_current(
        &self,
    ) -> Result<CurrentOrigin, crate::source_member::ObservationError> {
        use crate::source_member::ObservationError;
        self.live().map_err(ObservationError::Uncertain)?;
        let attachment = std::str::from_utf8(&self.0.attachment)
            .map_err(|_| ObservationError::Uncertain(unavailable()))?;
        if !valid_hex(attachment, 32) {
            return Err(ObservationError::Uncertain(unavailable()));
        }
        let operation = format!(
            "{:016x}{:016x}",
            codex_alab_core::native_source::connection_request_prefix(),
            codex_alab_core::native_source::connection_request_prefix()
        );
        let fields = format!(
            " member={} controller_operation={} attachment={attachment}",
            self.0.controller.member, self.0.controller.operation
        );
        let body = self
            .0
            .controller
            .transport
            .observe_retained(
                crate::source_member::RetainedQuery::Origin,
                &operation,
                &fields,
                &self.0.controller.member,
            )
            .await?;
        let decoded = (|| {
            let current: Current = serde_json::from_str(&body).map_err(|_| unavailable())?;
            if current.schema != 1
                || !valid_hex(&current.request, 32)
                || current.operation != operation
                || current.member != self.0.controller.member
                || current.session != self.0.controller.session
                || current.state != "current"
                || current.attachment != attachment
                || !valid_hex(&current.service, 32)
            {
                return Err(unavailable());
            }
            self.live()?;
            Ok(CurrentOrigin {
                receipt: self.clone(),
                service: current.service,
            })
        })();
        if decoded.is_err() {
            self.0.controller.transport.release();
        }
        decoded.map_err(ObservationError::Uncertain)
    }
}
