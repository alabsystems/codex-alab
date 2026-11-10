// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Owned ordering for member PRELOAD, separate from semantic operations and RPC replay.
//! These values order source observations; none is source or grant authority.

use crate::managed_auth::{Revision, unavailable, valid_hex};
use std::io;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, PartialEq, Eq)]
pub struct Sequence(String);
impl Sequence {
    pub fn parse(value: &str) -> io::Result<Self> {
        if !valid_hex(value, 32)
            || u64::from_str_radix(&value[16..], 16).unwrap_or(0) == 0
        {
            return Err(unavailable());
        }
        Ok(Self(value.into()))
    }
    pub fn encoded(&self) -> &str { &self.0 }
}
impl std::fmt::Debug for Sequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePreloadSequence(<private>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Body {
    pub operation: String,
    pub sequence: Sequence,
}
impl Body {
    pub fn validate(&self) -> io::Result<()> {
        if !valid_hex(&self.operation, 32) { return Err(unavailable()); }
        Sequence::parse(self.sequence.encoded()).map(|_| ())
    }
}
impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePreloadBody(<private>)")
    }
}

/// One immutable resampling predecessor. Replaying it cannot silently select a
/// later snapshot; another step requires the native returned registration/revision.
#[derive(Clone, PartialEq, Eq)]
pub struct Predecessor {
    pub registration: String,
    pub revision: Revision,
}
impl Predecessor {
    pub fn validate(&self) -> io::Result<()> {
        if !valid_hex(&self.registration, 32) || self.revision.generation == 0
            || !valid_hex(&self.revision.digest, 64)
        { return Err(unavailable()); }
        Ok(())
    }
}
impl std::fmt::Debug for Predecessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePreloadPredecessor(<private>)")
    }
}

/// Resampling never turns a committed-Ready observation into read authority.
#[derive(Clone)]
pub enum Resample {
    Preload(crate::native_source::Preload),
    ReadyCommitted(ReadyCommitted),
}
impl std::fmt::Debug for Resample {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("NativeResample(<private>)") }
}
#[derive(Clone)]
pub struct ReadyCommitted { body: Body, previous: Predecessor }
impl ReadyCommitted {
    pub fn from_native(body: Body, previous: Predecessor) -> io::Result<Self> {
        body.validate()?; previous.validate()?;
        Ok(Self { body, previous })
    }
    pub fn validate(&self, body: &Body, previous: &Predecessor) -> io::Result<()> {
        if &self.body != body || &self.previous != previous { return Err(unavailable()); }
        Ok(())
    }
}
impl std::fmt::Debug for ReadyCommitted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("NativeReadyCommitted(<private>)") }
}

/// Exact authenticated completion of one owned PRELOAD registration. The
/// private adapter must bind the native caller, instance and immutable RPC body.
#[derive(Clone)]
pub struct Ready {
    body: Body,
    registration: String,
}
impl Ready {
    pub fn from_native(body: Body, registration: String) -> io::Result<Self> {
        body.validate()?;
        if !valid_hex(&registration, 32) { return Err(unavailable()); }
        Ok(Self { body, registration })
    }
    pub fn validate(&self, body: &Body, registration: &str) -> io::Result<()> {
        if &self.body != body || self.registration != registration { return Err(unavailable()); }
        Ok(())
    }
    pub fn registration(&self) -> &str { &self.registration }
    pub fn body(&self) -> &Body { &self.body }
}
impl std::fmt::Debug for Ready {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("NativeOwnedReady(<private>)") }
}

/// Metadata-only native retirement completion. It proves neither NoReader nor
/// permission to select another source.
pub struct Closed { body: Body }
impl Closed {
    pub fn from_native(body: Body) -> io::Result<Self> { body.validate()?; Ok(Self { body }) }
    pub fn body(&self) -> &Body { &self.body }
}
impl std::fmt::Debug for Closed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("NativeOwnedClosed(<private>)") }
}

