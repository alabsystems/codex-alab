// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retained-service transport on the original independent controller.
//! Operation owners outlive cancelled response waiters. No service identifier,
//! startup-state word or callback PID creates native attachment permission.
//!
//! This transport requests genuine Native admission. Compiling its API and
//! observing transport state do not authorize ordinary TUI activation or any
//! credential/model/foreground use. The ordinary selector remains unmounted.
use super::*;
use crate::source_retained_service::StartupSettings;
use std::os::unix::ffi::OsStrExt as _;
use std::sync::Mutex;
use std::time::Duration;
use tokio::net::UnixStream;
use tokio::task::JoinHandle;

#[path = "frontend_retained_callback.rs"]
mod callback;
use callback::Callback;

#[path = "frontend_retained_editor_checkpoint.rs"]
mod editor_checkpoint;
pub use editor_checkpoint::EditorCheckpoint;

// Native bounds each complete physical exchange by sixty seconds. The outer
// hang detector also covers receipt processing without changing Conn's policy.
const EXCHANGE: Duration = Duration::from_secs(65);

/// Native startup observation, never readiness or an SDK/provider grant.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetainedPhase {
    Starting,
    Spawned,
    Failed,
    Stopping,
}

/// A failed/lost reply preserves uncertainty; it never reports a no-child fact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceObservation {
    Observed {
        service: String,
        phase: RetainedPhase,
    },
    Unresolved(io::ErrorKind),
}

/// Physical delivery and its correlated native control response must both
/// succeed before the unique stream is available. The flag is diagnostic data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentObservation {
    Confirmed,
    Unresolved {
        error: io::ErrorKind,
        descriptor_received: bool,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceFrame {
    schema: u32,
    request: String,
    operation: String,
    session: crate::accounts::Session,
    member: String,
    service: String,
    state: RetainedPhase,
}

struct Context {
    transport: Arc<source_member::Transport>,
    member: String,
    controller_operation: String,
    session: crate::accounts::Session,
    peer: u32,
}
impl Context {
    fn capture(controller: &Controller) -> io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            peer: controller.transport.server_pid()?,
            transport: controller.transport.clone(),
            member: controller.member.clone(),
            controller_operation: controller.operation.clone(),
            session: controller.session.clone(),
        }))
    }
    fn fields(&self) -> String {
        format!(
            " member={} controller_operation={}",
            self.member, self.controller_operation
        )
    }
    fn validate(&self, frame: &Allocation, operation: &str) -> io::Result<()> {
        if frame.schema != 1
            || !valid_hex(&frame.request, 32)
            || frame.operation != operation
            || frame.member != self.member
            || frame.session != self.session
        {
            return Err(unavailable());
        }
        Ok(())
    }
    fn observed(&self, body: &str, operation: &str) -> io::Result<ServiceObservation> {
        let frame: ServiceFrame = serde_json::from_str(body).map_err(|_| unavailable())?;
        self.validate(
            &Allocation {
                schema: frame.schema,
                request: frame.request,
                operation: frame.operation,
                session: frame.session,
                member: frame.member,
            },
            operation,
        )?;
        if !valid_hex(&frame.service, 32) {
            return Err(unavailable());
        }
        Ok(ServiceObservation::Observed {
            service: frame.service,
            phase: frame.state,
        })
    }
}

impl Controller {
    /// Prepare one immutable startup operation before any native request.
    /// Keep this owner after an unanswered call and reconcile this same operation;
    /// never replace it with a fresh controller or an embedded fallback.
    pub(crate) fn prepare_retained_start(
        &self,
        operation: &str,
        settings: StartupSettings,
    ) -> io::Result<RetainedStart> {
        if !valid_hex(operation, 32) {
            return Err(unavailable());
        }
        Ok(RetainedStart {
            context: Context::capture(self)?,
            operation: operation.into(),
            settings: Arc::new(settings),
            callback: Arc::new(Callback::bind()?),
            pending: None,
            service: None,
        })
    }

    /// Prepare one callback delivery on this original frontend controller.
    /// `service` is selection DATA; native independently checks the actual
    /// controller, foreground, backend constructor and SDK before passing IO.
    pub(crate) fn prepare_retained_attachment(
        &self,
        operation: &str,
        service: &str,
    ) -> io::Result<RetainedAttachment> {
        if !valid_hex(operation, 32) || !valid_hex(service, 32) {
            return Err(unavailable());
        }
        Ok(RetainedAttachment {
            context: Context::capture(self)?,
            operation: operation.into(),
            service: service.into(),
            callback: Arc::new(Callback::bind()?),
            received: Arc::new(Mutex::new(None)),
            pending: None,
            observed: None,
        })
    }
}

