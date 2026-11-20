// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Frontend control tenure, on a transport independent of credential readers.
use crate::Endpoint;
use crate::source_member;
use codex_alab_core::managed_auth::unavailable;
use codex_alab_core::managed_auth::valid_hex;
use serde::Deserialize;
use std::io;
use std::sync::Arc;

#[path = "frontend_backend.rs"]
mod backend;
pub use backend::Backend;
#[cfg(feature = "native-source-proof")]
#[path = "frontend_retained_proof.rs"]
pub mod native_source_proof;
#[cfg(unix)]
#[path = "frontend_retained.rs"]
mod retained;
#[cfg(unix)]
#[path = "frontend_retained_backend.rs"]
mod retained_backend;
#[cfg(unix)]
#[path = "frontend_retained_origin.rs"]
mod retained_origin;
#[cfg(unix)]
#[path = "frontend_retained_service.rs"]
pub mod retained_service;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Allocation {
    schema: u32,
    request: String,
    operation: String,
    session: crate::accounts::Session,
    member: String,
}

/// The startup policy reported for this controller's current foreground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupMode {
    Embedded,
    Retained,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartupModeReply {
    schema: u32,
    request: String,
    operation: String,
    session: crate::accounts::Session,
    member: String,
    mode: String,
}

fn request_operation() -> String {
    format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    )
}

/// An actual native-issued controller. It is neither a Source nor permission
/// to attach to a backend. Dropping it closes only its independent connection.
pub struct Controller {
    transport: Arc<source_member::Transport>,
    member: String,
    operation: String,
    session: crate::accounts::Session,
    environment_count: usize,
}

impl Controller {
    pub async fn open_at(endpoint: &Endpoint, operation: &str, birth: &str) -> io::Result<Self> {
        if !valid_hex(operation, 32) {
            return Err(unavailable());
        }
        let transport = source_member::controller_transport(endpoint, birth).await?;
        let body = transport
            .call("controller-open", operation, "", None)
            .await?;
        // Transport already checks schema, request, operation, host epoch and
        // actual process identity. Keep the exact closed response schema too.
        let reply: Allocation = serde_json::from_str(&body).map_err(|_| unavailable())?;
        if reply.schema != 1
            || !valid_hex(&reply.request, 32)
            || reply.operation != operation
            || !valid_hex(&reply.member, 32)
            || !crate::accounts::valid_session(&reply.session, &endpoint.sid, birth)
        {
            return Err(unavailable());
        }
        Ok(Self {
            transport: Arc::new(transport),
            member: reply.member,
            operation: operation.into(),
            session: reply.session,
            environment_count: std::env::vars_os().count(),
        })
    }

    /// Start or reconcile this same private root operation on the original
    /// native controller. Failure never selects an embedded/local fallback.
    pub async fn start_backend(
        &self,
        operation: &str,
        overrides: &[String],
    ) -> io::Result<Backend> {
        backend::start(self, operation, overrides).await
    }

    /// Await the actual captured native root's settlement before closing this
    /// controller. This reports neither all Source effects nor next-goal readiness.
    pub async fn stop_backend(&self) -> io::Result<()> {
        backend::stop(self).await
    }

    /// Read native construction policy. Service start and attachment perform
    /// their own admission checks on this connection.
    pub async fn startup_mode(&self) -> io::Result<StartupMode> {
        let operation = request_operation();
        let body = self
            .bound_request("controller-startup-mode", &operation)
            .await?;
        let reply: StartupModeReply = serde_json::from_str(&body).map_err(|_| unavailable())?;
        if reply.schema != 1
            || !valid_hex(&reply.request, 32)
            || reply.operation != operation
            || reply.member != self.member
            || reply.session != self.session
        {
            return Err(unavailable());
        }
        match reply.mode.as_str() {
            "embedded" => Ok(StartupMode::Embedded),
            "retained" => Ok(StartupMode::Retained),
            _ => Err(unavailable()),
        }
    }

    /// An explicit native withdrawal receipt, not an acknowledgment that any
    /// backend root or tool has stopped. Transport loss is returned as uncertain.
    pub async fn close(&self) -> io::Result<()> {
        self.bound_request("controller-close", &request_operation())
            .await?;
        Ok(())
    }

    async fn bound_request(&self, action: &str, operation: &str) -> io::Result<String> {
        self.transport
            .call(
                action,
                operation,
                &format!(
                    " member={} controller_operation={}",
                    self.member, self.operation
                ),
                Some(&self.member),
            )
            .await
    }
}

#[cfg(unix)]
#[path = "frontend_retained_frontend_frontier.rs"]
mod retained_frontier;
#[cfg(unix)]
pub use retained_frontier::AppFrontendOwner;
#[cfg(unix)]
pub use retained_frontier::FrontendFrontier;
#[cfg(unix)]
pub use retained_frontier::FrontendUiAck;

#[cfg(unix)]
pub use retained_frontier::FrontendSelectionAppOwner;
#[cfg(unix)]
pub use retained_frontier::FrontendSelectionFrontier;
#[cfg(unix)]
pub use retained_frontier::FrontendSelectionUiAck;

#[cfg(unix)]
#[path = "frontend_package_host.rs"]
mod package_host;
#[cfg(unix)]
pub use package_host::ControllerPackageHostPeer;
