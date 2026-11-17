// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Select completion within the original complete fork policy.
use super::*;

pub(super) enum ForkTarget {
    Client(ConnectionRequestId),
    #[cfg(unix)]
    Initial(Arc<super::alab_initial_session::Scope>),
    #[cfg(unix)]
    Backend(backend::ForkScope),
}
impl ForkTarget {
    pub(super) fn client(&self) -> Option<&ConnectionRequestId> {
        match self {
            Self::Client(request) => Some(request),
            #[cfg(unix)]
            Self::Initial(scope) => Some(scope.request()),
            #[cfg(unix)]
            Self::Backend(_) => None,
        }
    }
    pub(super) async fn configure(
        &self,
        processor: &ThreadRequestProcessor,
        config: &mut Config,
    ) -> Result<(), JSONRPCErrorError> {
        match self {
            Self::Client(_) => {
                let _ = (processor, config);
                Ok(())
            }
            #[cfg(unix)]
            Self::Backend(scope) => scope.configure(processor, config).await,
            #[cfg(unix)]
            Self::Initial(scope) => scope.configure_fork(config).await,
        }
    }
    pub(super) async fn trace(
        &self,
        processor: &ThreadRequestProcessor,
    ) -> Option<W3cTraceContext> {
        match self {
            Self::Client(request) => processor.request_trace_context(request).await,
            #[cfg(unix)]
            Self::Backend(scope) => scope.trace(),
            #[cfg(unix)]
            Self::Initial(scope) => scope.context.request_trace(),
        }
    }
}

#[cfg(unix)]
#[path = "retained_task_fork.rs"]
pub(crate) mod backend;

#[cfg(unix)]
impl ForkTarget {
    pub(super) async fn before_core(
        &self,
        processor: &ThreadRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        match self {
            Self::Client(_) => Ok(()),
            Self::Backend(scope) => scope.before_core(processor).await,
            Self::Initial(scope) => scope.before_core(),
        }
    }
    pub(super) fn retain_created(
        &self,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        match self {
            Self::Client(_) => Ok(()),
            Self::Backend(scope) => {
                scope.capture_birth(actual);
                Ok(())
            }
            Self::Initial(scope) => scope.capture(id, actual),
        }
    }
}

/// Preserve the original typed payload; only Initial decorates owned result DATA.
impl ForkTarget {
    pub(super) fn client_response(
        &self,
        response: ThreadForkResponse,
    ) -> Result<ClientResponsePayload, JSONRPCErrorError> {
        match self {
            Self::Client(_) => Ok(response.into()),
            #[cfg(unix)]
            Self::Initial(scope) => scope.fork_wire(&response),
            #[cfg(unix)]
            Self::Backend(_) => Err(internal_error("backend fork has no client response")),
        }
    }
}
