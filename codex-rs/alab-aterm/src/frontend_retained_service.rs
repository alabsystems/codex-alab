// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! IO adapters over the actual independent controller and retained owners.
//! These operations request Native admission; no argument, service selection,
//! receipt, or successful Source constructor grants SDK/model/foreground use.
//! Ordinary target selection must still complete the genuine continuation gate.
use super::Controller;
use super::retained;
use super::retained_backend;
use crate::source_retained_service::StartupSettings;
use codex_alab_core::native_incoming::Incoming;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Mutex;

pub use super::retained_backend::ClosedCustody;
pub use super::retained_backend::ConfiguredRuntimeAck;
pub use super::retained_backend::ConfiguredRuntimeActivation;
pub use super::retained_backend::ConfiguredRuntimeCommit;
pub use super::retained_backend::ConfiguredRuntimeFrontier;
pub use super::retained_backend::HeldSourceTransition;
pub use super::retained_backend::PendingHold;
pub use super::retained_backend::RegisteredSourceTransition;
pub use super::retained_backend::RuntimeAck;
pub use super::retained_backend::RuntimeActivation;
pub use super::retained_backend::RuntimeCommit;
pub use super::retained_backend::RuntimeFrontier;
pub use super::retained_backend::RuntimeObservationError;
pub use super::retained_backend::SelectionAck;
pub use super::retained_backend::SelectionCommit;
pub use super::retained_backend::SelectionFrontier;
pub use super::retained_backend::SelectionObservation;
pub use super::retained_backend::SelectionReleased;
pub use super::retained_backend::SourceShutdown;
pub use super::retained_backend::SourceTransition;
pub use super::retained_origin::AcceptedOrigin;
pub use super::retained_origin::CurrentOrigin;
pub use retained::AttachmentObservation;
pub use retained::EditorCheckpoint;
pub use retained::RetainedAttachment;
pub use retained::RetainedPhase;
pub use retained::RetainedStart;
pub use retained::ServiceObservation;

pub fn prepare_start(
    controller: &Controller,
    operation: &str,
    settings: StartupSettings,
) -> io::Result<RetainedStart> {
    controller.prepare_retained_start(operation, settings)
}

pub fn prepare_attachment(
    controller: &Controller,
    operation: &str,
    service: &str,
) -> io::Result<RetainedAttachment> {
    controller.prepare_retained_attachment(operation, service)
}

/// Actual constructor/control/queue custody from one successful physical bind.
/// Neither this object nor its Incoming clone is a Native continuation grant.
#[derive(Clone)]
pub struct BoundBackend {
    // Exact bound owner custody; frontier accessors land with their real consumer.
    _backend: std::sync::Arc<Backend>,
    incoming: Incoming,
}
impl BoundBackend {
    /// Close fresh Source scheduling/production, retaining this SAME actual
    /// controller for exact owned cancellation/drain. Final shutdown performs
    /// actual Native/IO withdrawal only after Source workers/Core cleanup.
    pub fn begin_source_shutdown(&self) -> SourceShutdown {
        self._backend.owner.begin_source_shutdown()
    }
    /// Same pure Source gate created by the actual physical bind; pass it
    /// unchanged to the full original-input factory before any Core start.
    /// Real current-A observation from THIS bound owner's exact origin map.
    /// Returned service is DATA, never model/reader/Go/release authority.
    pub async fn observe_reader_origin(
        &self,
        origin: &AcceptedOrigin,
    ) -> io::Result<CurrentOrigin> {
        self._backend.owner.observe_reader_origin(origin).await
    }
    pub fn goal_gate(&self) -> codex_alab_core::retained_goal_gate::Gate {
        self._backend.owner.goal_gate()
    }
    /// Source scheduling custody only. This captures the SAME actually held
    /// initial barrier; it grants no Native Go, UI, model, or release authority.
    pub fn initial_closed_custody(&self) -> io::Result<ClosedCustody> {
        self._backend.owner.initial_closed_custody()
    }
    pub fn stop_goal_admission(&self) {
        self._backend.owner.stop_goal_admission();
    }
    pub fn prepare_source_transition(
        &self,
        origin: &AcceptedOrigin,
    ) -> io::Result<SourceTransition> {
        self._backend.owner.prepare_source_transition(origin)
    }
    pub async fn frontier(
        &self,
        frontier: &str,
        origin: &AcceptedOrigin,
    ) -> io::Result<RuntimeFrontier> {
        self._backend.owner.frontier(frontier, origin).await
    }
    /// Same real physical runtime packet exposed only through the configured
    /// API, which deliberately has no Source scheduling release operation.
    pub async fn configured_frontier(
        &self,
        frontier: &str,
        origin: &AcceptedOrigin,
    ) -> io::Result<ConfiguredRuntimeFrontier> {
        Ok(ConfiguredRuntimeFrontier::from_actual(
            self.frontier(frontier, origin).await?,
        ))
    }
    pub async fn shutdown(&self) -> io::Result<()> {
        self._backend.owner.shutdown().await
    }
    pub fn incoming(&self) -> Incoming {
        self.incoming.clone()
    }
    pub fn into_incoming(self) -> Incoming {
        self.incoming
    }
}

/// Bind before config/auth, retaining the exact independent controller and A
/// socket. Native continuation operations must borrow this actual bound owner.
pub async fn bind_backend(controller: Controller) -> io::Result<BoundBackend> {
    let mut owner = controller.bind_retained_backend_stdin().await?;
    let connections = Mutex::new(owner.take_connections()?);
    let backend = std::sync::Arc::new(Backend { owner, connections });
    let incoming = Incoming::new(Provider(backend.clone()));
    Ok(BoundBackend {
        _backend: backend,
        incoming,
    })
}
struct Backend {
    owner: retained_backend::RetainedBackendOwner,
    connections: Mutex<retained_backend::BackendConnections>,
}
struct Provider(std::sync::Arc<Backend>);
impl IncomingStreamProvider for Provider {
    fn accept(&self) -> codex_alab_core::native_incoming::Accept<'_> {
        Box::pin(std::future::poll_fn(|context| {
            let mut connections = match self.0.connections.lock() {
                Ok(connections) => connections,
                Err(_) => {
                    return std::task::Poll::Ready(Err(
                        codex_alab_core::managed_auth::unavailable(),
                    ));
                }
            };
            connections
                .poll_next(context)
                .map(|next| Ok(next.map(retained_backend::BackendConnection::into_accepted)))
        }))
    }
    fn withdrawn(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(self.0.owner.withdrawn())
    }
}

pub use super::retained::EntryObservation;
pub use super::retained::GrantedFrontendCommand;
pub use super::retained::ReplacementObservation;
pub use super::retained::RetainedEntry;
pub use super::retained::RetainedReplacement;

pub use super::retained::FrontendCapture;
pub use super::retained::FrontendCaptureWindow;

pub use super::retained::FrontendCommandExecScope;
pub use super::retained::PreparedFrontendSocket;

#[path = "frontend_original_editor.rs"]
pub mod original_editor; // Actual original Child/TempPath custody, not a Native grant.
