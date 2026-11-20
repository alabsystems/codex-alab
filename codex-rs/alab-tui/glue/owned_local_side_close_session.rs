// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Explicit side close uses the unchanged complete original unsubscribe wire.
use super::*;
impl AppServerSession {
    pub(crate) fn alab_unsubscribe_side_view(
        &mut self,
        thread: ThreadId,
    ) -> (
        bool,
        impl std::future::Future<Output = (Result<()>, Option<SideCloseAck>)> + use<>,
    ) {
        let operation = self
            .retained_frontend()
            .map(|tenure| tenure.target.side_close_operation(thread))
            .transpose()
            .map(Option::flatten);
        let interrupt = match &operation {
            Ok(Some(reservation)) => reservation.is_first(),
            Ok(None) => true,
            Err(_) => false,
        };
        let handle = self.request_handle();
        let request = self.next_request_id();
        let original = self.thread_unsubscribe_response(thread, request);
        (interrupt, async move {
            let reservation = match operation {
                Err(error) => return (Err(error.into()), None),
                Ok(None) => {
                    return (
                        original
                            .await
                            .map(|_| ())
                            .wrap_err("thread/unsubscribe failed in TUI"),
                        None,
                    );
                }
                Ok(Some(reserved)) => reserved,
            };
            let result = if reservation.is_first() {
                reservation.operation.run(original).await
            } else {
                reservation.operation.read_result(&handle).await
            };
            let ack = SideCloseAck::for_reply(
                reservation.target.clone(),
                reservation.thread,
                &reservation.operation,
                handle,
                &result,
            );
            (
                result
                    .map(|_| ())
                    .wrap_err("thread/unsubscribe failed in TUI"),
                ack,
            )
        })
    }
}

impl SideCloseAck {
    pub(crate) fn for_reply(
        target: crate::owned_local_frontend::OwnedLocalTarget,
        thread: ThreadId,
        operation: &codex_app_server_client::SideViewCloseOperation,
        handle: codex_app_server_client::AppServerRequestHandle,
        result: &std::result::Result<ThreadUnsubscribeResponse, TypedRequestError>,
    ) -> Option<Self> {
        matches!(result, Ok(_) | Err(TypedRequestError::Server { .. })).then(|| Self {
            target,
            thread,
            operation: operation.clone(),
            handle,
        })
    }
}
