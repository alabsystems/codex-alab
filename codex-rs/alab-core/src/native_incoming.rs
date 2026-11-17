// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Owned native stream input for one persistent app-server runtime. This is IO
//! custody, never a serialized Source, SDK, foreground or continuation grant.
#![cfg(unix)]
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use tokio::net::UnixStream;
use tokio::sync::watch;

/// One actual socket and its optional process-local resource. Prep-only
/// consumers may intentionally drop the resource, never reconstruct it later.
pub struct AcceptedConnection {
    stream: UnixStream,
    resource: Option<crate::connection_resource::ConnectionResource>,
}
impl AcceptedConnection {
    pub fn stream_only(stream: UnixStream) -> Self {
        Self {
            stream,
            resource: None,
        }
    }
    pub fn with_resource(
        stream: UnixStream,
        resource: crate::connection_resource::ConnectionResource,
    ) -> Self {
        Self {
            stream,
            resource: Some(resource),
        }
    }
    pub fn into_stream(self) -> UnixStream {
        self.stream
    }
    pub fn into_parts(
        self,
    ) -> (
        UnixStream,
        Option<crate::connection_resource::ConnectionResource>,
    ) {
        (self.stream, self.resource)
    }
}
pub type Accept<'a> =
    Pin<Box<dyn Future<Output = io::Result<Option<AcceptedConnection>>> + Send + 'a>>;
pub type Withdrawal<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// The provider retains actual owner/queue objects. Native admission must occur
/// before a stream enters this interface; its implementation cannot mint SDK
/// permission from a pathname, PID, nonce, readiness flag or configuration data.
/// Dropping an accept future must leave any not-yet-returned endpoint in the
/// provider's actual custody, so shutdown/cancellation cannot discard delivery.
pub trait IncomingStreamProvider: Send + Sync + 'static {
    fn accept(&self) -> Accept<'_>;
    fn withdrawn(&self) -> Withdrawal<'_>;
}

struct Inner {
    provider: Box<dyn IncomingStreamProvider>,
    ended: watch::Sender<bool>,
    failure: watch::Sender<Option<Arc<io::Error>>>,
}

/// Clones retain this exact provider allocation, not another listener or queue.
#[derive(Clone)]
pub struct Incoming(Arc<Inner>);
impl Incoming {
    pub fn new(provider: impl IncomingStreamProvider) -> Self {
        let (ended, _) = watch::channel(false);
        let (failure, _) = watch::channel(None);
        Self(Arc::new(Inner {
            provider: Box::new(provider),
            ended,
            failure,
        }))
    }

    /// Preparation protocols intentionally consume only stream data. Final RPC
    /// dispatch MUST consume accept_envelope to retain the actual resource.
    pub async fn accept(&self) -> io::Result<Option<UnixStream>> {
        self.accept_envelope()
            .await
            .map(|accepted| accepted.map(AcceptedConnection::into_stream))
    }

    pub async fn accept_envelope(&self) -> io::Result<Option<AcceptedConnection>> {
        if *self.0.ended.borrow() {
            return Ok(None);
        }
        match self.0.provider.accept().await {
            Ok(Some(stream)) => Ok(Some(stream)),
            Ok(None) => {
                self.stop();
                Ok(None)
            }
            Err(error) => {
                let error = Arc::new(error);
                self.record_failure(error.clone());
                Err(io::Error::new(error.kind(), error))
            }
        }
    }

    /// Observe this exact input's sticky local withdrawal state. This is IO
    /// lifetime data only, never native permission or effect settlement.
    pub fn is_stopped(&self) -> bool {
        *self.0.ended.borrow()
    }

    /// Stop this runtime's input following a local transport failure. This is
    /// withdrawal/cleanup only; it says nothing about native effect settlement.
    pub fn stop(&self) {
        self.0.ended.send_replace(true);
    }

    /// Preserve a local input failure before withdrawing admission. The first
    /// actual error remains available through cleanup and every owner clone.
    pub fn fail(&self, error: io::Error) {
        self.record_failure(Arc::new(error));
    }

    /// A retained local failure, not a native settlement or permission verdict.
    /// Absence of an error is not evidence that native effects have completed.
    pub fn failure(&self) -> Option<Arc<io::Error>> {
        self.0.failure.borrow().clone()
    }

    fn record_failure(&self, error: Arc<io::Error>) {
        self.0.failure.send_if_modified(|failure| {
            if failure.is_some() {
                false
            } else {
                *failure = Some(error);
                true
            }
        });
        self.stop();
    }

    pub async fn withdrawn(&self) {
        let mut ended = self.0.ended.subscribe();
        tokio::select! {
            () = self.0.provider.withdrawn() => {}
            () = async {
                while !*ended.borrow_and_update() {
                    if ended.changed().await.is_err() { return; }
                }
            } => {}
        }
        self.stop();
    }
}
impl std::fmt::Debug for Incoming {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeIncoming(<owned>)")
    }
}
impl PartialEq for Incoming {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Incoming {}

#[cfg(test)]
#[path = "native_incoming_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "native_incoming_envelope_tests.rs"]
mod envelope_tests;