struct Counter { prefix: u64, next: u64 }
pub(crate) struct Order {
    admission: Arc<Semaphore>,
    counter: Mutex<Counter>,
}
impl Default for Order {
    fn default() -> Self {
        Self { admission: Arc::new(Semaphore::new(1)), counter: Mutex::new(Counter {
            prefix: rand::random(), next: 0,
        }) }
    }
}
impl Order {
    /// Called by the owned operation actor, never a cancellable observer. Its
    /// returned Attempt is retained in Lifetime state before any first send.
    pub(crate) async fn begin(&self, operation: String) -> io::Result<Arc<Attempt>> {
        if !valid_hex(&operation, 32) { return Err(unavailable()); }
        let admission = self.admission.clone().acquire_owned().await.map_err(|_| unavailable())?;
        let sequence = {
            let mut counter = self.counter.lock().map_err(|_| unavailable())?;
            counter.next = counter.next.checked_add(1).ok_or_else(unavailable)?;
            Sequence::parse(&format!("{:016x}{:016x}", counter.prefix, counter.next))?
        };
        Ok(Arc::new(Attempt { body: Body { operation, sequence }, admission: Mutex::new(Some(admission)) }))
    }
}

/// Metadata and semantic admission survive observer cancellation and transient I/O.
/// Only an exact native READY or Closed receipt can release the attempt.
pub(crate) struct Attempt {
    body: Body,
    admission: Mutex<Option<OwnedSemaphorePermit>>,
}
impl Attempt {
    pub(crate) fn body(&self) -> &Body { &self.body }
    pub(crate) fn ready(&self, ready: &Ready, registration: &str) -> io::Result<()> {
        ready.validate(&self.body, registration)?;
        self.admission.lock().map_err(|_| unavailable())?.take();
        Ok(())
    }
    pub(crate) fn closed(&self, closed: &Closed) -> io::Result<()> {
        if closed.body() != &self.body { return Err(unavailable()); }
        self.admission.lock().map_err(|_| unavailable())?.take();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sequence_is_positive_and_completion_is_exact() {
        assert!(Sequence::parse(&"0".repeat(32)).is_err());
        assert!(Sequence::parse("11111111111111110000000000000001").is_ok());
        let body = Body { operation: "a".repeat(32), sequence: Sequence::parse("11111111111111110000000000000001").unwrap() };
        let ready = Ready::from_native(body.clone(), "b".repeat(32)).unwrap();
        assert!(ready.validate(&body, &"b".repeat(32)).is_ok());
        let mut foreign = body.clone();
        foreign.sequence = Sequence::parse("11111111111111110000000000000002").unwrap();
        assert!(ready.validate(&foreign, &"b".repeat(32)).is_err());
        assert!(ready.validate(&body, &"c".repeat(32)).is_err());
    }

    #[tokio::test]
    async fn unresolved_attempt_owns_admission_until_exact_native_completion() {
        let order = Arc::new(Order::default());
        let first = order.begin("a".repeat(32)).await.unwrap();
        let observer = first.clone();
        drop(observer);
        let queued = order.clone();
        let waiter = tokio::spawn(async move { queued.begin("b".repeat(32)).await });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!waiter.is_finished(), "observer drop is not native closure");
        assert_eq!(order.counter.lock().unwrap().next, 1, "queued attempts cannot allocate a sequence");
        waiter.abort();
        let _ = waiter.await;
        assert_eq!(order.counter.lock().unwrap().next, 1);
        let wrong = Closed::from_native(Body { operation: "c".repeat(32), sequence: first.body.sequence.clone() }).unwrap();
        assert!(first.closed(&wrong).is_err());
        assert_eq!(order.admission.available_permits(), 0);
        first.closed(&Closed::from_native(first.body.clone()).unwrap()).unwrap();
        let next = order.begin("d".repeat(32)).await.unwrap();
        assert_eq!(&next.body.sequence.encoded()[16..], "0000000000000002");
        next.ready(&Ready::from_native(next.body.clone(), "e".repeat(32)).unwrap(), &"e".repeat(32)).unwrap();
    }
}
