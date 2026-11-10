// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private native members retain one authenticated transport until explicit last-use retirement.

use crate::CALL_TIMEOUT;
use crate::accounts::Session;
use crate::accounts::valid_session;
use crate::conn::Conn;
use crate::conn::{self};
use crate::locate::Endpoint;
use crate::native_source::Ready as SourceFrame;
use codex_alab_core::managed_auth::unavailable;
use codex_alab_core::managed_auth::valid_hex;
use codex_alab_core::native_source as core;
use serde::Deserialize;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

// Only transport uncertainty is retryable; malformed/authentication refusals stay closed.
fn transport_error(error: crate::Error) -> io::Error {
    let kind = match error {
        crate::Error::Io(error) => error.kind(),
        crate::Error::Unanswered => io::ErrorKind::TimedOut,
        crate::Error::Closed => io::ErrorKind::ConnectionReset,
        crate::Error::Busy { .. } => io::ErrorKind::WouldBlock,
        crate::Error::Refused(_) => io::ErrorKind::PermissionDenied,
        crate::Error::Malformed(_) => io::ErrorKind::InvalidData,
        crate::Error::BadRequest(_) => io::ErrorKind::InvalidInput,
    };
    io::Error::new(kind, "native member transport unavailable")
}

fn retryable_work_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted
            | io::ErrorKind::WouldBlock
            | io::ErrorKind::TimedOut
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::UnexpectedEof
    )
}

const MAX_FRAME: usize = 16 * 1024;
const WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);

macro_rules! frame {
    ($name:ident, $member:ty $(, $field:ident: $ty:ty)*) => {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $name {
            schema: u32,
            request: String,
            operation: String,
            session: Session,
            member: $member,
            $($field: $ty,)*
        }
        impl $name {
            fn validate(&self, request: &str, operation: &str, sid: &str, birth: &str, epoch: Option<&str>) -> io::Result<()> {
                if self.schema != 1 || self.request != request || self.operation != operation
                    || !valid_session(&self.session, sid, birth)
                    || epoch.is_some_and(|epoch| self.session.epoch != epoch) {
                    return Err(unavailable());
                }
                Ok(())
            }
        }
    };
}
frame!(CommonFrame, String);
frame!(WaitingFrame, serde_json::Value);
frame!(PreloadFrame, String, registration: String, source: SourceFrame, preload_sequence: String);
frame!(ResamplePreloadFrame, String, state: String, registration: String, source: SourceFrame, preload_sequence: String);
frame!(ReadyCommittedFrame, String, state: String, preload_sequence: String, previous_registration: String, expected_credential_generation: u64, expected_digest: String);
frame!(OwnedReceiptFrame, String, registration: String, preload_sequence: String);
frame!(OwnedClosedFrame, String, preload_sequence: String);
frame!(ReceiptFrame, String, registration: String);

#[path = "source_work_rebind.rs"]
mod work_rebind;

#[path = "source_target.rs"]
pub mod target;

