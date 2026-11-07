// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Owned native-use admission and release. Local Drop and EOF are never native ACKs.

use crate::managed_auth::unavailable;
use crate::managed_auth::valid_hex;
use crate::native_source::Member;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::Notify;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct UseBody {
    pub member: String,
    pub operation: String,
    pub registration: String,
}
impl std::fmt::Debug for UseBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeUseBody(<private>)")
    }
}

#[derive(Default)]
pub struct UseAttempt {
    body: Mutex<Option<UseBody>>,
    admission: Mutex<Option<OwnedSemaphorePermit>>,
    cancelled: AtomicBool,
    changed: Notify,
}
impl UseAttempt {
    pub fn body(&self) -> io::Result<Option<UseBody>> {
        self.body
            .lock()
            .map(|body| body.clone())
            .map_err(|_| unavailable())
    }
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.changed.notify_waiters();
    }
    /// The permit survives transport errors and belongs to the semantic attempt, not an RPC.
    pub async fn hold_admission(&self, admission: Arc<Semaphore>) -> io::Result<bool> {
        if self.admission.lock().map_err(|_| unavailable())?.is_some() {
            return Ok(true);
        }
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.cancelled() && self.body()?.is_none() {
                return Ok(false);
            }
            tokio::select! {
                permit = admission.clone().acquire_owned() => {
                    let permit = permit.map_err(|_| unavailable())?;
                    if self.cancelled() && self.body()?.is_none() { return Ok(false) }
                    *self.admission.lock().map_err(|_| unavailable())? = Some(permit);
                    return Ok(true);
                },
                _ = &mut changed => {},
            }
        }
    }
    /// Called only after the semantic admission permit is held and before the first write.
    pub fn assign(&self, body: UseBody) -> io::Result<UseBody> {
        if !valid_hex(&body.member, 32)
            || !valid_hex(&body.operation, 32)
            || !valid_hex(&body.registration, 32)
            || u64::from_str_radix(&body.operation[16..], 16).unwrap_or(0) == 0
            || self.admission.lock().map_err(|_| unavailable())?.is_none()
        {
            return Err(unavailable());
        }
        let mut original = self.body.lock().map_err(|_| unavailable())?;
        if let Some(original) = original.as_ref() {
            if original.member != body.member
                || original.operation != body.operation
                || original.registration != body.registration
            {
                return Err(unavailable());
            }
            return Ok(original.clone());
        }
        if self.cancelled() {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        *original = Some(body.clone());
        self.changed.notify_waiters();
        Ok(body)
    }
    pub fn finish_admission(&self) -> io::Result<()> {
        self.admission.lock().map_err(|_| unavailable())?.take();
        Ok(())
    }
}
impl std::fmt::Debug for UseAttempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeUseAttempt(<private>)")
    }
}

pub struct UseReceipt {
    member: String,
    operation: String,
    capability: zeroize::Zeroizing<String>,
}
impl UseReceipt {
    pub fn from_native(member: String, operation: String, capability: String) -> io::Result<Self> {
        let capability = zeroize::Zeroizing::new(capability);
        if !valid_hex(&member, 32)
            || !valid_hex(&operation, 32)
            || u64::from_str_radix(&operation[16..], 16).unwrap_or(0) == 0
            || !valid_hex(&capability, 64)
        {
            return Err(unavailable());
        }
        Ok(Self {
            member,
            operation,
            capability,
        })
    }
    pub fn member(&self) -> &str {
        &self.member
    }
    pub fn operation(&self) -> &str {
        &self.operation
    }
    pub fn private_capability(&self) -> &str {
        self.capability.as_str()
    }
    fn matches(&self, body: &UseBody) -> bool {
        self.member == body.member && self.operation == body.operation
    }
}
impl std::fmt::Debug for UseReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeUseReceipt(<private>)")
    }
}
pub enum UseOutcome {
    Active(UseReceipt),
    Closed,
    /// Local evidence only: cancellation won before nonce allocation or a wire send.
    Unsent,
}
pub type UseFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<UseOutcome>> + Send + 'a>>;

