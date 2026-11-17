// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual G1/I1/E1 on the SAME independent controller. This requests real
//! Native admission; selection data and successful codec IO grant no UI/model
//! permission. Requires the paired Native write-half-close correction.
use super::*;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use tokio::io::AsyncWriteExt as _;

const WHOLE: Duration = Duration::from_secs(125);
const MAX_BODY: usize = 256 * 1024 * 1024;
const HOLD_POLL: Duration = Duration::from_millis(100);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplacementObservation {
    Granted,
    Unresolved(io::ErrorKind),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryObservation {
    Entered,
    Unresolved(io::ErrorKind),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeResponse {
    schema: u32,
    request: String,
    operation: String,
    session: crate::accounts::Session,
    member: String,
    service: String,
    go: String,
    state: String,
}
fn response(
    context: &Context,
    body: &str,
    operation: &str,
    service: &str,
    go: &str,
    allowed: &[&str],
) -> io::Result<NativeResponse> {
    let decoded = (|| {
        let frame: NativeResponse = serde_json::from_str(body).map_err(|_| unavailable())?;
        context.validate(
            &Allocation {
                schema: frame.schema,
                request: frame.request.clone(),
                operation: frame.operation.clone(),
                session: frame.session.clone(),
                member: frame.member.clone(),
            },
            operation,
        )?;
        if frame.service != service
            || !valid_hex(&frame.service, 32)
            || frame.go != go
            || !valid_hex(&frame.go, 32)
            || !allowed.contains(&frame.state.as_str())
        {
            return Err(unavailable());
        }
        Ok(frame)
    })();
    if decoded.is_err() {
        context.transport.release();
    }
    decoded
}
struct Shared<T> {
    result: Mutex<Option<Result<T, io::ErrorKind>>>,
}
impl<T> Shared<T> {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(None),
        })
    }
    fn fail(&self, kind: io::ErrorKind) {
        let mut slot = self
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(Err(kind));
        }
    }
}
struct Guard<T> {
    context: Arc<Context>,
    shared: Arc<Shared<T>>,
    complete: bool,
}
impl<T> Drop for Guard<T> {
    fn drop(&mut self) {
        if !self.complete {
            self.context.transport.release();
            self.shared.fail(io::ErrorKind::Other);
        }
    }
}

