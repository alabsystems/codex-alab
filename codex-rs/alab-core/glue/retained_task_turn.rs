// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Initial-turn target over the existing complete turn/start policy.
//! The accepted host create-operation owns this whole future before awaiting;
//! callers cancel only their observation of that owner. No client ID, UI ACK,
//! Native SDK grant or separate detached turn coordinator is introduced here.

use super::super::OperationContext;
use super::*;
#[cfg(unix)]
use codex_alab_core::native_incoming::Incoming;
use codex_core::ThreadRegistration;
use tracing::Span;

pub(super) struct BackendTarget<'a> {
    #[cfg(unix)]
    incoming: &'a Incoming,
    parent_id: ThreadId,
    parent: &'a Arc<CodexThread>,
    child: &'a Arc<CodexThread>,
    context: &'a OperationContext,
    span: Span,
    purpose: BackendTurnPurpose,
}

#[derive(Clone, Copy)]
enum BackendTurnPurpose {
    InitialChild,
    Followup,
}

pub(super) enum TurnTarget<'a> {
    Client(ConnectionRequestId),
    Backend(BackendTarget<'a>),
}

impl TurnTarget<'_> {
    async fn check_backend(
        &self,
        processor: &TurnRequestProcessor,
        child_id: ThreadId,
    ) -> Result<(), JSONRPCErrorError> {
        if let Self::Backend(target) = self {
            #[cfg(unix)]
            let input_stopped = target.incoming.is_stopped();
            #[cfg(not(unix))]
            let input_stopped = true;
            if input_stopped || target.context.cancellation.is_cancelled() {
                return Err(internal_error(match target.purpose {
                    BackendTurnPurpose::InitialChild => "backend initial-turn owner withdrew",
                    BackendTurnPurpose::Followup => "backend follow-up turn owner withdrew",
                }));
            }
            if (matches!(target.purpose, BackendTurnPurpose::InitialChild)
                && Arc::ptr_eq(target.parent, target.child))
                || target.parent.has_shutdown_status()
                || target.child.has_shutdown_status()
                || !matches!(
                    processor
                        .thread_manager
                        .thread_registration(target.parent_id, target.parent,)
                        .await,
                    ThreadRegistration::Current,
                )
                || !matches!(
                    processor
                        .thread_manager
                        .thread_registration(child_id, target.child)
                        .await,
                    ThreadRegistration::Current,
                )
            {
                return Err(invalid_request(match target.purpose {
                    BackendTurnPurpose::InitialChild => {
                        "backend initial-turn parent or child is no longer current"
                    }
                    BackendTurnPurpose::Followup => {
                        "backend follow-up issuer or target is no longer current"
                    }
                }));
            }
        }
        Ok(())
    }

    pub(super) async fn load_thread(
        &self,
        processor: &TurnRequestProcessor,
        requested: &str,
    ) -> Result<(ThreadId, Arc<CodexThread>), JSONRPCErrorError> {
        match self {
            Self::Client(_) => processor.load_thread(requested).await,
            Self::Backend(target) => {
                let id = ThreadId::from_string(requested)
                    .map_err(|error| invalid_request(format!("invalid thread id: {error}")))?;
                self.check_backend(processor, id).await?;
                // The string only selects this captured Arc's registration.
                // It cannot substitute another runtime through an ID lookup.
                Ok((id, target.child.clone()))
            }
        }
    }

    pub(super) fn track_error(
        &self,
        processor: &TurnRequestProcessor,
        error: &JSONRPCErrorError,
        error_type: Option<AnalyticsJsonRpcError>,
    ) {
        match self {
            Self::Client(request) => processor.track_error_response(request, error, error_type),
            Self::Backend(target) => {
                // No client/request analytics identity exists for this operation.
                // Record only the public code on its actual child operation span.
                target.span.record("error.code", error.code);
            }
        }
    }

    pub(super) async fn ensure_direct_input_allowed(
        &self,
        processor: &TurnRequestProcessor,
        thread: &CodexThread,
    ) -> Result<(), JSONRPCErrorError> {
        ensure_direct_input_allowed(thread)
            .await
            .inspect_err(|error| {
                self.track_error(processor, error, /*error_type*/ None);
            })
    }

    pub(super) async fn trace(&self, processor: &TurnRequestProcessor) -> Option<W3cTraceContext> {
        match self {
            Self::Client(request) => processor.request_trace_context(request).await,
            Self::Backend(target) => codex_otel::span_w3c_trace_context(&target.span)
                .or_else(|| codex_otel::span_w3c_trace_context(&target.context.span))
                .or_else(|| target.context.parent_trace.clone()),
        }
    }

    pub(super) async fn before_submit(
        &self,
        processor: &TurnRequestProcessor,
        child_id: ThreadId,
    ) -> Result<(), JSONRPCErrorError> {
        self.check_backend(processor, child_id).await
    }

    pub(super) async fn record_turn(&self, processor: &TurnRequestProcessor, turn_id: &str) {
        match self {
            Self::Client(request) => {
                processor
                    .outgoing
                    .record_request_turn_id(request, turn_id)
                    .await
            }
            Self::Backend(target) => {
                // Do not overwrite the issuing parent's turn.id span field.
                target.span.record("turn.id", turn_id);
            }
        }
    }
}

