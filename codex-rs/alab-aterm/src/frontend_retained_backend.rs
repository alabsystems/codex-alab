// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Test-scoped independent-controller binding and retained backend attachment IO.
//! No Core dependency on this crate, ordinary TUI hook, config defaults, Root
//! tool channel, credential-reader constructor or SDK-readiness claim is added.
use super::*;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncReadExt as _;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::sync::watch;

// F1 and A use one guarded ancillary decoder in this ALab-owned private module.
use crate::retained_fd as fd;

#[path = "frontend_retained_control_frame.rs"]
mod control_frame;
#[path = "frontend_retained_goal_owner.rs"]
mod goal;
pub use goal::ClosedCustody;
pub use goal::ConfiguredRuntimeAck;
pub use goal::ConfiguredRuntimeActivation;
pub use goal::ConfiguredRuntimeCommit;
pub use goal::ConfiguredRuntimeFrontier;
pub use goal::HeldSourceTransition;
pub use goal::PendingHold;
pub use goal::RegisteredSourceTransition;
pub use goal::RuntimeAck;
pub use goal::RuntimeActivation;
pub use goal::RuntimeCommit;
pub use goal::RuntimeFrontier;
pub use goal::RuntimeObservationError;
pub use goal::SelectionAck;
pub use goal::SelectionCommit;
pub use goal::SelectionFrontier;
pub use goal::SelectionObservation;
pub use goal::SelectionReleased;
pub use goal::SourceShutdown;
pub use goal::SourceTransition;

const IO_DEADLINE: Duration = Duration::from_secs(60);
const QUEUED_CONNECTIONS: usize = 16;

impl Controller {
    /// Bind the actual inherited native socket before config/auth startup.
    /// Native must independently accept this issued controller and the challenge
    /// read from its original constructor socket. Arguments and frame data grant
    /// no permission. Failure has no ordinary/embedded startup fallback.
    pub(crate) async fn bind_retained_backend_stdin(self) -> io::Result<RetainedBackendOwner> {
        let socket = fd::take_private_stdin()?;
        bind(self, socket).await
    }

    #[cfg(test)]
    async fn bind_retained_backend_socket(
        self,
        socket: std::os::unix::net::UnixStream,
    ) -> io::Result<RetainedBackendOwner> {
        bind(self, fd::adopt_socket(socket)?).await
    }
}

async fn bind(controller: Controller, mut socket: UnixStream) -> io::Result<RetainedBackendOwner> {
    let operation = format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix(),
    );
    control(&controller, "controller-control-begin", &operation, None).await?;

    // The actual native begin writes ALABCTL1+nonce on its original socket.
    // No native epoch, PID, caller-provided nonce or claimed readiness replaces
    // physical reception from that socket.
    let mut challenge = [0_u8; 40];
    tokio::time::timeout(IO_DEADLINE, socket.read_exact(&mut challenge))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    let nonce = std::str::from_utf8(&challenge[8..]).map_err(|_| unavailable())?;
    if &challenge[..8] != b"ALABCTL1" || !valid_hex(nonce, 32) {
        return Err(unavailable());
    }
    control(
        &controller,
        "controller-control-ack",
        &operation,
        Some(nonce),
    )
    .await?;

    let controller = Arc::new(controller);
    let socket = Arc::new(socket);
    let (send, connections) = mpsc::channel(QUEUED_CONNECTIONS);
    let (withdrawn, observed_withdrawal) = watch::channel(false);
    let pending = Arc::new(Mutex::new(None));
    let goal = goal::GoalOwner::new(controller.clone(), observed_withdrawal.clone());
    let receive_goal = goal.clone();
    let retained_controller = controller.clone();
    let retained_socket = socket.clone();
    let lost = withdrawn.clone();
    let retained_pending = pending.clone();
    let task = tokio::spawn(async move {
        let _outcome = receive_connections(
            &retained_socket,
            send,
            retained_pending,
            retained_controller.clone(),
            lost.subscribe(),
            receive_goal.clone(),
        )
        .await;
        receive_goal.withdraw();
        // Any original-channel EOF/protocol/IO failure withdraws this actual
        // owner. Frontend stream EOF is separate and never enters this loop.
        lost.send_replace(true);
        fd::shutdown(&retained_socket);
        retained_controller.transport.release();
    });
    Ok(RetainedBackendOwner {
        controller,
        socket,
        connections: Some(connections),
        withdrawn,
        observed_withdrawal,
        pending,
        task: Mutex::new(Some(task)),
        goal,
        shutdown_serial: tokio::sync::Semaphore::new(1),
        receive_failed: std::sync::atomic::AtomicBool::new(false),
    })
}

