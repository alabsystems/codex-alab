// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Explicit side close uses the unchanged complete original unsubscribe wire.
use super::*;
impl AppServerSession {
    pub(crate) fn alab_unsubscribe_side_view(
        &mut self,
        thread: ThreadId,
    ) -> impl std::future::Future<Output = Result<()>> + use<> {
        let operation = self
            .retained_frontend()
            .map(|tenure| tenure.target.side_close_operation(thread))
            .transpose();
        let original = self.thread_unsubscribe(thread);
        async move {
            match operation?.flatten() {
                Some(operation) => operation.run(original).await,
                None => original.await,
            }
        }
    }
    /// The original local close/error callback has now been applied. Awaiting
    /// this bookkeeping cannot lose the operation retained by the target.
    pub(crate) async fn alab_observe_side_close(&self, thread: ThreadId) -> Result<()> {
        if let Some(tenure) = self.retained_frontend() {
            tenure
                .target
                .observe_side_close(thread, &self.request_handle())
                .await?;
        }
        Ok(())
    }
}