#[cfg(unix)]
#[derive(Clone)]
pub(crate) struct BackendInitialTurn {
    processor: TurnRequestProcessor,
    incoming: Incoming,
}

#[cfg(unix)]
impl TurnRequestProcessor {
    pub(crate) fn alab_backend_initial_turn(&self, incoming: Incoming) -> BackendInitialTurn {
        BackendInitialTurn {
            processor: self.clone(),
            incoming,
        }
    }
}

#[cfg(unix)]
impl BackendInitialTurn {
    /// The accepted create-operation worker retains this entire future. Do not
    /// resubmit after losing a response observation: submission may have effects.
    /// This returns the real typed policy result, never a frontend wire reply.
    pub(crate) async fn start(
        &self,
        parent_id: ThreadId,
        parent: Arc<CodexThread>,
        child: Arc<CodexThread>,
        context: &OperationContext,
        params: TurnStartParams,
    ) -> Result<TurnStartResponse, JSONRPCErrorError> {
        self.start_for(
            BackendTurnPurpose::InitialChild,
            parent_id,
            parent,
            child,
            context,
            params,
        )
        .await
    }

    /// Same full policy for an existing target. Unlike initial-child creation,
    /// a SEND may address the actual issuing runtime itself.
    pub(crate) async fn followup(
        &self,
        parent_id: ThreadId,
        parent: Arc<CodexThread>,
        target: Arc<CodexThread>,
        context: &OperationContext,
        params: TurnStartParams,
    ) -> Result<TurnStartResponse, JSONRPCErrorError> {
        self.start_for(
            BackendTurnPurpose::Followup,
            parent_id,
            parent,
            target,
            context,
            params,
        )
        .await
    }

    async fn start_for(
        &self,
        purpose: BackendTurnPurpose,
        parent_id: ThreadId,
        parent: Arc<CodexThread>,
        child: Arc<CodexThread>,
        context: &OperationContext,
        params: TurnStartParams,
    ) -> Result<TurnStartResponse, JSONRPCErrorError> {
        validate_user_input_image_urls(&params.input)?;
        let span = match purpose {
            BackendTurnPurpose::InitialChild => tracing::info_span!(
                parent: &context.span, "backend_initial_turn", turn.id = tracing::field::Empty,
                error.code = tracing::field::Empty),
            BackendTurnPurpose::Followup => tracing::info_span!(
                parent: &context.span, "backend_followup_turn", turn.id = tracing::field::Empty,
                error.code = tracing::field::Empty),
        };
        let target = TurnTarget::Backend(BackendTarget {
            incoming: &self.incoming,
            parent_id,
            parent: &parent,
            child: &child,
            context,
            span: span.clone(),
            purpose,
        });
        // None supplies no new client-info update. All model/provider, input,
        // environment, trust, MCP, sandbox and submission policy stays shared.
        self.processor
            .turn_start_inner(
                target, params, /*app_server_client_name*/ None,
                /*app_server_client_version*/ None,
            )
            .instrument(span)
            .await
    }
}

#[cfg(all(test, unix))]
#[path = "retained_task_turn_tests.rs"]
mod tests;
