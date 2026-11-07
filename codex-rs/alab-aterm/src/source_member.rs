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
frame!(PreloadFrame, String, registration: String, source: SourceFrame);
frame!(ReceiptFrame, String, registration: String);

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
                            serde_json::Value::String(id) => valid_hex(id, 32) && member.is_none_or(|expected| expected == id),
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
                        "member-refresh-use" => "member-refresh",
                        _ => action,
                    };
                    let body = reply.status.strip_prefix(&format!("OK codex-auth {response_action} ")).ok_or_else(unavailable)?;
                    // Deserialize the exact action below; common validation here also binds epoch
                    // before the first Ready can become an owned member.
                    let identity: IdentityFrame = serde_json::from_str(body).map_err(|_| unavailable())?;
                    if identity.schema != 1 || identity.request != request || identity.operation != operation
                        || !valid_session(&identity.session, &self.sid, &self.birth)
                        || !valid_hex(&identity.member, 32)
                        || member.is_some_and(|member| member != identity.member) {
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
    member: String,
}

struct NativeMember {
    transport: Arc<Transport>,
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
    async fn cleanup_call(
        &self,
        action: &str,
        operation: &str,
        fields: &str,
    ) -> io::Result<String> {
        let transport = connect_transport(&self.transport.endpoint, &self.transport.birth).await?;
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
                    " member={} preload_operation={preload} registration={}",
                    self.id,
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
        let frame: CommonFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
        frame.validate(
            &frame.request,
            &request.operation,
            &self.transport.sid,
            &self.transport.birth,
            None,
        )?;
        if frame.member != self.id {
            return Err(unavailable());
        }
        self.retired
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    fn fail<T>(&self, result: io::Result<T>) -> io::Result<T> {
        if result.is_err() {
            self.transport.conn.close();
        }
        result
    }
}
impl NativeMember {
    async fn preload_context(
        &self,
        operation: &str,
        borrow: Option<&core::Borrow>,
        use_receipt: Option<&codex_alab_core::native_source_use::UseReceipt>,
    ) -> io::Result<core::Preload> {
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
        let body = self
            .transport
            .call(
                if use_receipt.is_some() {
                    "member-preload-use"
                } else {
                    "member-preload"
                },
                operation,
                &format!(" member={}{}", self.id, context),
                Some(&self.id),
            )
            .await?;
        let result = (|| {
            let frame: PreloadFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
            frame.validate(
                &frame.request,
                operation,
                &self.transport.sid,
                &self.transport.birth,
                self.transport.epoch()?.as_deref(),
            )?;
            if frame.member != self.id
                || !valid_hex(&frame.registration, 32)
                || frame.source.session() != &frame.session
            {
                return Err(unavailable());
            }
            let source =
                frame
                    .source
                    .into_source(operation, &self.transport.sid, &self.transport.birth)?;
            let scope = Scope::from(&source);
            let mut kind = self.kind.lock().map_err(|_| unavailable())?;
            if matches!(&*kind, Kind::Managed(expected) if expected.as_ref() != &scope)
                || matches!(*kind, Kind::Unrelated(_))
            {
                return Err(unavailable());
            }
            *kind = Kind::Managed(Box::new(scope));
            Ok(core::Preload {
                registration: frame.registration,
                source,
            })
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
impl NativeMember {
    async fn ready_context(
        &self,
        operation: &str,
        registration: Option<&str>,
        loaded: &core::Loaded,
        use_receipt: Option<&codex_alab_core::native_source_use::UseReceipt>,
    ) -> io::Result<core::ReadyReceipt> {
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
                _ => return Err(unavailable()),
            }
        } else {
            action
        };
        let fields = format!("{fields}{}", use_fields(use_receipt));
        let body = self
            .transport
            .call(action, operation, &fields, Some(&self.id))
            .await?;
        let result = (|| {
            let frame: ReceiptFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
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
            let receipt = core::ReadyReceipt {
                registration: frame.registration,
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
    fn id(&self) -> &str {
        &self.id
    }
    fn live(&self) -> bool {
        !self.retired.load(std::sync::atomic::Ordering::Acquire) && self.transport.conn.is_open()
    }
    fn closed(&self) -> core::UnitFuture<'_> {
        Box::pin(async move {
            self.transport.conn.ended().await;
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
            let transport =
                connect_transport(&self.transport.endpoint, &self.transport.birth).await?;
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
            Ok(Arc::new(NativeMember {
                transport: Arc::new(transport),
                retired: Default::default(),
                id: frame.member,
                open_nonce: operation.into(),
                kind: Default::default(),
                uses: Arc::new(UseAdmission::default()),
            }) as Arc<dyn core::Member>)
        })
    }
    fn preload<'a>(
        &'a self,
        operation: &'a str,
        borrow: Option<&'a core::Borrow>,
    ) -> core::PreloadFuture<'a> {
        Box::pin(self.preload_context(operation, borrow, None))
    }
    fn preload_use<'a>(
        &'a self,
        operation: &'a str,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> core::PreloadFuture<'a> {
        Box::pin(self.preload_context(operation, None, Some(receipt)))
    }
    fn ready<'a>(
        &'a self,
        operation: &'a str,
        registration: Option<&'a str>,
        loaded: &'a core::Loaded,
    ) -> core::ReadyFuture<'a> {
        Box::pin(self.ready_context(operation, registration, loaded, None))
    }
    fn ready_use<'a>(
        &'a self,
        operation: &'a str,
        registration: &'a str,
        loaded: &'a core::Loaded,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> core::ReadyFuture<'a> {
        Box::pin(self.ready_context(operation, Some(registration), loaded, Some(receipt)))
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
                .transport
                .call(
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
                    .transport
                    .call(
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
    let transport = connect_transport(endpoint, birth).await?;
    let body = transport.call("member-open", open_nonce, "", None).await?;
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
        Ok(id) => Ok(Arc::new(NativeMember {
            transport: Arc::new(transport),
            retired: Default::default(),
            id,
            open_nonce: open_nonce.into(),
            kind: Default::default(),
            uses: Arc::new(UseAdmission::default()),
        })),
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