struct Sequence {
    prefix: u64,
    next: Option<u64>,
}
impl Default for Sequence {
    fn default() -> Self {
        Self {
            prefix: core::connection_request_prefix(),
            next: Some(1),
        }
    }
}
impl Sequence {
    fn allocate(&mut self) -> io::Result<String> {
        let counter = self.next.ok_or_else(unavailable)?;
        self.next = counter.checked_add(1);
        Ok(format!("{:016x}{counter:016x}", self.prefix))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Scope {
    slot: String,
    path: std::path::PathBuf,
    account: String,
    user: String,
    assignment: u64,
    intent: Option<String>,
}
impl From<&core::Source> for Scope {
    fn from(source: &core::Source) -> Self {
        Self {
            slot: source.slot.clone(),
            path: source.path.clone(),
            account: source.account.clone(),
            user: source.user.clone(),
            assignment: source.assignment_generation,
            intent: source.native_intent.clone(),
        }
    }
}
#[derive(Default)]
enum Kind {
    #[default]
    Unallocated,
    Managed(Box<Scope>),
    Unrelated(core::NoNativeReader),
}

struct Transport {
    conn: Conn,
    sid: String,
    birth: String,
    // Serializes nonce allocation with the complete wire exchange, including Waiting polls.
    sequence: Mutex<Sequence>,
    serial: tokio::sync::Semaphore,
    epoch: Mutex<Option<String>>,
    endpoint: Endpoint,
}
impl Transport {
    fn epoch(&self) -> io::Result<Option<String>> {
        self.epoch
            .lock()
            .map(|epoch| epoch.clone())
            .map_err(|_| unavailable())
    }
    fn bind_epoch(&self, epoch: &str) -> io::Result<()> {
        let mut current = self.epoch.lock().map_err(|_| unavailable())?;
        if current.as_ref().is_some_and(|current| current != epoch) {
            return Err(unavailable());
        }
        *current = Some(epoch.into());
        Ok(())
    }
    async fn call(
        &self,
        action: &str,
        operation: &str,
        fields: &str,
        member: Option<&str>,
    ) -> io::Result<String> {
        if !valid_hex(operation, 32)
            || member.is_some_and(|member| !valid_hex(member, 32))
            || !self.conn.is_open()
        {
            return Err(unavailable());
        }
        let result = tokio::time::timeout(WAIT, async {
            let _serial = self.serial.acquire().await.map_err(|_| unavailable())?;
            for _ in 0..8 {
                let request = self.sequence.lock().map_err(|_| unavailable())?.allocate()?;
                let command = format!("@{} codex-auth {action} schema=1 request={request} operation={operation}{fields}", self.sid);
                loop {
                    let reply = self.conn.request_status_bounded(&command, MAX_FRAME).await.map_err(transport_error)?;
                    if let Some(body) = reply.status.strip_prefix("OK codex-auth member-waiting ") {
                        let waiting: WaitingFrame = serde_json::from_str(body).map_err(|_| unavailable())?;
                        waiting.validate(&request, operation, &self.sid, &self.birth, self.epoch()?.as_deref())?;
                        let matches = match &waiting.member {
                            serde_json::Value::Null => member.is_none(),
                            serde_json::Value::String(id) => !matches!(action, "member-peer" | "member-target-context" | "member-target-ack") && valid_hex(id, 32) && member.is_none_or(|expected| expected == id),
                            _ => false,
                        };
                        if !matches { return Err(unavailable()) }
                        self.bind_epoch(&waiting.session.epoch)?;
                        tokio::time::sleep(POLL).await;
                        continue;
                    }
                    // A terminal Busy/Stale is not Waiting. Preserve semantics, allocate the next
                    // counter, and let native idempotency resolve the original operation.
                    if reply.status == "ERR codex-auth busy" || reply.status == "ERR codex-auth stale" {
                        tokio::time::sleep(POLL).await;
                        break;
                    }
                    if action == "member-rebound-ready" && reply.status == "ERR codex-auth rebound changed" {
                        return Ok("source-changed".into());
                    }
                    match reply.status.as_str() {
                        "ERR codex-auth denied" => return Err(io::Error::new(io::ErrorKind::PermissionDenied, "native member denied")),
                        "ERR codex-auth unsupported" => return Err(io::Error::new(io::ErrorKind::Unsupported, "native member unsupported")),
                        "ERR codex-auth unavailable" => return Err(io::Error::new(io::ErrorKind::WouldBlock, "native member unavailable")),
                        "ERR codex-auth invalid request" => return Err(io::Error::new(io::ErrorKind::InvalidInput, "native member request refused")),
                        _ => {}
                    }
                    let response_action = match action {
                        "member-use-begin" | "member-use-end" | "member-use-close" => "member-use",
                        "member-open-derived" => "member-open",
                        "member-preload-use" => "member-preload",
                        "member-ready-use" => "member-ready",
                        "member-reader-ready-use" => "member-reader-ready",
                        "member-unavailable-use" => "member-unavailable",
                        "member-refresh-use" => "member-refresh",
                        _ => action,
                    };
                    let body = reply.status.strip_prefix(&format!("OK codex-auth {response_action} ")).ok_or_else(unavailable)?;
                    // Deserialize the exact action below; common validation here also binds epoch
                    // before the first Ready can become an owned member.
                    let identity: IdentityFrame = serde_json::from_str(body).map_err(|_| unavailable())?;
                    let member_matches = if matches!(action, "member-peer" | "member-target-context" | "member-target-ack") {
                        identity.member.is_null() && member.is_none()
                    } else if action == "member-target-open-close" && member.is_none() {
                        identity.member.is_null()
                    } else {
                        identity.member.as_str().is_some_and(|id| valid_hex(id, 32) && member.is_none_or(|expected| expected == id))
                    };
                    if identity.schema != 1 || identity.request != request || identity.operation != operation
                        || !valid_session(&identity.session, &self.sid, &self.birth)
                        || !member_matches {
                        return Err(unavailable());
                    }
                    self.bind_epoch(&identity.session.epoch)?;
                    return Ok(body.into());
                }
            }
            Err(io::Error::new(io::ErrorKind::WouldBlock, "native member busy"))
        }).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "native member unanswered")).and_then(std::convert::identity);
        if result.is_err() {
            self.conn.close();
        }
        result
    }
}
// This first pass locates identity, while the exact closed action structs reject unknown fields.
#[derive(Deserialize)]
struct IdentityFrame {
    schema: u32,
    request: String,
    operation: String,
    session: Session,
    member: serde_json::Value,
}

