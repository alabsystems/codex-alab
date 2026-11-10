// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Cleanup observation retains no storage, Domain or source-use lease.
use crate::managed_auth::unavailable;
use crate::native_source::Member;
use crate::native_source::Retirement;
use crate::native_source_lifetime::MemberIncarnation;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Default)]
struct Progress {
    request: Option<Retirement>,
    receipt: Option<crate::native_source_retirement::Receipt>,
    attempt: Option<Arc<crate::native_preload::Attempt>>,
    failed: bool,
}
struct State {
    member: Arc<dyn Member>,
    target: Option<(Arc<dyn crate::native_target::Destination>, String)>,
    progress: Mutex<Progress>,
    changed: Notify,
}
impl State {
    fn start(&self, request: Retirement, attempt: Option<Arc<crate::native_preload::Attempt>>) -> io::Result<crate::native_source_retirement::Receipt> {
        if request.preload_operation.is_none()
            && let Some((destination, open)) = &self.target {
            return crate::native_source_retirement::start_target_open(self.member.clone(), destination.clone(), open.clone(), request);
        }
        crate::native_source_retirement::start_owned_request(self.member.clone(), request, attempt)
    }
}
pub(crate) struct Owner(Arc<State>);
impl Owner {
    pub(crate) fn new(member: Arc<dyn Member>) -> Self {
        Self(Arc::new(State {
            member,
            target: None,
            progress: Mutex::default(),
            changed: Notify::new(),
        }))
    }
    pub(crate) fn with_target(member: Arc<dyn Member>, destination: Arc<dyn crate::native_target::Destination>, open_operation: String) -> Self {
        Self(Arc::new(State { member, target: Some((destination, open_operation)), progress: Mutex::default(), changed: Notify::new() }))
    }
    pub(crate) fn observer(&self, member: MemberIncarnation) -> Observer {
        Observer {
            state: Arc::clone(&self.0),
            member,
        }
    }
    pub(crate) fn capture(
        &self,
        attempt: Option<Arc<crate::native_preload::Attempt>>,
        registration: Option<String>,
    ) -> io::Result<()> {
        let preload = attempt.as_ref().map(|attempt| attempt.body());
        let preload_operation = preload.as_ref().map(|body| body.operation.clone());
        let preload_sequence = preload.as_ref().map(|body| body.sequence.clone());
        let mut progress = self.0.progress.lock().map_err(|_| unavailable())?;
        if progress.failed {
            return Err(unavailable());
        }
        match &progress.request {
            Some(old)
                if old.preload_operation != preload_operation
                    || old.preload_sequence != preload_sequence
                    || old.registration != registration =>
            {
                return Err(unavailable());
            }
            Some(_) => {}
            None => {
                let request = Retirement {
                    operation: format!("{:032x}", rand::random::<u128>()),
                    preload_operation,
                    preload_sequence,
                    registration,
                };
                request.validate()?;
                progress.request = Some(request);
                progress.attempt = attempt;
            }
        }
        // Outside a runtime, keep the exact unsent metadata for the observer. It
        // may start the same owned cleanup once a runtime is available.
        if progress.receipt.is_none() && tokio::runtime::Handle::try_current().is_ok() {
            progress.receipt = Some(self.0.start(progress.request.clone().ok_or_else(unavailable)?, progress.attempt.clone())?);
        }
        drop(progress);
        self.0.changed.notify_waiters();
        Ok(())
    }
    pub(crate) fn fail(&self) {
        if let Ok(mut progress) = self.0.progress.lock() {
            progress.failed = true;
        }
        self.0.changed.notify_waiters();
    }
}
#[derive(Clone)]
pub struct Observer {
    state: Arc<State>,
    member: MemberIncarnation,
}
impl Observer {
    pub fn incarnation(&self) -> &MemberIncarnation {
        &self.member
    }
    pub async fn closed(&self) -> io::Result<Closed> {
        loop {
            let changed = self.state.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let receipt = {
                let mut progress = self.state.progress.lock().map_err(|_| unavailable())?;
                if progress.failed {
                    return Err(unavailable());
                }
                if progress.receipt.is_none()
                    && let Some(request) = progress.request.clone()
                {
                    progress.receipt = Some(self.state.start(request, progress.attempt.clone())?);
                }
                progress.receipt.clone()
            };
            if let Some(receipt) = receipt {
                receipt.closed().await;
                return Ok(Closed {
                    member: self.member.clone(),
                });
            }
            changed.await;
        }
    }
}
impl std::fmt::Debug for Observer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeReaderRetirement(<private>)")
    }
}
/// Only an exact native Closed acknowledgment can construct this terminal proof.
pub struct Closed {
    member: MemberIncarnation,
}
impl Closed {
    pub(crate) fn matches(&self, member: &MemberIncarnation) -> bool {
        &self.member == member
    }
}