/// Immutable startup settings, callback and operation. Cancelling `observe`
/// leaves the exact in-flight request owned here; a subsequent call joins it.
pub struct RetainedStart {
    context: Arc<Context>,
    operation: String,
    settings: Arc<StartupSettings>,
    callback: Arc<Callback>,
    pending: Option<JoinHandle<ServiceObservation>>,
    service: Option<String>,
}
impl RetainedStart {
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Start or observe the same accepted intent. A new observation sends the
    /// same settings and operation on the same actual connection. Connection
    /// loss stays unresolved; this API never reconnects or mints another owner.
    pub async fn observe(&mut self) -> ServiceObservation {
        if self.pending.is_none() {
            let context = self.context.clone();
            let settings = self.settings.clone();
            let callback = self.callback.clone();
            let operation = self.operation.clone();
            self.pending = Some(tokio::spawn(async move {
                let result = tokio::time::timeout(EXCHANGE, async {
                    if context.transport.server_pid()? != context.peer {
                        return Err(unavailable());
                    }
                    let fields = format!("{} settings={}", context.fields(), path(&callback.path));
                    let response = context.transport.call(
                        "controller-service-start",
                        &operation,
                        &fields,
                        Some(&context.member),
                    );
                    tokio::pin!(response);
                    tokio::select! {
                        result = &mut response => context.observed(&result?, &operation),
                        result = callback.settings(&context.transport, context.peer, &settings) => {
                            result?;
                            Err(unavailable())
                        }
                    }
                })
                .await;
                match result {
                    Ok(Ok(observation)) => observation,
                    Ok(Err(error)) => ServiceObservation::Unresolved(error.kind()),
                    Err(_) => ServiceObservation::Unresolved(io::ErrorKind::TimedOut),
                }
            }));
        }
        let completed = match self.pending.as_mut() {
            Some(pending) => Some(pending.await),
            None => None,
        };
        let observed = match completed {
            Some(Ok(observed)) => observed,
            Some(Err(_)) | None => ServiceObservation::Unresolved(io::ErrorKind::Other),
        };
        self.pending.take();
        if let ServiceObservation::Observed { service, .. } = &observed {
            if self
                .service
                .as_ref()
                .is_some_and(|original| original != service)
            {
                return ServiceObservation::Unresolved(io::ErrorKind::InvalidData);
            }
            self.service = Some(service.clone());
        }
        observed
    }
}

/// One physical delivery attempt. Neither a cancelled waiter nor an unanswered
/// control response can recreate its callback, resend native IO or start work.
pub struct RetainedAttachment {
    context: Arc<Context>,
    operation: String,
    service: String,
    callback: Arc<Callback>,
    received: Arc<Mutex<Option<UnixStream>>>,
    pending: Option<JoinHandle<AttachmentObservation>>,
    observed: Option<AttachmentObservation>,
}
impl RetainedAttachment {
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Join the same delivery even if an earlier waiter was cancelled. A lost
    /// reply retains any physically received endpoint but exposes no stream.
    pub async fn observe(&mut self) -> AttachmentObservation {
        if let Some(observed) = self.observed {
            return observed;
        }
        if self.pending.is_none() {
            let context = self.context.clone();
            let callback = self.callback.clone();
            let operation = self.operation.clone();
            let service = self.service.clone();
            let received = self.received.clone();
            self.pending = Some(tokio::spawn(async move {
                let result = tokio::time::timeout(EXCHANGE, async {
                    if context.transport.server_pid()? != context.peer {
                        return Err(unavailable());
                    }
                    let fields = format!(
                        "{} service={} callback={}",
                        context.fields(),
                        service,
                        path(&callback.path)
                    );
                    let control = async {
                        let body = context
                            .transport
                            .call(
                                "controller-service-attach",
                                &operation,
                                &fields,
                                Some(&context.member),
                            )
                            .await?;
                        let frame: Allocation =
                            serde_json::from_str(&body).map_err(|_| unavailable())?;
                        context.validate(&frame, &operation)
                    };
                    let physical = async {
                        let delivery = callback.endpoint(&context.transport, context.peer).await?;
                        delivery.acknowledge(&received).await
                    };
                    let (control, physical) = tokio::join!(control, physical);
                    control?;
                    physical
                })
                .await;
                let error = match result {
                    Ok(Ok(())) => return AttachmentObservation::Confirmed,
                    Ok(Err(error)) => error.kind(),
                    Err(_) => io::ErrorKind::TimedOut,
                };
                AttachmentObservation::Unresolved {
                    error,
                    descriptor_received: received.lock().is_ok_and(|stream| stream.is_some()),
                }
            }));
        }
        let completed = match self.pending.as_mut() {
            Some(pending) => Some(pending.await),
            None => None,
        };
        let observed = match completed {
            Some(Ok(observed)) => observed,
            Some(Err(_)) | None => AttachmentObservation::Unresolved {
                error: io::ErrorKind::Other,
                descriptor_received: self.received.lock().is_ok_and(|stream| stream.is_some()),
            },
        };
        self.pending.take();
        self.observed = Some(observed);
        observed
    }

    /// Take the unique stream only after the correlated reply and physical
    /// native delivery both completed. Its bytes grant no tool birth authority.
    pub fn take_stream(&mut self) -> io::Result<UnixStream> {
        if self.observed != Some(AttachmentObservation::Confirmed) {
            return Err(unavailable());
        }
        self.received
            .lock()
            .map_err(|_| unavailable())?
            .take()
            .ok_or_else(unavailable)
    }
}

fn path(path: &std::path::Path) -> String {
    path.as_os_str()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "frontend_retained_tests.rs"]
mod tests;

#[path = "frontend_retained_replacement.rs"]
mod replacement;
pub use replacement::EntryObservation;
pub use replacement::FrontendCommandExecScope;
pub use replacement::GrantedFrontendCommand;
pub use replacement::PreparedFrontendSocket;
pub use replacement::ReplacementObservation;
pub use replacement::RetainedEntry;
pub use replacement::RetainedReplacement;

pub use replacement::FrontendCapture;
pub use replacement::FrontendCaptureWindow;
