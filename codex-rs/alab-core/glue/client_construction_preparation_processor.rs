// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The SAME actual ConfigRequestProcessor policy returns its real typed config.
//! Only its response metadata selects a backend-owned prepared operation.
use super::*;
impl MessageProcessor {
    pub(crate) async fn alab_client_preparation_read(
        &self,
        context: &RequestContext,
        params: codex_app_server_protocol::ConfigReadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let claim = context
            .alab_client_preparation
            .clone()
            .ok_or_else(|| invalid_request("client preparation metadata missing"))?;
        // Protected Source route, before even the original config policy.
        self.thread_processor
            .alab_check_client_preparation_origin(context)
            .await?;
        let response = self.config_processor.read(params).await?;
        let completed = self
            .thread_processor
            .alab_prepare_client_request(context.clone(), claim)
            .await?;
        let value = serde_json::to_value(completed)
            .map_err(|_| invalid_request("client preparation result encode failed"))?;
        codex_app_server_protocol::alab_response_metadata::decorate(
            response.into(),
            crate::outgoing_message::alab_initial_claim::PREPARATION_RESULT_KEY,
            value,
        )
        .map(Some)
        .map_err(|_| invalid_request("client preparation result metadata failed"))
    }
}