fn transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::TimedOut
            | io::ErrorKind::Interrupted
            | io::ErrorKind::WouldBlock
            | io::ErrorKind::NotFound
            | io::ErrorKind::UnexpectedEof
    )
}
#[derive(Default)]
struct Progress {
    active: Option<Arc<UseReceipt>>,
    begin_done: bool,
    ended: bool,
    failed: bool,
}
struct State {
    member: Arc<dyn Member>,
    registration: String,
    attempt: UseAttempt,
    progress: Mutex<Progress>,
    closing: AtomicBool,
    changed: Notify,
    domains: Mutex<Vec<std::sync::Weak<crate::assignment::Domain>>>,
    _lifetime: Option<crate::native_source_lifetime::Lifetime>,
}
static STATES: Mutex<Vec<std::sync::Weak<State>>> = Mutex::new(Vec::new());
pub(crate) fn settled_for(domain: &Arc<crate::assignment::Domain>) -> bool {
    let Ok(mut states) = STATES.lock() else {
        return false;
    };
    states.retain(|state| state.strong_count() != 0);
    states
        .iter()
        .filter_map(std::sync::Weak::upgrade)
        .all(|state| {
            let relevant = state.domains.lock().map_or(true, |domains| {
                domains
                    .iter()
                    .any(|entry| std::sync::Weak::ptr_eq(entry, &Arc::downgrade(domain)))
            });
            !relevant || state.progress.lock().is_ok_and(|progress| progress.ended)
        })
}
fn signal_changed(state: &State) {
    state.changed.notify_waiters();
    if let Ok(domains) = state.domains.lock() {
        for domain in domains.iter().filter_map(std::sync::Weak::upgrade) {
            domain.native_use_changed();
        }
    }
}
struct Owner(Arc<State>);
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.closing.store(true, Ordering::Release);
        self.0.attempt.cancel();
        signal_changed(&self.0);
    }
}
/// Retained only by admitted Activity and its inherited uses, never by cached factories.
#[derive(Clone)]
pub struct ParentUseLease(Arc<Owner>);
impl std::fmt::Debug for ParentUseLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeParentUse(<private>)")
    }
}
#[derive(Clone)]
pub(crate) struct ParentUseRef(std::sync::Weak<Owner>);
impl ParentUseRef {
    pub(crate) fn upgrade(&self) -> Option<ParentUseLease> {
        self.0
            .upgrade()
            .filter(|owner| !owner.0.closing.load(Ordering::Acquire))
            .map(ParentUseLease)
    }
}
impl ParentUseLease {
    pub(crate) fn settled(&self) -> bool {
        self.0
            .0
            .progress
            .lock()
            .is_ok_and(|progress| progress.ended)
    }
    pub(crate) fn can_replace(&self, registration: &str) -> bool {
        self.settled() && registration != self.0.0.registration
    }

    pub(crate) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub(crate) fn downgrade(&self) -> ParentUseRef {
        ParentUseRef(Arc::downgrade(&self.0))
    }

