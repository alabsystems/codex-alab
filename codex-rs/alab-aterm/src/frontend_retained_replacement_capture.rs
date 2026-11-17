// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Defers original UI/input capture until Native has connected the SAME
//! operation's settings callback, which its real actor does only after H1 ACK.
//! This is a transport milestone, never a Native Go/UI/model capability.
use super::*;

struct Data {
    settings: StartupSettings,
    input: Arc<[u8]>,
}
#[derive(Default)]
struct Progress {
    connected: bool,
    data: Option<Arc<Data>>,
    result: Option<ReplacementObservation>,
}
#[derive(Default)]
struct State {
    progress: Mutex<Progress>,
    changed: tokio::sync::Notify,
}
impl State {
    fn completed(&self, result: ReplacementObservation) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if progress.result.is_none() {
            progress.result = Some(result);
        }
        drop(progress);
        self.changed.notify_waiters();
    }
    async fn data(&self) -> io::Result<Arc<Data>> {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            let _ = notified.as_mut().enable();
            {
                let progress = self
                    .progress
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if matches!(progress.result, Some(ReplacementObservation::Unresolved(_))) {
                    return Err(unavailable());
                }
                if let Some(data) = &progress.data {
                    return Ok(data.clone());
                }
            }
            notified.await;
        }
    }
}
struct CaptureGuard {
    state: Arc<State>,
    complete: bool,
}
impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.state
                .completed(ReplacementObservation::Unresolved(io::ErrorKind::Other));
        }
    }
}

/// Own before the first observation await. Cancellation keeps this SAME
/// Native operation, pending worker, callbacks, capture milestone and command.
pub struct FrontendCapture {
    context: Arc<Context>,
    operation: String,
    service: String,
    source_build: u64,
    target_build: u64,
    settings_callback: Arc<Callback>,
    input_callback: Arc<Callback>,
    command_callback: Arc<Callback>,
    command: Arc<Shared<GrantedFrontendCommand>>,
    state: Arc<State>,
    pending: Option<JoinHandle<ReplacementObservation>>,
    observed: Option<ReplacementObservation>,
}

