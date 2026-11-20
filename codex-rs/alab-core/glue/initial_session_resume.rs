// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A genuine client target inside the complete original resume policy. The
//! accepted startup keeps the original request/context and listener frontier;
//! no synthetic client, default history, replayed Core call, or UI ACK is made.
#[cfg(unix)]
use super::alab_initial_session::Scope;
use super::*;

pub(crate) struct ResumeClientTarget {
    request: ConnectionRequestId,
    model_refresh: std::sync::Mutex<std::sync::Weak<CodexThread>>,
    #[cfg(unix)]
    publication: Option<ResumePublication>,
}
impl From<ConnectionRequestId> for ResumeClientTarget {
    fn from(request: ConnectionRequestId) -> Self {
        Self {
            request,
            model_refresh: Default::default(),
            #[cfg(unix)]
            publication: None,
        }
    }
}
impl ResumeClientTarget {
    #[cfg(unix)]
    pub(super) fn retained(scope: Arc<Scope>) -> Self {
        Self {
            request: scope.request().clone(),
            model_refresh: Default::default(),
            publication: Some(ResumePublication { scope }),
        }
    }
    /// Check once per actual loaded Core for this request, including if the thread
    /// was replaced while the previous owner's catalog request was in flight.
    pub(super) async fn refresh_model(
        &self,
        thread: &Arc<CodexThread>,
        params: &ThreadResumeParams,
    ) -> Option<alab_running_resume::ModelRefresh> {
        if !codex_alab_core::active()
            || params.model.is_some()
            || params.model_provider.is_some()
            || params.config.as_ref().is_some_and(|overrides| {
                overrides.contains_key("model") || overrides.contains_key("model_provider")
            })
        {
            return None;
        }
        let refresh = thread.alab_refresh_for_model_resume().await?;
        let actual = Arc::downgrade(thread);
        let mut checked = self
            .model_refresh
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if checked.ptr_eq(&actual) {
            return None;
        }
        *checked = actual;
        Some(Box::pin(refresh))
    }