async fn control(
    controller: &Controller,
    action: &str,
    operation: &str,
    nonce: Option<&str>,
) -> io::Result<()> {
    let mut fields = format!(
        " member={} controller_operation={}",
        controller.member, controller.operation,
    );
    if let Some(nonce) = nonce {
        if !valid_hex(nonce, 32) {
            return Err(unavailable());
        }
        fields.push_str(&format!(" nonce={nonce}"));
    }
    let body = controller
        .transport
        .call(action, operation, &fields, Some(&controller.member))
        .await?;
    let frame: Allocation = serde_json::from_str(&body).map_err(|_| unavailable())?;
    // Transport validates the exact request counter, native epoch and actual
    // process identity. Keep the action's closed schema and original controller.
    if frame.schema != 1
        || !valid_hex(&frame.request, 32)
        || frame.operation != operation
        || frame.member != controller.member
        || frame.session != controller.session
    {
        return Err(unavailable());
    }
    Ok(())
}

/// Main-stack ownership of the actual independent controller/private socket.
/// Retain it through backend dispatch. Its construction is not SDK readiness,
/// Source permission, frontend activation or a cohort-settlement receipt.
pub(crate) struct RetainedBackendOwner {
    controller: Arc<Controller>,
    socket: Arc<UnixStream>,
    connections: Option<mpsc::Receiver<BackendConnection>>,
    withdrawn: watch::Sender<bool>,
    observed_withdrawal: watch::Receiver<bool>,
    // Actual endpoint custody persists outside a cancelled/failed receive task.
    // Queue ownership replaces this pending owner before native gets its ACK.
    pending: Arc<Mutex<Option<PendingAttachment>>>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    goal: Arc<goal::GoalOwner>,
    shutdown_serial: tokio::sync::Semaphore,
    receive_failed: std::sync::atomic::AtomicBool,
}

struct PendingAttachment {
    stream: Option<UnixStream>,
    deadline: tokio::time::Instant,
}

impl RetainedBackendOwner {
    pub(crate) async fn observe_reader_origin(
        &self,
        origin: &crate::frontend_control::retained_origin::AcceptedOrigin,
    ) -> io::Result<crate::frontend_control::retained_origin::CurrentOrigin> {
        self.goal.observe_reader_origin(origin).await
    }
    pub(crate) fn begin_source_shutdown(&self) -> SourceShutdown {
        self.goal.begin_source_shutdown()
    }
    pub(crate) fn initial_closed_custody(&self) -> io::Result<ClosedCustody> {
        self.goal.initial_closed_custody()
    }
    pub(crate) fn goal_gate(&self) -> codex_alab_core::retained_goal_gate::Gate {
        self.goal.gate()
    }
    pub(crate) fn stop_goal_admission(&self) {
        self.goal.withdraw();
    }
    pub(crate) fn prepare_source_transition(
        &self,
        origin: &super::retained_origin::AcceptedOrigin,
    ) -> io::Result<SourceTransition> {
        self.goal.prepare_source_transition(origin)
    }
    pub(crate) async fn frontier(
        &self,
        frontier: &str,
        origin: &super::retained_origin::AcceptedOrigin,
    ) -> io::Result<RuntimeFrontier> {
        self.goal.frontier(frontier, origin).await
    }
    /// The actual backend caller drains Core/accepted operations first. This
    /// then withdraws the same IO owner and joins its original workers. A
    /// canceled wait restores the same task handle, never claims completion.
    pub(crate) async fn shutdown(&self) -> io::Result<()> {
        let _serial = self
            .shutdown_serial
            .acquire()
            .await
            .map_err(|_| unavailable())?;
        self.goal.withdraw();
        self.withdrawn.send_replace(true);
        fd::shutdown(&self.socket);
        self.controller.transport.release();
        let task = self.task.lock().map_err(|_| unavailable())?.take();
        if let Some(task) = task {
            let mut owned = ReceiveTaskLease {
                owner: self,
                task: Some(task),
            };
            let joined = owned.task.as_mut().ok_or_else(unavailable)?.await;
            owned.task.take();
            if joined.is_err() {
                self.receive_failed
                    .store(true, std::sync::atomic::Ordering::Release);
            }
        }
        let drained = self.goal.shutdown().await;
        if self
            .receive_failed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            Err(unavailable())
        } else {
            drained
        }
    }

    /// Transfer the one actual connection queue to the persistent app-server's
    /// transport acceptor, never to another runtime or a fresh named listener.
    pub(crate) fn take_connections(&mut self) -> io::Result<BackendConnections> {
        Ok(BackendConnections {
            receive: self.connections.take().ok_or_else(unavailable)?,
        })
    }

    /// Release/cancellation input for the owning persistent runtime. This grants
    /// no work permission and reports no completion or resource settlement.
    pub(crate) async fn withdrawn(&self) {
        let mut receiver = self.observed_withdrawal.clone();
        while !*receiver.borrow_and_update() {
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

impl Drop for RetainedBackendOwner {
    fn drop(&mut self) {
        self.goal.withdraw();
        self.withdrawn.send_replace(true);
        fd::shutdown(&self.socket);
        // Withdraw actual pending IO even if the receive task still owns its
        // Arc during cancellation. Keep the descriptor in custody until drop;
        // poison recovery here can only close IO, never admit or settle work.
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(stream) = pending.as_ref().and_then(|pending| pending.stream.as_ref()) {
            fd::shutdown(stream);
        }
        drop(pending);
        // Close only this original authenticated transport. Never reconnect or
        // reissue a controller as a substitute for this original connection.
        self.controller.transport.release();
        if let Some(task) = self
            .task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            task.abort();
        }
    }
}

struct ReceiveTaskLease<'a> {
    owner: &'a RetainedBackendOwner,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for ReceiveTaskLease<'_> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            *self
                .owner
                .task
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(task);
        }
    }
}

