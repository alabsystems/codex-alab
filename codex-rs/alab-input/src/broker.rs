// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One input owner shared by streams, synchronous queries and startup replay.
use crate::Driver;
use crate::InternalEvent;
use crate::Parser;
use crate::invalid;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

type Open = dyn Fn() -> io::Result<File> + Send + Sync;
pub struct Broker {
    state: Mutex<State>,
    open: Box<Open>,
    wake: Arc<Waiters>,
}
struct State {
    pristine: bool,
    parser: Parser,
    input: Option<File>,
    driver: Option<Driver>,
    users: usize,
    frozen: bool,
    committed: bool,
    failure: Option<String>,
    next_user: u64,
}
#[derive(Default)]
struct Waiters(Mutex<HashMap<u64, Waker>>);
impl Waiters {
    fn register(&self, id: u64, waker: &Waker) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, waker.clone());
    }
    fn notify(&self) {
        let waiters = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for waker in waiters {
            waker.wake();
        }
    }
}
impl Wake for Waiters {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
}
impl Broker {
    pub fn terminal() -> Arc<Self> {
        Self::new(crate::reader::open_terminal_input)
    }
    pub fn new(open: impl Fn() -> io::Result<File> + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                pristine: true,
                parser: Parser::default(),
                input: None,
                driver: None,
                users: 0,
                frozen: false,
                committed: false,
                failure: None,
                next_user: 0,
            }),
            open: Box::new(open),
            wake: Arc::default(),
        })
    }
    fn start(&self, state: &mut State, raw_mode: bool) -> io::Result<()> {
        state.pristine = false;
        if state.frozen || state.committed {
            return Err(invalid("input is fenced for an upgrade"));
        }
        if let Some(failure) = &state.failure {
            return Err(io::Error::other(failure.clone()));
        }
        if state.driver.is_none() {
            if state.input.is_none() {
                state.input = Some((self.open)()?);
            }
            let input = state
                .input
                .as_ref()
                .ok_or_else(|| invalid("input description is absent"))?
                .try_clone()?;
            state.driver = Some(Driver::resume(input, &mut state.parser, raw_mode)?);
        }
        Ok(())
    }
    fn stop(state: &mut State) -> io::Result<()> {
        if let Some(mut driver) = state.driver.take()
            && let Err(error) = driver.freeze_into(&mut state.parser)
        {
            state.failure = Some(error.to_string());
            return Err(error);
        }
        Ok(())
    }
    pub fn subscribe(self: &Arc<Self>, raw_mode: bool) -> io::Result<Subscription> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = state.next_user;
        let next = id
            .checked_add(1)
            .ok_or_else(|| invalid("input subscription identities exhausted"))?;
        self.start(&mut state, raw_mode)?;
        state.next_user = next;
        state.users += 1;
        Ok(Subscription {
            broker: self.clone(),
            id,
        })
    }
    /// Startup replay never starts a background reader. A caller holding terminal
    /// ownership can append bytes before the first stream is constructed.
    pub fn buffer_input(&self, bytes: &[u8], raw_mode: bool) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.driver.is_some() || state.frozen || state.committed {
            return Err(invalid("startup replay requires stopped input"));
        }
        state.pristine = false;
        state.parser.feed(bytes, raw_mode)
    }
    /// Explicit protected-screen quarantine; this is never an upgrade operation.
    pub fn discard_buffered_input(&self) -> io::Result<crossterm::event::InputDiscardStatus> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.driver.is_some() || state.frozen || state.committed {
            return Err(invalid("input quarantine requires stopped input"));
        }
        state.pristine = false;
        Ok(state.parser.discard_buffered_input())
    }
    /// Restore before any replay, quarantine, preparation or reader ever used
    /// this owner. Validation happens before publishing any parser data. The
    /// callback may establish a matching fresh terminal owner, never a reader.
    pub fn restore_with(
        &self,
        bytes: &[u8],
        validate: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let parser = Parser::restore(bytes)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.pristine
            || state.users != 0
            || state.input.is_some()
            || state.driver.is_some()
            || state.frozen
            || state.committed
        {
            return Err(invalid("input restoration is too late"));
        }
        validate()?;
        state.parser = parser;
        state.pristine = false;
        Ok(())
    }
    /// Join the real producer before source commitment. While this guard lives,
    /// no new subscription or replay can start another reader.
    pub fn prepare(self: &Arc<Self>) -> io::Result<Fence> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.frozen || state.committed {
            return Err(invalid("input already has an upgrade owner"));
        }
        state.pristine = false;
        Self::stop(&mut state)?;
        let snapshot = state.parser.checkpoint()?;
        state.frozen = true;
        Ok(Fence {
            broker: self.clone(),
            snapshot,
            committed: false,
        })
    }
}
pub struct Subscription {
    broker: Arc<Broker>,
    id: u64,
}
impl Subscription {
    pub fn prepare_upgrade(&self) -> io::Result<Fence> {
        self.broker.prepare()
    }
    pub fn poll_available(
        &self,
        cx: &mut Context<'_>,
        wanted: impl Fn(&InternalEvent) -> bool,
    ) -> Poll<Option<io::Result<()>>> {
        let mut state = self
            .broker
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.broker.wake.register(self.id, cx.waker());
        if state.frozen || state.committed {
            return Poll::Pending;
        }
        if state.driver.is_none() {
            let mode = match crossterm::terminal::is_raw_mode_enabled() {
                Ok(mode) => mode,
                Err(error) => return Poll::Ready(Some(Err(error))),
            };
            if let Err(error) = self.broker.start(&mut state, mode) {
                return Poll::Ready(Some(Err(error)));
            }
        }
        match &state.driver {
            Some(driver) => {
                let waker = Waker::from(self.broker.wake.clone());
                driver.poll_available(&mut Context::from_waker(&waker), wanted)
            }
            None => Poll::Ready(Some(Err(invalid("input reader is absent")))),
        }
    }
    /// Filtering keeps unrelated events in their original queue order, so cursor
    /// queries cannot eat the person's keys or a pending paste.
    pub fn poll_where(
        &self,
        cx: &mut Context<'_>,
        wanted: impl Fn(&InternalEvent) -> bool,
    ) -> Poll<Option<io::Result<InternalEvent>>> {
        let mut state = self
            .broker
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.frozen || state.committed {
            self.broker.wake.register(self.id, cx.waker());
            return Poll::Pending;
        }
        if state.driver.is_none() {
            // A refused preparation stopped the old producer. Restore it with
            // the actual current terminal mode before accepting another read.
            let mode = match crossterm::terminal::is_raw_mode_enabled() {
                Ok(mode) => mode,
                Err(error) => return Poll::Ready(Some(Err(error))),
            };
            if let Err(error) = self.broker.start(&mut state, mode) {
                return Poll::Ready(Some(Err(error)));
            }
        }
        self.broker.wake.register(self.id, cx.waker());
        match &state.driver {
            Some(driver) => {
                let waker = Waker::from(self.broker.wake.clone());
                driver.poll_where(&mut Context::from_waker(&waker), wanted)
            }
            None => Poll::Ready(Some(Err(invalid("input reader is absent")))),
        }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        {
            let mut state = self
                .broker
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.broker
                .wake
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.id);
            state.users -= 1;
            if state.users == 0 {
                let _ = Broker::stop(&mut state);
            }
        }
        // A departing filtered query must not strand another pending consumer
        // whose user event already arrived while the query was waiting.
        self.broker.wake.notify();
    }
}
pub struct Fence {
    broker: Arc<Broker>,
    snapshot: Vec<u8>,
    committed: bool,
}
impl Fence {
    /// Read the identity of the actual retained input description, never a path
    /// or a serialized checkpoint. Pairing data alone supplies no read grant.
    pub fn terminal_identity(&self) -> io::Result<(u64, u64, u64)> {
        let state = self
            .broker
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let input = state
            .input
            .as_ref()
            .ok_or_else(|| invalid("input description is absent"))?;
        let metadata = input.metadata()?;
        Ok((metadata.dev(), metadata.ino(), metadata.rdev()))
    }
    pub fn valid(&self) -> bool {
        let state = self
            .broker
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !self.committed && state.frozen && !state.committed && state.driver.is_none()
    }
    pub fn committed(&self) -> bool {
        self.committed
            && self
                .broker
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .committed
    }
    pub fn snapshot(&self) -> &[u8] {
        &self.snapshot
    }
    /// The input gate remains held through the caller's one final Core/goal/SDK
    /// decision. A false result leaves this guard revocable and its data intact.
    pub fn commit_if(&mut self, decide: impl FnOnce() -> bool) -> bool {
        let mut state = self
            .broker
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.committed || !state.frozen || state.committed || !decide() {
            return false;
        }
        state.committed = true;
        self.committed = true;
        true
    }
}
impl Drop for Fence {
    fn drop(&mut self) {
        {
            let mut state = self
                .broker
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !self.committed {
                state.frozen = false;
            }
        }
        self.broker.wake.notify();
    }
}
