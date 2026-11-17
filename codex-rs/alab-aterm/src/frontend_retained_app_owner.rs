// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Native App promotion for the SAME replacement Go/UI receipt. This
//! keeps constructor custody independent of its old backend service. It grants
//! no Source/model permission, SDK reader role or all-effects settlement.
use super::FrontendFrontier;
use super::FrontendUiAck;
use super::operation;
use crate::accounts::Session;
use crate::source_member::ObservationError;
use crate::source_member::RetainedQuery;
use codex_alab_core::managed_auth::unavailable;
use codex_alab_core::managed_auth::valid_hex;
use serde::Deserialize;
use std::io;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Promotion {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: Session,
    service: String,
    go: String,
    frontier: String,
    state: String,
    app_owner: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Current {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: Session,
    state: String,
    app_owner: String,
}
/// Opaque actual promotion receipt, retaining its SAME original F1 controller.
/// Presence/clone is historical custody; check_current remains a real query.
#[derive(Clone)]
pub struct AppFrontendOwner {
    frontier: FrontendFrontier,
    native: std::sync::Arc<Promotion>,
}
impl std::fmt::Debug for AppFrontendOwner {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("AppFrontendOwner(<actual Native replacement owner; requires current query>)")
    }
}
impl FrontendFrontier {
    /// The caller retains this result on the SAME physical frontend tenure
    /// BEFORE asking Source to remove its closed scheduling inhibitor.
    pub async fn promote_in_app(&self, ui: &FrontendUiAck) -> io::Result<AppFrontendOwner> {
        if !ui.same_frontier(self) {
            return Err(unavailable());
        }
        let original = self.0.native.fields();
        let go = original.go.ok_or_else(unavailable)?; // Initial creator is a distinct route.
        let context = &self.0.context;
        let fields = context.fields(original.service, Some(go), Some(original.frontier));
        let body = context
            .transport
            .call(
                "controller-retained-app-promote",
                &self.0.app_operation,
                &fields,
                Some(&context.member),
            )
            .await?;
        let checked = (|| {
            let value: Promotion = serde_json::from_str(&body).map_err(|_| unavailable())?;
            if value.schema != 1
                || !valid_hex(&value.request, 32)
                || value.operation != self.0.app_operation
                || value.member != context.member
                || value.session != context.session
                || value.service != original.service
                || value.go != go
                || value.frontier != original.frontier
                || value.state != "app-current"
                || !valid_hex(&value.app_owner, 32)
            {
                return Err(unavailable());
            }
            Ok(value)
        })();
        if checked.is_err() {
            context.transport.release();
        }
        Ok(AppFrontendOwner {
            frontier: self.clone(),
            native: std::sync::Arc::new(checked?),
        })
    }
}
impl AppFrontendOwner {
    /// Resource identity only, never a cached Native currentness boolean.
    pub fn same_frontier(&self, actual: &FrontendFrontier) -> bool {
        std::sync::Arc::ptr_eq(&self.frontier.0, &actual.0)
    }
    /// Native rechecks the exact constructor/exec generation, App lifetime,
    /// SAME actual session endpoint, controller and current foreground. An
    /// authenticated negative denies this query without closing that channel;
    /// IO/auth/protocol uncertainty closes it and never permits reconnect.
    pub async fn check_current(&self) -> io::Result<()> {
        let context = &self.frontier.0.context;
        let operation = operation();
        let fields = format!(
            " member={} controller_operation={} app_owner={}",
            context.member, context.controller_operation, self.native.app_owner
        );
        let body = context
            .transport
            .observe_retained(
                RetainedQuery::AppCurrent,
                &operation,
                &fields,
                &context.member,
            )
            .await
            .map_err(ObservationError::into_io)?;
        let checked = (|| {
            let value: Current = serde_json::from_str(&body).map_err(|_| unavailable())?;
            if value.schema != 1
                || !valid_hex(&value.request, 32)
                || value.operation != operation
                || value.member != context.member
                || value.session != context.session
                || value.state != "app-current"
                || value.app_owner != self.native.app_owner
            {
                return Err(unavailable());
            }
            Ok(())
        })();
        if checked.is_err() {
            context.transport.release();
        }
        checked
    }
}
