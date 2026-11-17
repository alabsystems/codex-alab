// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Reuse the original complete delete policy and original notification mapper.
use super::*;
impl ThreadRequestProcessor {
    pub(crate) async fn alab_original_side_delete(
        &self,
        params: ThreadDeleteParams,
    ) -> Result<(ThreadDeleteResponse, Vec<String>), JSONRPCErrorError> {
        let mut deleted = Vec::new();
        let response = {
            let _permit = self.acquire_thread_list_state_permit().await?;
            self.thread_delete_response(params, &mut deleted).await?
        };
        Ok((response, deleted))
    }
    pub(crate) async fn alab_original_side_deleted(&self, deleted: Vec<String>) {
        self.send_thread_deleted_notifications(deleted).await;
    }
}