pub(crate) struct BackendConnections {
    receive: mpsc::Receiver<BackendConnection>,
}
impl BackendConnections {
    pub(crate) fn poll_next(
        &mut self,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<BackendConnection>> {
        self.receive.poll_recv(context)
    }

    #[cfg(test)]
    pub(crate) async fn next(&mut self) -> Option<BackendConnection> {
        std::future::poll_fn(|context| self.poll_next(context)).await
    }
}

/// One physically received native endpoint. Its bytes grant no Source/provider
/// permission. Native independently checks the frontend/controller/SDK and sends
/// the peer endpoint only after receiving this channel's exact nonce receipt.
pub(crate) struct BackendConnection {
    stream: UnixStream,
    origin: super::retained_origin::AcceptedOrigin,
}
impl BackendConnection {
    #[cfg(test)]
    pub(crate) fn into_stream(self) -> UnixStream {
        self.stream
    }
    pub(crate) fn into_accepted(self) -> codex_alab_core::native_incoming::AcceptedConnection {
        codex_alab_core::native_incoming::AcceptedConnection::with_resource(
            self.stream,
            codex_alab_core::connection_resource::ConnectionResource::new(self.origin),
        )
    }
}

async fn receive_connections(
    socket: &UnixStream,
    send: mpsc::Sender<BackendConnection>,
    pending: Arc<Mutex<Option<PendingAttachment>>>,
    controller: Arc<Controller>,
    withdrawal: watch::Receiver<bool>,
    goal: Arc<goal::GoalOwner>,
) -> io::Result<()> {
    loop {
        // A is the native->backend channel, not frontend F1. Exactly one right
        // is received and CLOEXEC-marked under the shared synchronous guard.
        let (descriptor, received_deadline) = match control_frame::receive(socket).await? {
            control_frame::Frame::Attachment {
                descriptor,
                deadline,
            } => (descriptor, deadline),
            control_frame::Frame::Hold(hold) => {
                goal.accept(hold)?;
                continue;
            }
            control_frame::Frame::Runtime(runtime) => {
                goal.accept_runtime(runtime)?;
                continue;
            }
            control_frame::Frame::SourceInhibit(packet) => {
                goal.accept_source_inhibition(packet)?;
                continue;
            }
            control_frame::Frame::SourceSelection(packet) => {
                goal.accept_source_selection(packet)?;
                continue;
            }
        };
        let stream = fd::from_descriptor(descriptor)?;
        let deadline = {
            let mut slot = pending.lock().map_err(|_| unavailable())?;
            slot.insert(PendingAttachment {
                stream: Some(stream),
                deadline: received_deadline,
            })
            .deadline
        };
        // Every await after physical receipt leaves the actual endpoint in this
        // owner or the real queue. Header/queue/ACK share one absolute bound.
        tokio::time::timeout_at(deadline, async {
            let nonce = fd::receive_nonce(socket).await?;
            let permit = tokio::select! {
                biased;
                error = fd::owner_ended(socket) => return Err(error),
                permit = send.reserve() => permit.map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?,
            };
            let stream = pending.lock().map_err(|_| unavailable())?.as_mut()
                .and_then(|pending| pending.stream.take()).ok_or_else(unavailable)?;
            let origin = super::retained_origin::AcceptedOrigin::received(
                controller.clone(), nonce, withdrawal.clone());
            goal.register_origin(&origin)?;
            permit.send(BackendConnection { stream, origin });
            let mut offset = 0;
            while offset < nonce.len() {
                tokio::select! {
                    biased;
                    error = fd::owner_ended(socket) => return Err(error),
                    ready = socket.writable() => ready?,
                }
                match socket.try_write(&nonce[offset..]) {
                    Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                    Ok(count) => offset += count,
                    Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        }).await.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
        pending.lock().map_err(|_| unavailable())?.take();
        // Persistent A channel: do not half-close after a receipt. Native may
        // pass the next newly admitted frontend through this same actual owner.
    }
}

#[cfg(test)]
#[path = "frontend_retained_backend_tests.rs"]
mod tests;
