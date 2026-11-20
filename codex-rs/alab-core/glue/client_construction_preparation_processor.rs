// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Ordinary preparation carries its offer on the typed config response. A saved
//! resume lookup returns only the existing operation's observation metadata.
use super::*;
use crate::outgoing_message::alab_initial_claim::PREPARATION_RESULT_KEY;
use crate::outgoing_message::alab_initial_claim::PreparationRequest;
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
        if let PreparationRequest::ReadSavedResume { original, offer } = claim.0.as_ref() {
            let completed = self
                .thread_processor
                .alab_read_saved_resume(context, original, offer)
                .await?;
            return codex_app_server_protocol::alab_response_metadata::acknowledgment(
                PREPARATION_RESULT_KEY,
                serde_json::to_value(completed)
                    .map_err(|_| invalid_request("saved resume result encode failed"))?,
            )
            .map(Some)
            .map_err(|_| invalid_request("saved resume result metadata failed"));
        }
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
            PREPARATION_RESULT_KEY,
            value,
        )
        .map(Some)
        .map_err(|_| invalid_request("client preparation result metadata failed"))
    }
}