    pub fn start(member: Arc<dyn Member>, registration: String) -> io::Result<Self> {
        Self::start_inner(member, registration, None, None)
    }
    pub(crate) fn start_for(
        member: Arc<dyn Member>,
        registration: String,
        domain: Arc<crate::assignment::Domain>,
        lifetime: crate::native_source_lifetime::Lifetime,
    ) -> io::Result<Self> {
        Self::start_inner(member, registration, Some(domain), Some(lifetime))
    }
    fn start_inner(
        member: Arc<dyn Member>,
        registration: String,
        domain: Option<Arc<crate::assignment::Domain>>,
        lifetime: Option<crate::native_source_lifetime::Lifetime>,
    ) -> io::Result<Self> {
        if !member.live() || !valid_hex(&registration, 32) {
            return Err(unavailable());
        }
        let state = Arc::new(State {
            member,
            registration,
            attempt: UseAttempt::default(),
            progress: Mutex::default(),
            closing: AtomicBool::new(false),
            changed: Notify::new(),
            domains: Mutex::new(domain.iter().map(Arc::downgrade).collect()),
            _lifetime: lifetime,
        });
        {
            let mut states = STATES.lock().map_err(|_| unavailable())?;
            states.retain(|state| state.strong_count() != 0);
            states.push(Arc::downgrade(&state));
        }
        tokio::runtime::Handle::try_current()
            .map_err(|_| unavailable())?
            .spawn(begin(Arc::clone(&state)));
        tokio::spawn(cleanup(Arc::clone(&state)));
        Ok(Self(Arc::new(Owner(state))))
    }
    pub(crate) fn cover_domain(&self, domain: &Arc<crate::assignment::Domain>) -> io::Result<()> {
        let mut domains = self.0.0.domains.lock().map_err(|_| unavailable())?;
        let weak = Arc::downgrade(domain);
        if !domains
            .iter()
            .any(|entry| std::sync::Weak::ptr_eq(entry, &weak))
        {
            domains.push(weak);
        }
        Ok(())
    }
    pub async fn ready(&self) -> io::Result<Arc<UseReceipt>> {
        loop {
            let changed = self.0.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let progress = self.0.0.progress.lock().map_err(|_| unavailable())?;
                if self.0.0.closing.load(Ordering::Acquire)
                    || progress.failed
                    || progress.ended
                    || !self.0.0.member.live()
                {
                    return Err(unavailable());
                }
                if let Some(receipt) = &progress.active {
                    return Ok(Arc::clone(receipt));
                }
            }
            changed.await;
        }
    }
}
async fn begin(state: Arc<State>) {
    loop {
        if state
            .progress
            .lock()
            .map_or(true, |progress| progress.ended)
        {
            return;
        }
        let pending = state.member.begin_use(&state.attempt, &state.registration);
        tokio::pin!(pending);
        let outcome = loop {
            let changed = state.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            // Only actual cleanup ACK (or proven never-sent) can release this wait.
            if state.progress.lock().is_ok_and(|progress| progress.ended) {
                return;
            }
            tokio::select! { result = &mut pending => break result, _ = &mut changed => {} }
        };
        match outcome {
            Ok(UseOutcome::Active(receipt)) => {
                let valid = state
                    .attempt
                    .body()
                    .ok()
                    .flatten()
                    .is_some_and(|body| receipt.matches(&body));
                if let Ok(mut progress) = state.progress.lock() {
                    progress.begin_done = true;
                    if !progress.ended && valid {
                        progress.active = Some(Arc::new(receipt));
                    } else if !valid {
                        progress.failed = true;
                        state.closing.store(true, Ordering::Release);
                    }
                }
                let _ = state.attempt.finish_admission();
                signal_changed(&state);
                return;
            }
            Ok(UseOutcome::Closed | UseOutcome::Unsent) => {
                if let Ok(mut progress) = state.progress.lock() {
                    progress.begin_done = true;
                    progress.failed = true;
                    progress.ended = true;
                }
                let _ = state.attempt.finish_admission();
                signal_changed(&state);
                return;
            }
            Err(error) => {
                if !transient(&error) || state.closing.load(Ordering::Acquire) {
                    if let Ok(mut progress) = state.progress.lock() {
                        progress.begin_done = true;
                        progress.failed = true;
                    }
                    state.closing.store(true, Ordering::Release);
                    signal_changed(&state);
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
}
async fn cleanup(state: Arc<State>) {
    loop {
        let changed = state.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if state
            .progress
            .lock()
            .map_or(true, |progress| progress.ended)
        {
            return;
        }
        if state.closing.load(Ordering::Acquire) {
            let receipt = state
                .progress
                .lock()
                .ok()
                .and_then(|progress| progress.active.clone());
            let outcome = if let Some(receipt) = receipt {
                state
                    .member
                    .end_use(&receipt)
                    .await
                    .map(|()| UseOutcome::Closed)
            } else if let Ok(Some(body)) = state.attempt.body() {
                state.member.close_use(&body).await
            } else if state
                .progress
                .lock()
                .is_ok_and(|progress| progress.begin_done)
            {
                Ok(UseOutcome::Unsent)
            } else {
                tokio::select! { _ = &mut changed => {}, _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {} }
                continue;
            };
            match outcome {
                Ok(UseOutcome::Active(receipt)) => {
                    if state
                        .attempt
                        .body()
                        .ok()
                        .flatten()
                        .is_some_and(|body| receipt.matches(&body))
                    {
                        if let Ok(mut progress) = state.progress.lock() {
                            progress.active = Some(Arc::new(receipt));
                        }
                        let _ = state.attempt.finish_admission();
                        continue;
                    }
                }
                Ok(UseOutcome::Closed | UseOutcome::Unsent) => {
                    if let Ok(mut progress) = state.progress.lock() {
                        progress.ended = true;
                        progress.active = None;
                    }
                    let _ = state.attempt.finish_admission();
                    signal_changed(&state);
                    return;
                }
                Err(_) => {}
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        } else {
            changed.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_source::Borrow;
    use crate::native_source::Loaded;
    use crate::native_source::PreloadFuture;
    use crate::native_source::ReadyFuture;
    use crate::native_source::UnitFuture;
    use std::sync::atomic::AtomicU64;
    struct Peer {
        serial: Arc<Semaphore>,
        next: AtomicU64,
        started: Notify,
        release: Notify,
        closes: AtomicU64,
        closed: Mutex<Vec<String>>,
    }
    impl Member for Peer {
        fn id(&self) -> &str {
            "11111111111111111111111111111111"
        }
        fn live(&self) -> bool {
            true
        }
        fn preload<'a>(&'a self, _: &'a str, _: Option<&'a Borrow>) -> PreloadFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
        fn ready<'a>(&'a self, _: &'a str, _: Option<&'a str>, _: &'a Loaded) -> ReadyFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
        fn retire<'a>(&'a self, _: Option<&'a str>) -> UnitFuture<'a> {
            Box::pin(async { Ok(()) })
        }
        fn begin_use<'a>(
            &'a self,
            attempt: &'a UseAttempt,
            registration: &'a str,
        ) -> UseFuture<'a> {
            Box::pin(async move {
                if !attempt.hold_admission(self.serial.clone()).await? {
                    return Ok(UseOutcome::Unsent);
                }
                let body = match attempt.body()? {
                    Some(body) => body,
                    None => {
                        let next = self.next.fetch_add(1, Ordering::AcqRel) + 1;
                        attempt.assign(UseBody {
                            member: self.id().into(),
                            operation: format!("0000000000000001{next:016x}"),
                            registration: registration.into(),
                        })?
                    }
                };
                self.started.notify_one();
                self.release.notified().await;
                if self.closed.lock().unwrap().contains(&body.operation) {
                    return Ok(UseOutcome::Closed);
                }
                Ok(UseOutcome::Active(UseReceipt::from_native(
                    body.member,
                    body.operation.clone(),
                    body.operation.repeat(2),
                )?))
            })
        }
        fn close_use<'a>(&'a self, body: &'a UseBody) -> UseFuture<'a> {
            Box::pin(async move {
                self.closes.fetch_add(1, Ordering::AcqRel);
                self.closed.lock().unwrap().push(body.operation.clone());
                Ok(UseOutcome::Closed)
            })
        }
        fn end_use<'a>(&'a self, _: &'a UseReceipt) -> UnitFuture<'a> {
            Box::pin(async { Ok(()) })
        }
    }
    fn peer() -> Arc<Peer> {
        Arc::new(Peer {
            serial: Arc::new(Semaphore::new(1)),
            next: AtomicU64::new(0),
            started: Notify::new(),
            release: Notify::new(),
            closes: AtomicU64::new(0),
            closed: Mutex::new(Vec::new()),
        })
    }
    async fn ended(state: &Arc<State>) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let changed = state.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if state.progress.lock().unwrap().ended {
                    return;
                }
                changed.await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn cancelled_queued_begin_has_no_nonce_or_close_and_cannot_suppress_earlier_begin() {
        let peer = peer();
        let first = ParentUseLease::start(peer.clone(), "2".repeat(32)).unwrap();
        peer.started.notified().await;
        let second = ParentUseLease::start(peer.clone(), "2".repeat(32)).unwrap();
        let second_state = second.0.0.clone();
        tokio::task::yield_now().await;
        drop(second);
        ended(&second_state).await;
        assert!(second_state.attempt.body().unwrap().is_none());
        assert_eq!(peer.next.load(Ordering::Acquire), 1);
        assert_eq!(peer.closes.load(Ordering::Acquire), 0);
        let first_state = first.0.0.clone();
        let cached_parent = first.downgrade();
        drop(first);
        assert!(
            cached_parent.upgrade().is_none(),
            "weak cached factory cannot resurrect retiring work"
        );
        ended(&first_state).await;
        assert_eq!(peer.closes.load(Ordering::Acquire), 1);
        assert_eq!(
            peer.closed.lock().unwrap().as_slice(),
            &["00000000000000010000000000000001".to_owned()]
        );
        peer.release.notify_one();
    }
    #[test]
    fn use_receipt_is_strict_and_debug_never_exposes_capability() {
        assert!(UseReceipt::from_native("1".repeat(32), "0".repeat(32), "c".repeat(64)).is_err());
        let receipt = UseReceipt::from_native(
            "1".repeat(32),
            "00000000000000010000000000000001".into(),
            "c".repeat(64),
        )
        .unwrap();
        assert!(!format!("{receipt:?}").contains(&"c".repeat(64)));
    }
}