/// Private construction requires the actual authenticated Native settings
/// connection retained in the accepted worker. No Boolean/label constructor.
/// The original App/StartupDraft still owns every UI/input/editor resource.
pub struct FrontendCaptureWindow {
    context: Arc<Context>,
    operation: String,
    service: String,
    state: Arc<State>,
}
impl Controller {
    pub fn prepare_frontend_capture(
        &self,
        operation: &str,
        service: &str,
        source_build: u64,
        target_build: u64,
    ) -> io::Result<FrontendCapture> {
        if !valid_hex(operation, 32)
            || !valid_hex(service, 32)
            || source_build == 0
            || target_build == 0
        {
            return Err(unavailable());
        }
        Ok(FrontendCapture {
            context: Context::capture(self)?,
            operation: operation.into(),
            service: service.into(),
            source_build,
            target_build,
            settings_callback: Arc::new(Callback::bind()?),
            input_callback: Arc::new(Callback::bind()?),
            command_callback: Arc::new(Callback::bind()?),
            command: Shared::new(),
            state: Arc::new(State::default()),
            pending: None,
            observed: None,
        })
    }
}
impl FrontendCaptureWindow {
    pub fn same_controller(&self, actual: &Controller) -> bool {
        Arc::ptr_eq(&self.context.transport, &actual.transport)
            && self.context.member == actual.member
            && self.context.controller_operation == actual.operation
            && self.context.session == actual.session
    }
    pub fn service_data(&self) -> &str {
        &self.service
    }
    pub fn operation_data(&self) -> &str {
        &self.operation
    }
    /// Called only after real original capture, with its original resource
    /// owners still retained. DATA never substitutes for those owners or Go.
    pub fn provide(self, settings: StartupSettings, input: Vec<u8>) -> io::Result<()> {
        if input.is_empty() || input.len() > MAX_BODY {
            return Err(unavailable());
        }
        let data = Arc::new(Data {
            settings,
            input: input.into(),
        });
        let mut progress = self
            .state
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !progress.connected || progress.result.is_some() || progress.data.is_some() {
            return Err(unavailable());
        }
        progress.data = Some(data);
        drop(progress);
        self.state.changed.notify_waiters();
        Ok(())
    }
}
impl FrontendCapture {
    fn start(&mut self) -> io::Result<()> {
        if self.pending.is_some() || self.observed.is_some() {
            return Ok(());
        }
        let runtime = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
        let context = self.context.clone();
        let operation = self.operation.clone();
        let service = self.service.clone();
        let source_build = self.source_build;
        let target_build = self.target_build;
        let settings = self.settings_callback.clone();
        let input = self.input_callback.clone();
        let command = self.command_callback.clone();
        let shared = self.command.clone();
        let state = self.state.clone();
        // Both guards exist before spawn, including an unpolled worker loss.
        let guard = Guard {
            context: context.clone(),
            shared: shared.clone(),
            complete: false,
        };
        let capture_guard = CaptureGuard {
            state: state.clone(),
            complete: false,
        };
        self.pending = Some(runtime.spawn(async move {
            let mut guard = guard;
            let mut capture_guard = capture_guard;
            let outcome = tokio::time::timeout(WHOLE, async {
                let fields = format!(
                    "{} service={} source_build={} target_build={} settings={} input={} command={}",
                    context.fields(),
                    service,
                    source_build,
                    target_build,
                    path(&settings.path),
                    path(&input.path),
                    path(&command.path)
                );
                let control = async {
                    loop {
                        let body = context
                            .transport
                            .call(
                                "controller-retained-prepare",
                                &operation,
                                &fields,
                                Some(&context.member),
                            )
                            .await?;
                        let frame = response(
                            &context,
                            &body,
                            &operation,
                            &service,
                            &operation,
                            &["holding", "go"],
                        )?;
                        if frame.state == "go" {
                            return Ok::<_, io::Error>(());
                        }
                        tokio::time::sleep(HOLD_POLL).await;
                    }
                };
                let settings_io = async {
                    // Actual Native does this only AFTER its genuine H1 ACK.
                    // Keep the accepted socket owned before publishing window.
                    let mut stream = settings.accept(&context.transport, context.peer).await?;
                    if context.transport.server_pid()? != context.peer {
                        return Err(unavailable());
                    }
                    {
                        let mut progress = state.progress.lock().map_err(|_| unavailable())?;
                        if progress.connected || progress.result.is_some() {
                            return Err(unavailable());
                        }
                        progress.connected = true;
                    }
                    state.changed.notify_waiters();
                    let data = state.data().await?;
                    if context.transport.server_pid()? != context.peer {
                        return Err(unavailable());
                    }
                    data.settings.write_and_shutdown(&mut stream).await?;
                    require_eof(&stream).await
                };
                let input_io = async {
                    let data = state.data().await?;
                    serve_input(&input, &context, &data.input).await
                };
                let command_io = async {
                    let mut offer = receive_offer(&command, &context, b"G1", &operation).await?;
                    let actual = GrantedFrontendCommand::decode(
                        &offer.body,
                        &operation,
                        &service,
                        context.clone(),
                    )?;
                    {
                        let mut slot = shared.result.lock().map_err(|_| unavailable())?;
                        if slot.is_some() {
                            return Err(unavailable());
                        }
                        *slot = Some(Ok(actual)); // Actual command BEFORE physical ACK.
                    }
                    offer.acknowledge().await
                };
                tokio::try_join!(control, settings_io, input_io, command_io)?;
                Ok::<_, io::Error>(())
            })
            .await;
            let observed = match outcome {
                Ok(Ok(())) => ReplacementObservation::Granted,
                Ok(Err(error)) => {
                    context.transport.release();
                    shared.fail(error.kind());
                    ReplacementObservation::Unresolved(error.kind())
                }
                Err(_) => {
                    context.transport.release();
                    shared.fail(io::ErrorKind::TimedOut);
                    ReplacementObservation::Unresolved(io::ErrorKind::TimedOut)
                }
            };
            state.completed(observed);
            guard.complete = true;
            capture_guard.complete = true;
            observed
        }));
        Ok(())
    }
    pub async fn capture_window(&mut self) -> io::Result<FrontendCaptureWindow> {
        self.start()?;
        loop {
            let notified = self.state.changed.notified();
            tokio::pin!(notified);
            let _ = notified.as_mut().enable();
            {
                let progress = self
                    .state
                    .progress
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(ReplacementObservation::Unresolved(kind)) = progress.result {
                    return Err(io::Error::new(
                        kind,
                        "same Native replacement capture remains unresolved",
                    ));
                }
                if progress.connected {
                    return Ok(FrontendCaptureWindow {
                        context: self.context.clone(),
                        operation: self.operation.clone(),
                        service: self.service.clone(),
                        state: self.state.clone(),
                    });
                }
            }
            notified.await;
        }
    }
    pub async fn observe(&mut self) -> ReplacementObservation {
        if let Some(observed) = self.observed {
            return observed;
        }
        if let Err(error) = self.start() {
            return ReplacementObservation::Unresolved(error.kind());
        }
        let observed = match self.pending.as_mut() {
            Some(task) => task
                .await
                .unwrap_or(ReplacementObservation::Unresolved(io::ErrorKind::Other)),
            None => ReplacementObservation::Unresolved(io::ErrorKind::Other),
        };
        self.pending.take();
        self.observed = Some(observed);
        observed
    }
    pub fn take_command(&mut self) -> io::Result<GrantedFrontendCommand> {
        if self.observed != Some(ReplacementObservation::Granted) {
            return Err(unavailable());
        }
        match self
            .command
            .result
            .lock()
            .map_err(|_| unavailable())?
            .take()
        {
            Some(Ok(actual)) => Ok(actual),
            _ => Err(unavailable()),
        }
    }
}

#[cfg(test)]
#[path = "frontend_retained_replacement_capture_tests.rs"]
mod tests;
