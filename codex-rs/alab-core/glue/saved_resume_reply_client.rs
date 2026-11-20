// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Observe the original saved resume's wire success before retained metadata or
//! the ordinary typed response is decoded. Projection and preparation use other sends.
use super::*;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use std::cell::Cell;

#[derive(Clone, Copy, PartialEq)]
enum OriginalReply {
    Unknown,
    Success,
    RetainedTransportFailure,
}
tokio::task_local! { static ORIGINAL: (RequestId, Cell<OriginalReply>); }

pub(super) fn matches(request: &ClientRequest) -> bool {
    matches!(request, ClientRequest::ThreadResume { request_id, .. }
        if ORIGINAL.try_with(|(original, _)| original == request_id).unwrap_or(false))
}

pub(super) fn record_received(original: bool, result: &IoResult<RequestResult>) {
    if original && matches!(result, Ok(Ok(_))) {
        ORIGINAL.with(|(_, received)| received.set(OriginalReply::Success));
    }
}

pub(super) fn record_retained(original: bool, result: &IoResult<RequestResult>) {
    if original && result.is_err() {
        ORIGINAL.with(|(_, received)| received.set(OriginalReply::RetainedTransportFailure));
    } else {
        record_received(original, result);
    }
}

impl AppServerRequestHandle {
    /// Return the usual typed result and whether this very request received an
    /// original success that could not be consumed. A lost reply has no such evidence.
    pub async fn request_saved_resume(
        &self,
        request_id: RequestId,
        params: ThreadResumeParams,
    ) -> (Result<ThreadResumeResponse, TypedRequestError>, bool) {
        let (result, unreadable, _) = self.request_saved_resume_observed(request_id, params).await;
        (result, unreadable)
    }
    /// Also distinguish a transport error returned by the very original,
    /// admitted retained wire. A prior attempt, preflight, or projection cannot supply it.
    pub async fn request_saved_resume_observed(
        &self,
        request_id: RequestId,
        params: ThreadResumeParams,
    ) -> (Result<ThreadResumeResponse, TypedRequestError>, bool, bool) {
        ORIGINAL
            .scope(
                (request_id.clone(), Cell::new(OriginalReply::Unknown)),
                async move {
                    let result = self
                        .request_typed(ClientRequest::ThreadResume { request_id, params })
                        .await;
                    let observed = ORIGINAL.with(|(_, received)| received.get());
                    let unreadable = result.is_err() && observed == OriginalReply::Success;
                    (
                        result,
                        unreadable,
                        observed == OriginalReply::RetainedTransportFailure,
                    )
                },
            )
            .await
    }
}