    pub(crate) fn request(&self) -> &ConnectionRequestId {
        &self.request
    }
    pub(crate) fn publication(&self) -> Option<ResumePublication> {
        #[cfg(unix)]
        {
            self.publication.clone()
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
    pub(super) async fn trace(
        &self,
        processor: &ThreadRequestProcessor,
    ) -> Option<W3cTraceContext> {
        #[cfg(unix)]
        if let Some(publication) = &self.publication {
            return publication.scope.context.request_trace();
        }
        processor.request_trace_context(&self.request).await
    }
}
/// Constructed only after the actual one-shot operation's Native-origin check.
/// It owns the same accepted Scope through the original loaded listener command.
#[derive(Clone)]
pub(crate) struct ResumePublication {
    #[cfg(unix)]
    scope: Arc<Scope>,
}
impl ResumePublication {
    pub(crate) fn accepts_closed_observer(&self) -> bool {
        #[cfg(unix)]
        {
            self.scope.check_owner().is_ok()
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
    #[cfg(unix)]
    pub(crate) async fn ready(
        &self,
        processor: &ThreadRequestProcessor,
        id: ThreadId,
        actual: &Arc<CodexThread>,
        born: bool,
    ) -> Result<(), JSONRPCErrorError> {
        self.scope
            .resume_ready(processor.listener_task_context(), id, actual, born)
            .await
    }
}
#[cfg(unix)]
impl ThreadResumeTarget {
    pub(super) fn alab_resume_publication(&self) -> Option<ResumePublication> {
        match self {
            Self::Client(client) => client.publication(),
            #[cfg(unix)]
            Self::Backend(_) => None,
            Self::DaemonRecovery(_) => None,
        }
    }
    pub(super) fn alab_capture_loaded(&self, id: ThreadId, actual: &Arc<CodexThread>) {
        match self {
            Self::Client(client) => {
                if let Some(publication) = &client.publication {
                    publication.scope.capture_resume_loaded(id, actual);
                }
            }
            #[cfg(unix)]
            Self::Backend(scope) => scope.capture_loaded(actual),
            Self::DaemonRecovery(_) => {}
        }
    }
    pub(super) fn alab_capture_birth(
        &self,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        match self {
            Self::Client(client) => match &client.publication {
                Some(publication) => publication.scope.capture(id, actual),
                None => Ok(()),
            },
            #[cfg(unix)]
            Self::Backend(scope) => {
                scope.capture_birth(actual);
                Ok(())
            }
            Self::DaemonRecovery(_) => Ok(()),
        }
    }
    pub(super) async fn alab_before_resume_core(
        &self,
        processor: &ThreadRequestProcessor,
        config: &mut Config,
    ) -> Result<(), JSONRPCErrorError> {
        match self {
            Self::Client(client) => {
                if let Some(publication) = &client.publication {
                    publication.scope.configure_resume(config).await?;
                    publication.scope.before_core()?;
                }
                Ok(())
            }
            #[cfg(unix)]
            Self::Backend(scope) => scope.before_core(processor, config).await,
            Self::DaemonRecovery(_) => Ok(()),
        }
    }
}
/// Preserve original typed responses and ordering. Only the actual accepted
/// operation stores/enriches its own exact result before original publication.
pub(crate) async fn publish(
    publication: Option<ResumePublication>,
    outgoing: &OutgoingMessageSender,
    request: ConnectionRequestId,
    id: ThreadId,
    actual: &Arc<CodexThread>,
    response: ThreadResumeResponse,
    originator: String,
) {
    #[cfg(unix)]
    if let Some(publication) = publication {
        if publication.scope.request() != &request {
            let error = internal_error("resume publication request/resource mismatch");
            send_error(Some(publication), outgoing, request, error).await;
            return;
        }
        match publication
            .scope
            .stage_resume(id, actual, response, originator.clone())
            .await
        {
            Ok(wire) => {
                outgoing
                    .send_response_with_thread_originator(request, wire, originator)
                    .await
            }
            Err(error) => send_error(Some(publication), outgoing, request, error).await,
        }
        return;
    }
    #[cfg(not(unix))]
    let _ = (publication, id, actual);
    outgoing
        .send_response_with_thread_originator(request, response, originator)
        .await;
}
pub(crate) async fn send_error(
    publication: Option<ResumePublication>,
    outgoing: &OutgoingMessageSender,
    request: ConnectionRequestId,
    error: JSONRPCErrorError,
) {
    #[cfg(unix)]
    if let Some(publication) = publication {
        // The SAME accepted worker owns the sole final error publication after
        // normal completion and offer construction. A raw early error would
        // complete C1 without its offer and duplicate the final response.
        publication.scope.stage_error(error);
        return;
    }
    #[cfg(not(unix))]
    let _ = publication;
    outgoing.send_error(request, error).await;
}

impl ThreadRequestProcessor {
    pub(crate) async fn alab_thread_resume_client(
        &self,
        request: ConnectionRequestId,
        params: ThreadResumeParams,
        name: Option<String>,
        version: Option<String>,
        extensions: ClientMcpExtensions,
        context: RequestContext,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        if context.alab_initial_claim.is_some() {
            #[cfg(unix)]
            {
                if context.alab_request_id() != &request {
                    return Err(invalid_request("initial resume context mismatch"));
                }
                return self
                    .alab_initial_session
                    .accept(
                        context,
                        super::alab_initial_session::Arguments::Resume(params),
                        super::alab_initial_session::ClientMetadata {
                            name,
                            version,
                            extensions,
                        },
                    )
                    .await
                    .map(|()| None);
            }
            #[cfg(not(unix))]
            {
                return Err(invalid_request("retained initial sessions are unavailable"));
            }
        }
        if context.alab_client_construction.is_some() {
            #[cfg(unix)]
            {
                if context.alab_request_id() != &request {
                    return Err(invalid_request("client resume context mismatch"));
                }
                return self
                    .alab_initial_session
                    .accept_client(
                        context,
                        super::alab_initial_session::Arguments::Resume(params),
                        super::alab_initial_session::ClientMetadata {
                            name,
                            version,
                            extensions,
                        },
                    )
                    .await
                    .map(|()| None);
            }
            #[cfg(not(unix))]
            {
                return Err(invalid_request(
                    "retained client construction is unavailable",
                ));
            }
        }
        self.thread_resume(
            ThreadResumeTarget::Client(request.into()),
            params,
            name,
            version,
            extensions,
        )
        .await
    }
}

impl ThreadResumeTarget {
    pub(super) async fn alab_parent_trace(
        &self,
        processor: &ThreadRequestProcessor,
    ) -> Option<W3cTraceContext> {
        match self {
            Self::Client(client) => client.trace(processor).await,
            #[cfg(unix)]
            Self::Backend(scope) => scope.trace(),
            Self::DaemonRecovery(_) => None,
        }
    }
}