pub struct RetainedReplacement {
    context: Arc<Context>,
    operation: String,
    service: String,
    source_build: u64,
    target_build: u64,
    settings: Arc<StartupSettings>,
    input: Arc<[u8]>,
    settings_callback: Arc<Callback>,
    input_callback: Arc<Callback>,
    command_callback: Arc<Callback>,
    command: Arc<Shared<GrantedFrontendCommand>>,
    pending: Option<JoinHandle<ReplacementObservation>>,
    observed: Option<ReplacementObservation>,
}
impl Controller {
    /// The UI caller must already own its genuine stopped/joined input parser
    /// and complete capture. `input` remains DATA; Native independently checks
    /// the physical seal, retired readers, signed images, H1 and current F1.
    pub fn prepare_frontend_replacement(
        &self,
        operation: &str,
        service: &str,
        source_build: u64,
        target_build: u64,
        settings: StartupSettings,
        input: Vec<u8>,
    ) -> io::Result<RetainedReplacement> {
        if !valid_hex(operation, 32)
            || !valid_hex(service, 32)
            || source_build == 0
            || target_build == 0
            || input.is_empty()
            || input.len() > MAX_BODY
        {
            return Err(unavailable());
        }
        Ok(RetainedReplacement {
            context: Context::capture(self)?,
            operation: operation.into(),
            service: service.into(),
            source_build,
            target_build,
            settings: Arc::new(settings),
            input: input.into(),
            settings_callback: Arc::new(Callback::bind()?),
            input_callback: Arc::new(Callback::bind()?),
            command_callback: Arc::new(Callback::bind()?),
            command: Shared::new(),
            pending: None,
            observed: None,
        })
    }
    /// Go/service are selectors from the genuine Native command's marker.
    /// Only real entered constructor/kernel/current foreground checks admit E1.
    pub fn prepare_frontend_entry(
        &self,
        operation: &str,
        service: &str,
        go: &str,
    ) -> io::Result<RetainedEntry> {
        if !valid_hex(operation, 32) || !valid_hex(service, 32) || !valid_hex(go, 32) {
            return Err(unavailable());
        }
        Ok(RetainedEntry {
            context: Context::capture(self)?,
            operation: operation.into(),
            service: service.into(),
            go: go.into(),
            callback: Arc::new(Callback::bind()?),
            input: Shared::new(),
            pending: None,
            observed: None,
        })
    }
}
impl RetainedReplacement {
    /// Canceling observation keeps this SAME accepted task, callbacks and
    /// possible command. Never create a second operation after uncertainty.
    pub async fn observe(&mut self) -> ReplacementObservation {
        if let Some(observed) = self.observed {
            return observed;
        }
        if self.pending.is_none() {
            let context = self.context.clone();
            let operation = self.operation.clone();
            let service = self.service.clone();
            let source_build = self.source_build;
            let target_build = self.target_build;
            let settings = self.settings.clone();
            let input = self.input.clone();
            let settings_callback = self.settings_callback.clone();
            let input_callback = self.input_callback.clone();
            let command_callback = self.command_callback.clone();
            let shared = self.command.clone();
            // This guard exists before spawn, including unpolled destruction.
            let guard = Guard {
                context: context.clone(),
                shared: shared.clone(),
                complete: false,
            };
            self.pending = Some(tokio::spawn(async move {
                let mut guard = guard;
                let result = tokio::time::timeout(WHOLE, async {
                    let fields = format!("{} service={} source_build={} target_build={} settings={} input={} command={}",
                        context.fields(), service, source_build, target_build,
                        path(&settings_callback.path), path(&input_callback.path), path(&command_callback.path));
                    let control = async {
                        loop {
                            let body = context.transport.call("controller-retained-prepare", &operation, &fields, Some(&context.member)).await?;
                            let frame = response(&context, &body, &operation, &service, &operation, &["holding", "go"])?;
                            if frame.state == "go" { return Ok::<_, io::Error>(()); }
                            // This is an authenticated observation of the SAME
                            // Native operation waiting for its actual H1 drain.
                            tokio::time::sleep(HOLD_POLL).await;
                        }
                    };
                    let settings_io = serve_settings(&settings_callback, &context, &settings);
                    let input_io = serve_input(&input_callback, &context, &input);
                    let command_io = async {
                        let mut offer = receive_offer(&command_callback, &context, b"G1", &operation).await?;
                        let command = GrantedFrontendCommand::decode(&offer.body, &operation, &service, context.clone())?;
                        {
                            let mut slot = shared.result.lock().map_err(|_| unavailable())?;
                            if slot.is_some() { return Err(unavailable()); }
                            *slot = Some(Ok(command)); // Own it BEFORE physical ACK.
                        }
                        offer.acknowledge().await
                    };
                    tokio::try_join!(control, settings_io, input_io, command_io)?;
                    Ok::<_, io::Error>(())
                }).await;
                let observed = match result {
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
                guard.complete = true;
                observed
            }));
        }
        let observed = match self.pending.as_mut() {
            Some(job) => match job.await {
                Ok(value) => value,
                Err(_) => ReplacementObservation::Unresolved(io::ErrorKind::Other),
            },
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
            Some(Ok(command)) => Ok(command),
            _ => Err(unavailable()),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandData {
    schema: u32,
    program: Vec<u8>,
    argv: Vec<Vec<u8>>,
    env: Vec<(Vec<u8>, Vec<u8>)>,
    cwd: Vec<u8>,
}
/// Private construction requires the genuine G1 physical offer AND its closed
/// correlated Native Go response. It holds no Source/model/UI permission.
pub struct GrantedFrontendCommand {
    context: Arc<Context>,
    data: CommandData,
}
impl std::fmt::Debug for GrantedFrontendCommand {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("GrantedFrontendCommand(<Native-owned command; not UI activation>)")
    }
}
impl GrantedFrontendCommand {
    fn decode(body: &[u8], go: &str, service: &str, context: Arc<Context>) -> io::Result<Self> {
        Ok(Self {
            context,
            data: CommandData::decode(body, go, service)?,
        })
    }
    /// The actual UI/exec owner retains its original input/editor fence and all
    /// transfer custody around this consuming call. Errors remain possible-exec
    /// uncertainty; no fallback/retry or NoExec receipt is returned.
    pub fn execute(self) -> io::Error {
        self.execute_with_resources(|_| Ok(()))
    }
    /// Prepare only already-owned inheritance inside this SAME genuine command's
    /// descriptor exclusion. No Source RPC, async handshake or blocking receive
    /// belongs in the callback. Scope-owned rollback runs before the
    /// original executing guard is released. No separate Go/exec is granted.
    pub fn execute_with_resources(
        self,
        prepare: impl FnOnce(&FrontendCommandExecScope<'_>) -> io::Result<()>,
    ) -> io::Error {
        use std::os::unix::process::CommandExt as _;
        if let Err(error) = self.context.transport.server_pid() {
            return error;
        }
        let _exec = match codex_alab_core::native_fd_custody::executing() {
            Ok(guard) => guard,
            Err(error) => return error,
        };
        let program = OsString::from_vec(self.data.program);
        exec_resources::within_command(&_exec, std::path::Path::new(&program), prepare, || {
            let mut command = std::process::Command::new(&program);
            command.args(self.data.argv.into_iter().map(OsString::from_vec));
            command.env_clear().envs(
                self.data
                    .env
                    .into_iter()
                    .map(|(key, value)| (OsString::from_vec(key), OsString::from_vec(value))),
            );
            command.current_dir(OsString::from_vec(self.data.cwd));
            command.exec()
        })
    }
}
impl CommandData {
    fn decode(body: &[u8], go: &str, service: &str) -> io::Result<Self> {
        let data: Self = serde_json::from_slice(body).map_err(|_| unavailable())?;
        let absolute = |bytes: &[u8]| {
            !bytes.contains(&0)
                && std::path::Path::new(std::ffi::OsStr::from_bytes(bytes)).is_absolute()
        };
        let mut keys = std::collections::BTreeSet::new();
        if data.schema != 1
            || !absolute(&data.program)
            || !absolute(&data.cwd)
            || data.argv.len() < 3
            || data.argv[0] != b"--alab-retained-frontend"
            || data.argv[1] != go.as_bytes()
            || data.argv[2] != service.as_bytes()
            || data.argv.iter().any(|word| word.contains(&0))
            || data.env.iter().any(|(key, value)| {
                key.is_empty()
                    || key.contains(&0)
                    || key.contains(&b'=')
                    || value.contains(&0)
                    || !keys.insert(key)
            })
        {
            return Err(unavailable());
        }
        Ok(data)
    }
}

pub struct RetainedEntry {
    context: Arc<Context>,
    operation: String,
    service: String,
    go: String,
    callback: Arc<Callback>,
    input: Arc<Shared<Vec<u8>>>,
    pending: Option<JoinHandle<EntryObservation>>,
    observed: Option<EntryObservation>,
}
impl RetainedEntry {
    pub async fn observe(&mut self) -> EntryObservation {
        if let Some(observed) = self.observed {
            return observed;
        }
        if self.pending.is_none() {
            let context = self.context.clone();
            let operation = self.operation.clone();
            let service = self.service.clone();
            let go = self.go.clone();
            let callback = self.callback.clone();
            let shared = self.input.clone();
            let guard = Guard {
                context: context.clone(),
                shared: shared.clone(),
                complete: false,
            };
            self.pending = Some(tokio::spawn(async move {
                let mut guard = guard;
                let result = tokio::time::timeout(WHOLE, async {
                    let fields = format!(
                        "{} service={} go={} callback={}",
                        context.fields(),
                        service,
                        go,
                        path(&callback.path)
                    );
                    let control = async {
                        let body = context
                            .transport
                            .call(
                                "controller-retained-enter",
                                &operation,
                                &fields,
                                Some(&context.member),
                            )
                            .await?;
                        response(&context, &body, &operation, &service, &go, &["entered"])?;
                        Ok::<_, io::Error>(())
                    };
                    let physical = async {
                        let mut offer = receive_offer(&callback, &context, b"E1", &go).await?;
                        {
                            let mut slot = shared.result.lock().map_err(|_| unavailable())?;
                            if slot.is_some() {
                                return Err(unavailable());
                            }
                            *slot = Some(Ok(std::mem::take(&mut offer.body))); // BEFORE ACK.
                        }
                        offer.acknowledge().await
                    };
                    tokio::try_join!(control, physical)?;
                    Ok::<_, io::Error>(())
                })
                .await;
                let observed = match result {
                    Ok(Ok(())) => EntryObservation::Entered,
                    Ok(Err(error)) => {
                        context.transport.release();
                        shared.fail(error.kind());
                        EntryObservation::Unresolved(error.kind())
                    }
                    Err(_) => {
                        context.transport.release();
                        shared.fail(io::ErrorKind::TimedOut);
                        EntryObservation::Unresolved(io::ErrorKind::TimedOut)
                    }
                };
                guard.complete = true;
                observed
            }));
        }
        let observed = match self.pending.as_mut() {
            Some(job) => match job.await {
                Ok(value) => value,
                Err(_) => EntryObservation::Unresolved(io::ErrorKind::Other),
            },
            None => EntryObservation::Unresolved(io::ErrorKind::Other),
        };
        self.pending.take();
        self.observed = Some(observed);
        observed
    }
    pub fn take_input(&mut self) -> io::Result<Vec<u8>> {
        if self.observed != Some(EntryObservation::Entered) {
            return Err(unavailable());
        }
        match self.input.result.lock().map_err(|_| unavailable())?.take() {
            Some(Ok(input)) => Ok(input),
            _ => Err(unavailable()),
        }
    }
    /// Use the SAME genuine C2 entry controller for its final F1. Only an
    /// observed native Entered+E1 result can choose the replacement branch;
    /// no caller chooses Some(go) from argv or reconstructs a receipt.
    /// Preparation F1s, if needed, must use their own separate controllers.
    pub async fn begin_frontier(
        &self,
        controller: &Controller,
    ) -> io::Result<super::super::FrontendFrontier> {
        if self.observed != Some(EntryObservation::Entered) || !self.is_for_controller(controller) {
            return Err(unavailable());
        }
        controller
            .begin_retained_frontier(&self.service, Some(&self.go))
            .await
    }
    pub fn is_for_controller(&self, controller: &Controller) -> bool {
        Arc::ptr_eq(&self.context.transport, &controller.transport)
            && self.context.member == controller.member
            && self.context.controller_operation == controller.operation
            && self.context.session == controller.session
    }
    /// Historical actual Entered/E1 custody may REQUEST a fresh Native
    /// frontier on another controller. It does not make that controller
    /// current, replay E1, or assert a new delivery/Go. Native independently
    /// rechecks the same entered constructor/kernel/image and new actual F1/A.
    /// The original first-entry begin_frontier identity rule is unchanged.
    pub async fn rebind_frontier(
        &self,
        controller: &Controller,
    ) -> io::Result<super::super::FrontendFrontier> {
        if self.observed != Some(EntryObservation::Entered) {
            return Err(unavailable());
        }
        controller
            .begin_retained_frontier(&self.service, Some(&self.go))
            .await
    }
}
struct Offer {
    stream: UnixStream,
    nonce: [u8; 32],
    body: Vec<u8>,
}
impl Offer {
    async fn acknowledge(&mut self) -> io::Result<()> {
        self.stream.write_all(&self.nonce).await?;
        self.stream.shutdown().await
    }
}
async fn receive_offer(
    callback: &Callback,
    context: &Context,
    tag: &[u8; 2],
    operation: &str,
) -> io::Result<Offer> {
    let stream = callback.accept(&context.transport, context.peer).await?;
    let offer = read_offer(stream, tag, operation).await?;
    if context.transport.server_pid()? != context.peer {
        return Err(unavailable());
    }
    Ok(offer)
}
async fn read_offer(stream: UnixStream, tag: &[u8; 2], operation: &str) -> io::Result<Offer> {
    let mut header = [0; 74];
    read_exact_no_rights(&stream, &mut header).await?;
    if &header[..2] != tag
        || &header[2..34] != operation.as_bytes()
        || !header[34..66]
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(unavailable());
    }
    let count = usize::try_from(u64::from_be_bytes(
        header[66..74].try_into().map_err(|_| unavailable())?,
    ))
    .map_err(|_| unavailable())?;
    if count == 0 || count > MAX_BODY {
        return Err(unavailable());
    }
    let mut body = vec![0; count];
    read_exact_no_rights(&stream, &mut body).await?;
    require_eof(&stream).await?; // Paired Native half-close BEFORE source ACK.
    Ok(Offer {
        stream,
        nonce: header[34..66].try_into().map_err(|_| unavailable())?,
        body,
    })
}
async fn serve_settings(
    callback: &Callback,
    context: &Context,
    settings: &StartupSettings,
) -> io::Result<()> {
    let mut stream = callback.accept(&context.transport, context.peer).await?;
    if context.transport.server_pid()? != context.peer {
        return Err(unavailable());
    }
    settings.write_and_shutdown(&mut stream).await?;
    require_eof(&stream).await
}
async fn serve_input(callback: &Callback, context: &Context, input: &[u8]) -> io::Result<()> {
    let mut stream = callback.accept(&context.transport, context.peer).await?;
    let mut challenge = [0; 34];
    read_exact_no_rights(&stream, &mut challenge).await?;
    if &challenge[..2] != b"I1"
        || !challenge[2..]
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(unavailable());
    }
    require_eof(&stream).await?;
    if context.transport.server_pid()? != context.peer {
        return Err(unavailable());
    }
    stream.write_all(&challenge[2..]).await?;
    stream
        .write_all(&(input.len() as u64).to_be_bytes())
        .await?;
    stream.write_all(input).await?;
    stream.shutdown().await
}
async fn read_exact_no_rights(stream: &UnixStream, mut body: &mut [u8]) -> io::Result<()> {
    while !body.is_empty() {
        stream.readable().await?;
        match stream.try_io(tokio::io::Interest::READABLE, || {
            crate::retained_fd::receive_chunk(stream, body, 0)
        }) {
            Ok((0, _)) => return Err(unavailable()),
            Ok((count, _)) => body = &mut body[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
async fn require_eof(stream: &UnixStream) -> io::Result<()> {
    loop {
        stream.readable().await?;
        match stream.try_io(tokio::io::Interest::READABLE, || {
            crate::retained_fd::receive_chunk(stream, &mut [0], 0)
        }) {
            Ok((0, _)) => return Ok(()),
            Ok(_) => return Err(unavailable()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
#[path = "frontend_retained_replacement_tests.rs"]
mod tests;

#[path = "frontend_retained_replacement_capture.rs"]
mod capture;
pub use capture::FrontendCapture;
pub use capture::FrontendCaptureWindow;

#[path = "frontend_retained_exec_resources.rs"]
mod exec_resources;
pub use exec_resources::FrontendCommandExecScope;
pub use exec_resources::PreparedFrontendSocket;