struct NativeMember {
    target: Option<Arc<target::Role>>,
    transport: Arc<Transport>,
    work: Mutex<Arc<Transport>>,
    work_binding: work_rebind::Binding,
    // A single I/O driver reconnects; transport state uses the short lock above.
    reconnect: tokio::sync::Semaphore,
    work_incarnation: std::sync::atomic::AtomicU64,
    poisoned: std::sync::atomic::AtomicBool,
    retired: std::sync::atomic::AtomicBool,
    id: String,
    open_nonce: String,
    kind: Mutex<Kind>,
    uses: Arc<UseAdmission>,
}
struct UseAdmission {
    // Semantic admission is retained by UseAttempt through uncertain transport retries.
    serial: Arc<tokio::sync::Semaphore>,
    sequence: Mutex<Sequence>,
}
impl Default for UseAdmission {
    fn default() -> Self {
        Self {
            serial: Arc::new(tokio::sync::Semaphore::new(1)),
            sequence: Mutex::default(),
        }
    }
}
impl NativeMember {
    fn new(
        transport: Transport,
        id: String,
        operation: String,
        target: Option<Arc<target::Role>>,
        original: work_rebind::OriginalBody,
    ) -> io::Result<Self> {
        let work_binding = work_rebind::Binding::capture(&transport, &operation, &id, original)?;
        let transport = Arc::new(transport);
        Ok(Self {
            target,
            transport: transport.clone(),
            work: Mutex::new(transport),
            work_binding,
            reconnect: tokio::sync::Semaphore::new(1),
            work_incarnation: Default::default(),
            poisoned: Default::default(),
            retired: Default::default(),
            id,
            open_nonce: operation,
            kind: Default::default(),
            uses: Arc::new(UseAdmission::default()),
        })
    }
    fn work_snapshot(&self) -> io::Result<Arc<Transport>> {
        self.work
            .lock()
            .map(|work| work.clone())
            .map_err(|_| unavailable())
    }
    async fn work_connection(&self) -> io::Result<Arc<Transport>> {
        let _reconnect = self.reconnect.acquire().await.map_err(|_| unavailable())?;
        if self.retired.load(std::sync::atomic::Ordering::Acquire)
            || self.poisoned.load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(unavailable());
        }
        let previous = self.work_snapshot()?;
        if previous.conn.is_open() {
            return Ok(previous);
        }
        let replacement = self.work_binding.reconnect().await?;
        if self.retired.load(std::sync::atomic::Ordering::Acquire)
            || self.poisoned.load(std::sync::atomic::Ordering::Acquire)
        {
            replacement.conn.close();
            return Err(unavailable());
        }
        if self
            .work_incarnation
            .try_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |old| old.checked_add(1),
            )
            .is_err()
        {
            self.poisoned
                .store(true, std::sync::atomic::Ordering::Release);
            replacement.conn.close();
            return Err(unavailable());
        }
        *self.work.lock().map_err(|_| unavailable())? = replacement.clone();
        Ok(replacement)
    }
    async fn work_call(
        &self,
        action: &str,
        operation: &str,
        fields: &str,
        member: Option<&str>,
    ) -> io::Result<String> {
        self.work_connection()
            .await?
            .call(action, operation, fields, member)
            .await
    }
    async fn cleanup_call(
        &self,
        action: &str,
        operation: &str,
        fields: &str,
    ) -> io::Result<String> {
        let transport = connect_transport(&self.transport.endpoint, &self.transport.birth).await?;
        transport.bind_epoch(&self.transport.epoch()?.ok_or_else(unavailable)?)?;
        let result = transport
            .call(action, operation, fields, Some(&self.id))
            .await;
        transport.conn.close();
        result
    }
    async fn retire_context(&self, request: &core::Retirement) -> io::Result<()> {
        request.validate()?;
        let (action, fields) = match request.preload_operation.as_deref() {
            Some(preload) => (
                "member-retire-owned",
                format!(
                    " member={} preload_operation={preload} preload_sequence={} registration={}",
                    self.id,
                    request
                        .preload_sequence
                        .as_ref()
                        .ok_or_else(unavailable)?
                        .encoded(),
                    request.registration.as_deref().unwrap_or("none")
                ),
            ),
            None => (
                "member-retire",
                format!(
                    " member={} registration={}",
                    self.id,
                    request.registration.as_deref().unwrap_or("none")
                ),
            ),
        };
        let body = self
            .cleanup_call(action, &request.operation, &fields)
            .await?;
        match request.preload_sequence.as_ref() {
            Some(sequence) => {
                let frame: OwnedClosedFrame =
                    serde_json::from_str(&body).map_err(|_| unavailable())?;
                frame.validate(
                    &frame.request,
                    &request.operation,
                    &self.transport.sid,
                    &self.transport.birth,
                    self.transport.epoch()?.as_deref(),
                )?;
                if frame.member != self.id || frame.preload_sequence != sequence.encoded() {
                    return Err(unavailable());
                }
            }
            None => {
                let frame: CommonFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
                frame.validate(
                    &frame.request,
                    &request.operation,
                    &self.transport.sid,
                    &self.transport.birth,
                    self.transport.epoch()?.as_deref(),
                )?;
                if frame.member != self.id {
                    return Err(unavailable());
                }
            }
        }
        self.retired
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    fn fail<T>(&self, result: io::Result<T>) -> io::Result<T> {
        if result.is_err() {
            self.poisoned
                .store(true, std::sync::atomic::Ordering::Release);
            if let Ok(work) = self.work_snapshot() {
                work.conn.close();
            }
        }
        result
    }
}
impl NativeMember {
    async fn preload_context(
        &self,
        attempt: &codex_alab_core::native_preload::Body,
        previous: Option<&codex_alab_core::native_preload::Predecessor>,
        borrow: Option<&core::Borrow>,
        use_receipt: Option<&codex_alab_core::native_source_use::UseReceipt>,
    ) -> io::Result<codex_alab_core::native_preload::Resample> {
        self.preload_context_action(attempt, previous, borrow, use_receipt, false)
            .await
    }
    async fn preload_context_action(
        &self,
        attempt: &codex_alab_core::native_preload::Body,
        previous: Option<&codex_alab_core::native_preload::Predecessor>,
        borrow: Option<&core::Borrow>,
        use_receipt: Option<&codex_alab_core::native_source_use::UseReceipt>,
        rebound: bool,
    ) -> io::Result<codex_alab_core::native_preload::Resample> {
        attempt.validate()?;
        let operation = attempt.operation.as_str();
        if let Some(previous) = previous {
            previous.validate()?;
        }
        // Native lock borrowing is a separate required interface, never inferred from IDs.
        if borrow.is_some() {
            return Err(unavailable());
        }
        if matches!(
            *self.kind.lock().map_err(|_| unavailable())?,
            Kind::Unrelated(_)
        ) {
            return Err(unavailable());
        }
        let context = use_fields(use_receipt);
        let target = use_receipt
            .is_none()
            .then_some(self.target.as_ref())
            .flatten();
        let target_fields = target.map_or_else(String::new, |role| role.fields());
        let action = match (use_receipt.is_some(), target.is_some(), previous.is_some()) {
            (true, _, false) => "member-preload-use",
            (true, _, true) => "member-preload-use-resample",
            (false, true, false) => "member-preload-target",
            (false, true, true) => "member-preload-target-resample",
            (false, false, false) => "member-preload",
            (false, false, true) => "member-preload-resample",
        };
        let action = if rebound {
            if previous.is_some() || use_receipt.is_some() || target.is_some() {
                return Err(unavailable());
            }
            "member-preload-rebound"
        } else {
            action
        };
        let predecessor = previous.map_or_else(String::new, |previous| {
            format!(
                " previous_registration={} expected_credential_generation={} expected_digest={}",
                previous.registration, previous.revision.generation, previous.revision.digest,
            )
        });
        let body = self
            .work_call(
                action,
                operation,
                &format!(
                    " member={} preload_sequence={}{}{}{}",
                    self.id,
                    attempt.sequence.encoded(),
                    context,
                    target_fields,
                    predecessor
                ),
                Some(&self.id),
            )
            .await?;
        let result = (|| {
            let frame: PreloadFrame = if let Some(previous) = previous {
                let discriminator: serde_json::Value =
                    serde_json::from_str(&body).map_err(|_| unavailable())?;
                match discriminator
                    .get("state")
                    .and_then(serde_json::Value::as_str)
                {
                    Some("ready_committed") => {
                        let frame: ReadyCommittedFrame =
                            serde_json::from_str(&body).map_err(|_| unavailable())?;
                        frame.validate(
                            &frame.request,
                            operation,
                            &self.transport.sid,
                            &self.transport.birth,
                            self.transport.epoch()?.as_deref(),
                        )?;
                        if frame.member != self.id
                            || frame.state != "ready_committed"
                            || frame.preload_sequence != attempt.sequence.encoded()
                            || frame.previous_registration != previous.registration
                            || frame.expected_credential_generation != previous.revision.generation
                            || frame.expected_digest != previous.revision.digest
                        {
                            return Err(unavailable());
                        }
                        return Ok(codex_alab_core::native_preload::Resample::ReadyCommitted(
                            codex_alab_core::native_preload::ReadyCommitted::from_native(
                                attempt.clone(),
                                previous.clone(),
                            )?,
                        ));
                    }
                    Some("preload") => {
                        let frame: ResamplePreloadFrame =
                            serde_json::from_str(&body).map_err(|_| unavailable())?;
                        frame.validate(
                            &frame.request,
                            operation,
                            &self.transport.sid,
                            &self.transport.birth,
                            self.transport.epoch()?.as_deref(),
                        )?;
                        if frame.state != "preload" {
                            return Err(unavailable());
                        }
                        PreloadFrame {
                            schema: frame.schema,
                            request: frame.request,
                            operation: frame.operation,
                            session: frame.session,
                            member: frame.member,
                            registration: frame.registration,
                            source: frame.source,
                            preload_sequence: frame.preload_sequence,
                        }
                    }
                    _ => return Err(unavailable()),
                }
            } else {
                serde_json::from_str(&body).map_err(|_| unavailable())?
            };
            frame.validate(
                &frame.request,
                operation,
                &self.transport.sid,
                &self.transport.birth,
                self.transport.epoch()?.as_deref(),
            )?;
            if frame.member != self.id
                || frame.preload_sequence != attempt.sequence.encoded()
                || !valid_hex(&frame.registration, 32)
                || frame.source.session() != &frame.session
            {
                return Err(unavailable());
            }
            let source =
                frame
                    .source
                    .into_source(operation, &self.transport.sid, &self.transport.birth)?;
            if let Some(target) = target {
                target.context.validate_source(&source)?;
            }
            let scope = Scope::from(&source);
            let mut kind = self.kind.lock().map_err(|_| unavailable())?;
            if matches!(&*kind, Kind::Managed(expected) if expected.as_ref() != &scope)
                || matches!(*kind, Kind::Unrelated(_))
            {
                return Err(unavailable());
            }
            *kind = Kind::Managed(Box::new(scope));
            Ok(codex_alab_core::native_preload::Resample::Preload(
                core::Preload {
                    registration: frame.registration,
                    source,
                    sequence: codex_alab_core::native_preload::Sequence::parse(
                        &frame.preload_sequence,
                    )?,
                },
            ))
        })();
        self.fail(result)
    }
}
fn use_fields(receipt: Option<&codex_alab_core::native_source_use::UseReceipt>) -> String {
    receipt
        .map(|receipt| {
            format!(
                " use_member={} use_operation={} capability={}",
                receipt.member(),
                receipt.operation(),
                receipt.private_capability()
            )
        })
        .unwrap_or_default()
}
#[derive(Debug)]
struct ReboundSourceChanged;
impl std::fmt::Display for ReboundSourceChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("native rebound source changed")
    }
}
impl std::error::Error for ReboundSourceChanged {}
impl NativeMember {
    async fn ready_context(
        &self,
        operation: &str,
        registration: Option<&str>,
        loaded: &core::Loaded,
        use_receipt: Option<&codex_alab_core::native_source_use::UseReceipt>,
        attempt: Option<&codex_alab_core::native_preload::Body>,
    ) -> io::Result<core::ReadyReceipt> {
        self.ready_context_action(operation, registration, loaded, use_receipt, attempt, false)
            .await
    }
    async fn ready_context_action(
        &self,
        operation: &str,
        registration: Option<&str>,
        loaded: &core::Loaded,
        use_receipt: Option<&codex_alab_core::native_source_use::UseReceipt>,
        attempt: Option<&codex_alab_core::native_preload::Body>,
        rebound: bool,
    ) -> io::Result<core::ReadyReceipt> {
        if matches!(loaded, core::Loaded::NoNativeReader(_)) != attempt.is_none() {
            return Err(unavailable());
        }
        if let Some(attempt) = attempt {
            attempt.validate()?;
            if attempt.operation != operation {
                return Err(unavailable());
            }
        }
        if registration.is_some_and(|value| !valid_hex(value, 32)) {
            return Err(unavailable());
        }
        let (action, fields, unrelated) = match loaded {
            core::Loaded::Source {
                source,
                generations,
            } => {
                source.validate()?;
                if !matches!(&*self.kind.lock().map_err(|_| unavailable())?, Kind::Managed(scope) if scope.as_ref() == &Scope::from(source.as_ref()))
                {
                    return Err(unavailable());
                }
                let registration = registration.ok_or_else(unavailable)?;
                (
                    "member-ready",
                    format!(
                        " member={} registration={registration} credential_generation={} digest={} account={} user={} auth_generation={} owner_generation={} source_generation={} login_generation={}",
                        self.id,
                        source.revision.generation,
                        source.revision.digest,
                        source.account,
                        source.user,
                        generations.auth,
                        generations.owner,
                        generations.source,
                        generations.login
                    ),
                    None,
                )
            }
            core::Loaded::ReaderSource { source } => {
                source.validate()?;
                if !matches!(&*self.kind.lock().map_err(|_| unavailable())?, Kind::Managed(scope) if scope.as_ref() == &Scope::from(source.as_ref()))
                {
                    return Err(unavailable());
                }
                let registration = registration.ok_or_else(unavailable)?;
                (
                    "member-reader-ready",
                    format!(
                        " member={} registration={registration} credential_generation={} digest={} account={} user={}",
                        self.id,
                        source.revision.generation,
                        source.revision.digest,
                        source.account,
                        source.user
                    ),
                    None,
                )
            }
            core::Loaded::PolicyUnavailable => {
                let registration = registration.ok_or_else(unavailable)?;
                if !matches!(
                    *self.kind.lock().map_err(|_| unavailable())?,
                    Kind::Managed(_)
                ) {
                    return Err(unavailable());
                }
                (
                    "member-unavailable",
                    format!(" member={} registration={registration}", self.id),
                    None,
                )
            }
            core::Loaded::NoNativeReader(kind) => {
                let conflict = match &*self.kind.lock().map_err(|_| unavailable())? {
                    Kind::Managed(_) => true,
                    Kind::Unrelated(existing) => existing != kind,
                    Kind::Unallocated => false,
                };
                if operation != self.open_nonce || conflict {
                    return Err(unavailable());
                }
                (
                    "member-unrelated",
                    format!(" member={} kind={}", self.id, unrelated_word(*kind)),
                    Some(*kind),
                )
            }
        };
        let action = if use_receipt.is_some() {
            match action {
                "member-ready" => "member-ready-use",
                "member-reader-ready" => "member-reader-ready-use",
                "member-unavailable" => "member-unavailable-use",
                _ => return Err(unavailable()),
            }
        } else {
            action
        };
        let target = use_receipt
            .is_none()
            .then_some(self.target.as_ref())
            .flatten();
        let action = if target.is_some() {
            match action {
                "member-ready" => "member-ready-target",
                "member-reader-ready" => "member-reader-ready-target",
                "member-unavailable" => "member-unavailable-target",
                _ => return Err(unavailable()),
            }
        } else {
            action
        };
        if let Some(target) = target {
            match loaded {
                core::Loaded::Source { source, .. } | core::Loaded::ReaderSource { source } => {
                    target.context.validate_source(source)?;
                }
                core::Loaded::PolicyUnavailable => {}
                core::Loaded::NoNativeReader(_) => return Err(unavailable()),
            }
        }
        let action = if rebound {
            if action != "member-ready" || use_receipt.is_some() || target.is_some() {
                return Err(unavailable());
            }
            "member-rebound-ready"
        } else {
            action
        };
        let fields = format!(
            "{fields}{}{}{}",
            attempt.map_or_else(String::new, |attempt| format!(
                " preload_sequence={}",
                attempt.sequence.encoded()
            )),
            use_fields(use_receipt),
            target.map_or_else(String::new, |role| {
                format!(
                    "{} assignment_generation={}",
                    role.fields(),
                    role.context.assignment_generation()
                )
            })
        );
        let body = self
            .work_call(action, operation, &fields, Some(&self.id))
            .await?;
        if rebound && body == "source-changed" {
            return Err(io::Error::other(ReboundSourceChanged));
        }
        let result = (|| {
            let native_registration = match attempt {
                Some(attempt) => {
                    let frame: OwnedReceiptFrame =
                        serde_json::from_str(&body).map_err(|_| unavailable())?;
                    frame.validate(
                        &frame.request,
                        operation,
                        &self.transport.sid,
                        &self.transport.birth,
                        self.transport.epoch()?.as_deref(),
                    )?;
                    if frame.member != self.id
                        || frame.preload_sequence != attempt.sequence.encoded()
                    {
                        return Err(unavailable());
                    }
                    frame.registration
                }
                None => {
                    let frame: ReceiptFrame =
                        serde_json::from_str(&body).map_err(|_| unavailable())?;
                    frame.validate(
                        &frame.request,
                        operation,
                        &self.transport.sid,
                        &self.transport.birth,
                        self.transport.epoch()?.as_deref(),
                    )?;
                    if frame.member != self.id {
                        return Err(unavailable());
                    }
                    frame.registration
                }
            };
            let receipt = core::ReadyReceipt {
                registration: native_registration,
            };
            receipt.validate(registration)?;
            if let Some(kind) = unrelated {
                *self.kind.lock().map_err(|_| unavailable())? = Kind::Unrelated(kind);
            }
            Ok(receipt)
        })();
        self.fail(result)
    }
}
impl core::Member for NativeMember {
    #[cfg(debug_assertions)]
    fn debug_close_work(&self) -> io::Result<()> {
        self.work_snapshot()?.conn.close();
        Ok(())
    }
    fn target_resource_ready<'a>(
        &'a self,
        operation: &'a str,
        report: &'a codex_alab_core::native_target::ResourceReport,
    ) -> codex_alab_core::native_target::ResourceFuture<'a> {
        Box::pin(self.target_resource_ready_context(operation, report))
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn work_incarnation(&self) -> u64 {
        self.work_incarnation
            .load(std::sync::atomic::Ordering::Acquire)
    }
    fn reconcile_work(&self) -> core::UnitFuture<'_> {
        Box::pin(async move { self.work_connection().await.map(|_| ()) })
    }
    fn live(&self) -> bool {
        !self.retired.load(std::sync::atomic::Ordering::Acquire)
            && !self.poisoned.load(std::sync::atomic::Ordering::Acquire)
            && self.work_snapshot().is_ok_and(|work| work.conn.is_open())
    }
    fn closed(&self) -> core::UnitFuture<'_> {
        Box::pin(async move {
            self.work_snapshot()?.conn.ended().await;
            Ok(())
        })
    }
    fn open_derived<'a>(
        &'a self,
        operation: &'a str,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> core::MemberFuture<'a> {
        Box::pin(async move {
            if !self.live() || receipt.member() != self.id {
                return Err(unavailable());
            }
            // A child owns its work channel. A withheld parent reply cannot block it.
            // Native admission still requires the original parent's actual active
            // SourceUse/App custody; this is not a weaker generic peer fallback.
            let peer = work_rebind::PeerIdentity::capture(&self.transport)?;
            let hello = format!(
                "{:016x}{:016x}",
                core::connection_request_prefix(),
                core::connection_request_prefix()
            );
            let transport = peer.reconnect_peer(&hello).await?;
            let body = transport
                .call(
                    "member-open-derived",
                    operation,
                    &use_fields(Some(receipt)),
                    None,
                )
                .await?;
            let frame: CommonFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
            frame.validate(
                &frame.request,
                operation,
                &transport.sid,
                &transport.birth,
                transport.epoch()?.as_deref(),
            )?;
            if !valid_hex(&frame.member, 32) || frame.member == self.id {
                return Err(unavailable());
            }
            Ok(Arc::new(NativeMember::new(
                transport,
                frame.member,
                operation.into(),
                self.target.as_ref().map(Arc::clone),
                work_rebind::OriginalBody::Derived {
                    use_member: receipt.member().into(),
                    use_operation: receipt.operation().into(),
                    capability: receipt.private_capability().into(),
                },
            )?) as Arc<dyn core::Member>)
        })
    }
    fn preload_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        borrow: Option<&'a core::Borrow>,
    ) -> core::PreloadFuture<'a> {
        Box::pin(async move {
            match self.preload_context(attempt, None, borrow, None).await? {
                codex_alab_core::native_preload::Resample::Preload(packet) => Ok(packet),
                codex_alab_core::native_preload::Resample::ReadyCommitted(_) => Err(unavailable()),
            }
        })
    }
    fn preload_owned_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> core::PreloadFuture<'a> {
        Box::pin(async move {
            match self
                .preload_context(attempt, None, None, Some(receipt))
                .await?
            {
                codex_alab_core::native_preload::Resample::Preload(packet) => Ok(packet),
                codex_alab_core::native_preload::Resample::ReadyCommitted(_) => Err(unavailable()),
            }
        })
    }
    fn resample_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        previous: &'a codex_alab_core::native_preload::Predecessor,
        borrow: Option<&'a core::Borrow>,
    ) -> core::ResampleFuture<'a> {
        Box::pin(self.preload_context(attempt, Some(previous), borrow, None))
    }
    fn resample_owned_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        previous: &'a codex_alab_core::native_preload::Predecessor,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> core::ResampleFuture<'a> {
        Box::pin(self.preload_context(attempt, Some(previous), None, Some(receipt)))
    }
    fn preload<'a>(
        &'a self,
        operation: &'a str,
        borrow: Option<&'a core::Borrow>,
    ) -> core::PreloadFuture<'a> {
        Box::pin(async move {
            let _ = (operation, borrow);
            Err(unavailable())
        })
    }
    fn preload_use<'a>(
        &'a self,
        operation: &'a str,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> core::PreloadFuture<'a> {
        Box::pin(async move {
            let _ = (operation, receipt);
            Err(unavailable())
        })
    }
    fn rebound_ready_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        registration: &'a str,
        loaded: &'a core::Loaded,
    ) -> core::ReboundReadyFuture<'a> {
        Box::pin(async move {
            match self
                .ready_context_action(
                    &attempt.operation,
                    Some(registration),
                    loaded,
                    None,
                    Some(attempt),
                    true,
                )
                .await
            {
                Ok(receipt) => Ok(core::ReboundReadyOutcome::Ready(
                    codex_alab_core::native_preload::Ready::from_native(
                        attempt.clone(),
                        receipt.registration,
                    )?,
                )),
                Err(error)
                    if error.get_ref().is_some_and(
                        <dyn std::error::Error + Send + Sync>::is::<ReboundSourceChanged>,
                    ) =>
                {
                    Ok(core::ReboundReadyOutcome::SourceChanged)
                }
                Err(error) => Err(error),
            }
        })
    }
    fn rebound_preload_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
    ) -> core::PreloadFuture<'a> {
        Box::pin(async move {
            match self
                .preload_context_action(attempt, None, None, None, true)
                .await?
            {
                codex_alab_core::native_preload::Resample::Preload(packet) => Ok(packet),
                codex_alab_core::native_preload::Resample::ReadyCommitted(_) => Err(unavailable()),
            }
        })
    }
    fn ready_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        registration: &'a str,
        loaded: &'a core::Loaded,
    ) -> core::OwnedReadyFuture<'a> {
        Box::pin(async move {
            let receipt = self
                .ready_context(
                    &attempt.operation,
                    Some(registration),
                    loaded,
                    None,
                    Some(attempt),
                )
                .await?;
            codex_alab_core::native_preload::Ready::from_native(
                attempt.clone(),
                receipt.registration,
            )
        })
    }
    fn ready_owned_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        registration: &'a str,
        loaded: &'a core::Loaded,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> core::OwnedReadyFuture<'a> {
        Box::pin(async move {
            let ready = self
                .ready_context(
                    &attempt.operation,
                    Some(registration),
                    loaded,
                    Some(receipt),
                    Some(attempt),
                )
                .await?;
            codex_alab_core::native_preload::Ready::from_native(attempt.clone(), ready.registration)
        })
    }
    fn ready<'a>(
        &'a self,
        operation: &'a str,
        registration: Option<&'a str>,
        loaded: &'a core::Loaded,
    ) -> core::ReadyFuture<'a> {
        Box::pin(self.ready_context(operation, registration, loaded, None, None))
    }
    fn ready_use<'a>(
        &'a self,
        operation: &'a str,
        registration: &'a str,
        loaded: &'a core::Loaded,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> core::ReadyFuture<'a> {
        Box::pin(self.ready_context(operation, Some(registration), loaded, Some(receipt), None))
    }
    fn begin_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_source_use::UseAttempt,
        registration: &'a str,
    ) -> codex_alab_core::native_source_use::UseFuture<'a> {
        Box::pin(async move {
            use codex_alab_core::native_source_use::UseBody;
            use codex_alab_core::native_source_use::UseOutcome;
            if !attempt
                .hold_admission(Arc::clone(&self.uses.serial))
                .await?
            {
                return Ok(UseOutcome::Unsent);
            }
            let body = match attempt.body()? {
                Some(body) => body,
                None => {
                    let operation = self
                        .uses
                        .sequence
                        .lock()
                        .map_err(|_| unavailable())?
                        .allocate()?;
                    attempt.assign(UseBody {
                        member: self.id.clone(),
                        operation,
                        registration: registration.into(),
                    })?
                }
            };
            if body.member != self.id {
                return Err(unavailable());
            }
            let response = self
                .work_call(
                    "member-use-begin",
                    &body.operation,
                    &format!(" member={} registration={}", self.id, body.registration),
                    Some(&self.id),
                )
                .await?;
            parse_use(
                &response,
                &body.operation,
                &self.id,
                &self.transport.sid,
                &self.transport.birth,
            )
        })
    }
    fn close_use<'a>(
        &'a self,
        body: &'a codex_alab_core::native_source_use::UseBody,
    ) -> codex_alab_core::native_source_use::UseFuture<'a> {
        Box::pin(async move {
            if body.member != self.id {
                return Err(unavailable());
            }
            let response = self
                .cleanup_call(
                    "member-use-close",
                    &body.operation,
                    &format!(" member={} registration={}", self.id, body.registration),
                )
                .await?;
            parse_use(
                &response,
                &body.operation,
                &self.id,
                &self.transport.sid,
                &self.transport.birth,
            )
        })
    }
    fn end_use<'a>(
        &'a self,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> core::UnitFuture<'a> {
        Box::pin(async move {
            if receipt.member() != self.id {
                return Err(unavailable());
            }
            let response = self
                .cleanup_call(
                    "member-use-end",
                    receipt.operation(),
                    &format!(
                        " member={} capability={}",
                        self.id,
                        receipt.private_capability()
                    ),
                )
                .await?;
            match parse_use(
                &response,
                receipt.operation(),
                &self.id,
                &self.transport.sid,
                &self.transport.birth,
            )? {
                codex_alab_core::native_source_use::UseOutcome::Closed => Ok(()),
                _ => Err(unavailable()),
            }
        })
    }
    fn refresh_use<'a>(
        &'a self,
        request: &'a codex_alab_core::managed_auth::Request,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> codex_alab_core::managed_auth::RefreshFuture<'a> {
        Box::pin(async move {
            if !valid_hex(&request.operation, 32)
                || !valid_hex(&request.expected_digest, 64)
                || !matches!(&*self.kind.lock().map_err(|_| unavailable())?, Kind::Managed(scope) if scope.slot == request.slot)
            {
                return Err(unavailable());
            }
            let fields = format!(
                " member={} slot={} expected_digest={}{}",
                self.id,
                request.slot,
                request.expected_digest,
                use_fields(Some(receipt))
            );
            let mut observed: Option<codex_alab_core::managed_auth::Revision> = None;
            loop {
                let body = self
                    .work_call(
                        "member-refresh-use",
                        &request.operation,
                        &fields,
                        Some(&self.id),
                    )
                    .await?;
                let result = (|| {
                    let frame: RefreshFrame =
                        serde_json::from_str(&body).map_err(|_| unavailable())?;
                    frame.validate(
                        &frame.request,
                        &request.operation,
                        &self.transport.sid,
                        &self.transport.birth,
                        self.transport.epoch()?.as_deref(),
                    )?;
                    if frame.member != self.id
                        || frame.slot != request.slot
                        || frame.expected_digest != request.expected_digest
                        || frame.generation == 0
                        || !valid_hex(&frame.digest, 64)
                    {
                        return Err(unavailable());
                    }
                    let revision = codex_alab_core::managed_auth::Revision {
                        generation: frame.generation,
                        digest: frame.digest,
                    };
                    if observed.as_ref().is_some_and(|previous| {
                        revision.generation < previous.generation
                            || (revision.generation == previous.generation
                                && revision.digest != previous.digest)
                    }) {
                        return Err(unavailable());
                    }
                    observed = Some(revision.clone());
                    match frame.state.as_str() {
                        "waiting" => Ok(None),
                        "ready" => Ok(Some(revision)),
                        _ => Err(unavailable()),
                    }
                })();
                if let Some(revision) = self.fail(result)? {
                    return Ok(revision);
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
    }
    fn retire_captured<'a>(&'a self, request: &'a core::Retirement) -> core::UnitFuture<'a> {
        Box::pin(self.retire_context(request))
    }
    fn retire<'a>(&'a self, registration: Option<&'a str>) -> core::UnitFuture<'a> {
        Box::pin(async move {
            self.retire_context(&core::Retirement {
                operation: crate::accounts::request_id(),
                preload_operation: None,
                preload_sequence: None,
                registration: registration.map(str::to_owned),
            })
            .await
        })
    }
}
fn unrelated_word(kind: core::NoNativeReader) -> &'static str {
    match kind {
        core::NoNativeReader::ApiKey => "api_key",
        core::NoNativeReader::External => "external",
        core::NoNativeReader::Workload => "workload",
        core::NoNativeReader::BedrockApiKey => "bedrock_api_key",
        core::NoNativeReader::BedrockAccessKeys => "bedrock_access_keys",
        core::NoNativeReader::Headers => "headers",
        core::NoNativeReader::AgentIdentity => "agent_identity",
        core::NoNativeReader::PersonalAccessToken => "personal_access_token",
        core::NoNativeReader::NoCredentials => "no_credentials",
    }
}

async fn connect_transport(endpoint: &Endpoint, birth: &str) -> io::Result<Transport> {
    let opened = conn::open(&endpoint.sock, CALL_TIMEOUT)
        .await
        .map_err(transport_error)?;
    let transport = Transport {
        conn: Conn::spawn(opened, CALL_TIMEOUT),
        sid: endpoint.sid.clone(),
        birth: birth.into(),
        sequence: Default::default(),
        serial: tokio::sync::Semaphore::new(1),
        epoch: Default::default(),
        endpoint: endpoint.clone(),
    };
    Ok(transport)
}

/// No supervision claim or presentation setting is used for this private constructor channel.
pub async fn open_at(
    endpoint: &Endpoint,
    open_nonce: &str,
    birth: &str,
) -> io::Result<Arc<dyn core::Member>> {
    if !crate::locate::valid_sid(&endpoint.sid)
        || !valid_hex(open_nonce, 32)
        || birth.is_empty()
        || birth.len() > 128
        || birth.chars().any(char::is_control)
    {
        return Err(unavailable());
    }
    let mut transport = connect_transport(endpoint, birth).await?;
    let hello = format!(
        "{:016x}{:016x}",
        core::connection_request_prefix(),
        core::connection_request_prefix()
    );
    let peer = work_rebind::PeerIdentity::establish(&transport, &hello).await?;
    let body = loop {
        match transport.call("member-open", open_nonce, "", None).await {
            Ok(body) => break body,
            Err(error) if retryable_work_error(&error) => loop {
                let hello = format!(
                    "{:016x}{:016x}",
                    core::connection_request_prefix(),
                    core::connection_request_prefix()
                );
                match peer.reconnect_peer(&hello).await {
                    Ok(replacement) => {
                        transport = replacement;
                        break;
                    }
                    Err(error) if retryable_work_error(&error) => tokio::time::sleep(POLL).await,
                    Err(error) => return Err(error),
                }
            },
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        let frame: CommonFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
        frame.validate(
            &frame.request,
            open_nonce,
            &endpoint.sid,
            birth,
            transport.epoch()?.as_deref(),
        )?;
        if !valid_hex(&frame.member, 32) {
            return Err(unavailable());
        }
        Ok(frame.member)
    })();
    match result {
        Ok(id) => Ok(Arc::new(NativeMember::new(
            transport,
            id,
            open_nonce.into(),
            None,
            work_rebind::OriginalBody::Ordinary,
        )?)),
        Err(error) => {
            transport.conn.close();
            Err(error)
        }
    }
}

frame!(RefreshFrame, String, slot: String, expected_digest: String, state: String, generation: u64, digest: String);
frame!(UseFrame, String, state: String, capability: serde_json::Value);
fn parse_use(
    body: &str,
    operation: &str,
    member: &str,
    sid: &str,
    birth: &str,
) -> io::Result<codex_alab_core::native_source_use::UseOutcome> {
    use codex_alab_core::native_source_use::UseOutcome;
    use codex_alab_core::native_source_use::UseReceipt;
    let frame: UseFrame = serde_json::from_str(body).map_err(|_| unavailable())?;
    frame.validate(&frame.request, operation, sid, birth, None)?;
    if frame.member != member || u64::from_str_radix(&frame.operation[16..], 16).unwrap_or(0) == 0 {
        return Err(unavailable());
    }
    match (frame.state.as_str(), frame.capability) {
        ("active", serde_json::Value::String(capability)) => Ok(UseOutcome::Active(
            UseReceipt::from_native(frame.member, frame.operation, capability)?,
        )),
        ("closed", serde_json::Value::Null) => Ok(UseOutcome::Closed),
        _ => Err(unavailable()),
    }
}

#[cfg(test)]
#[path = "source_member_tests.rs"]
mod tests;
