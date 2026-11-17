// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One real same-backend managed snapshot read. A request scope/target label is
//! never authority: the exact bound owner and received A resource are checked.
use super::*;
use crate::outgoing_message::alab_reader_claim::metadata;
use crate::request_processors::config_processor::ConfigRequestProcessor;
use codex_app_server_protocol::ConfigReadParams;

impl Binding {
    async fn accept_reader(
        self: &Arc<Self>,
        context: RequestContext,
        params: ConfigReadParams,
        selected: metadata::Selection,
        config: ConfigRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        if !selected.valid() {
            return Err(invalid_request("retained reader selectors invalid"));
        }
        let origin =
            context
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| {
                    invalid_request("retained reader requires actual received A origin")
                })?;
        let current = self
            .backend
            .observe_reader_origin(&origin)
            .await
            .map_err(native_error)?;
        if current.service_data() != selected.service || !current.same_receipt(&origin) {
            return Err(invalid_request(
                "retained reader changed actual backend service",
            ));
        }
        let guard = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed
                || self.input.is_stopped()
                || self.input.withdrawn().now_or_never().is_some()
            {
                return Err(invalid_request("retained reader admission closed"));
            }
            ReaderGuard {
                input: self.input.clone(),
                _context: context.clone(),
                _token: self.tracker.token(),
                complete: false,
            }
        };
        let binding = self.clone();
        tokio::spawn(async move {
            let mut guard = guard;
            let result: Result<
                codex_app_server_protocol::ClientResponsePayload,
                JSONRPCErrorError,
            > = async {
                let (response, cloud, account) = config
                    .alab_retained_managed_read(params, selected.kind)
                    .await?;
                binding.open()?;
                let snapshot = metadata::Snapshot {
                    schema: 1,
                    service: current.service_data().into(),
                    operation: selected.operation,
                    kind: selected.kind,
                    cloud,
                    account,
                };
                codex_app_server_protocol::alab_response_metadata::decorate(
                    response.into(),
                    metadata::RESULT_KEY,
                    serde_json::to_value(snapshot)
                        .map_err(|_| internal_error("retained reader snapshot encode failed"))?,
                )
                .map_err(|_| internal_error("retained reader response metadata unavailable"))
            }
            .await;
            match result {
                Ok(payload) => {
                    binding
                        .processor
                        .outgoing
                        .send_response(context.alab_request_id().clone(), payload)
                        .await
                }
                Err(error) => {
                    binding
                        .processor
                        .outgoing
                        .send_error(context.alab_request_id().clone(), error)
                        .await
                }
            }
            guard.complete = true;
        });
        Ok(())
    }
}
struct ReaderGuard {
    input: Incoming,
    _context: RequestContext,
    _token: tokio_util::task::task_tracker::TaskTrackerToken,
    complete: bool,
}
impl Drop for ReaderGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.input.fail(std::io::Error::other(
                "accepted retained reader outcome remains unobserved",
            ));
        }
    }
}
impl ThreadRequestProcessor {
    pub(crate) async fn alab_retained_reader(
        &self,
        context: RequestContext,
        params: ConfigReadParams,
        selected: metadata::Selection,
        config: ConfigRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        self.alab_retained_frontier
            .actual()?
            .accept_reader(context, params, selected, config)
            .await
    }
}
