// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Scoped metadata on the exact original startup future. No synthetic client,
//! persistent flag on later chats, Native currentness or permission is created.
use super::*;
use std::sync::Mutex;
use std::sync::PoisonError;

#[path = "initial_session_metadata.rs"]
mod metadata;
pub use client_construction::scope_original_side_injection;
pub use metadata::ClientConstructionResult;
pub use metadata::ClientConstructionSelection;
pub use metadata::ClientPresentation;
#[path = "saved_resume_reply_client.rs"]
mod saved_resume_reply;
#[path = "retained_side_cleanup_reply.rs"]
mod side_cleanup_reply;
#[path = "retained_side_rollback_client.rs"]
mod side_rollback;
#[path = "retained_side_view_client.rs"]
mod side_view;
pub use metadata::InitialSessionResult;
pub use metadata::InitialSessionSelection;
pub use metadata::NextSelection;
pub use side_rollback::SideRollbackCheckpoint;
pub use side_rollback::SideRollbackOperation;
pub use side_view::SideView;
pub use side_view::SideViewCloseCheckpoint;
pub use side_view::SideViewCloseOperation;
/// Actual original UI request choice DATA, not an actual Core identity.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "method",
    content = "threadId",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum InitialAction {
    Start,
    Fork(String),
    Resume(String),
}
struct InitialScope {
    selection: InitialSessionSelection,
    action: Option<InitialAction>,
    result: Mutex<Option<InitialSessionResult>>,
    next_selection: Mutex<Option<NextSelection>>,
    request: Mutex<Option<(String, Option<serde_json::Value>)>>,
}
tokio::task_local! { static INITIAL_SCOPE: std::sync::Arc<InitialScope>; }
#[path = "initial_construction_owner.rs"]
mod initial_owner;
pub use initial_owner::InitialConstructionCheckpoint;
pub use initial_owner::InitialConstructionOperation;

pub async fn scope_initial_session<T>(
    selection: Option<InitialSessionSelection>,
    work: impl std::future::Future<Output = T>,
) -> T {
    observe_initial_session(selection, work).await.0
}
/// The original UI caller records an observed source offer only after the real
/// original choice finishes. This never retries or chooses a different action.
pub async fn observe_initial_session<T>(
    selection: Option<InitialSessionSelection>,
    work: impl std::future::Future<Output = T>,
) -> (T, Option<NextSelection>) {
    match selection {
        Some(selection) => {
            let operation = InitialConstructionOperation::fresh(selection, None);
            let output = operation.run(work).await;
            (output, operation.next_selection())
        }
        None => (work.await, None),
    }
}
/// This lexical client scope selects backend-owned compatibility handling. It
/// is not readiness, Native origin, successful acceptance or permission.
pub fn owns_construction_fallback() -> bool {
    INITIAL_SCOPE.try_with(|_| ()).is_ok() || client_construction::active()
}
/// Only within the actual original startup call, and only for that response's
/// exact typed thread id. This is Source capability/compatibility DATA only.
pub fn initial_session_result(thread_id: &str) -> Option<InitialSessionResult> {
    INITIAL_SCOPE
        .try_with(|scope| {
            scope
                .result
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .filter(|result| result.thread_id == thread_id)
                .cloned()
        })
        .ok()
        .flatten()
}
/// Captured only from this exact accepted request's owned error DATA. A caller
/// may carry it to a later REAL original UI selection; this never retries.
pub fn next_initial_selection() -> Option<NextSelection> {
    INITIAL_SCOPE
        .try_with(|scope| {
            scope
                .next_selection
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        })
        .ok()
        .flatten()
}
fn invalid(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}
pub(super) async fn request(
    handle: &AppServerRequestHandle,
    request: ClientRequest,
) -> IoResult<RequestResult> {
    if side_rollback::active() {
        return side_rollback::request(handle, request).await;
    }
    if request.method_name() == "thread/unsubscribe" && side_view::active() {
        return side_view::request(handle, request)
            .await?
            .ok_or_else(|| invalid("actual Side close scope lost"));
    }
    let selection = INITIAL_SCOPE.try_with(|scope| scope.selection.clone()).ok();
    if !matches!(
        request.method_name(),
        "thread/start" | "thread/fork" | "thread/resume"
    ) || selection.is_none()
    {
        return client_construction::request(handle, request).await;
    }
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "initial session metadata requires actual retained frontend IO",
        ));
    };
    let Some(selection) = selection else {
        return Err(invalid("initial claim scope lost"));
    };
    let saved_resume = saved_resume_reply::matches(&request);
    // Same exhaustive original serialization; no copied request input mapping.
    let value = serde_json::to_value(request).map_err(std::io::Error::other)?;
    let mut wire: codex_app_server_protocol::JSONRPCRequest =
        serde_json::from_value(value).map_err(std::io::Error::other)?;
    if !selection.valid() {
        return Err(invalid("invalid initial session metadata"));
    }
    INITIAL_SCOPE
        .try_with(|scope| {
            if scope
                .action
                .as_ref()
                .is_some_and(|action| !action.matches(&wire.method, wire.params.as_ref()))
            {
                return Err(invalid("initial request changed its original UI action"));
            }
            let mut original = scope.request.lock().unwrap_or_else(PoisonError::into_inner);
            match original.as_ref() {
                Some((method, params)) if method != &wire.method || params != &wire.params => Err(
                    invalid("initial selection cannot retry changed original arguments"),
                ),
                Some(_) => Ok(()),
                None => {
                    *original = Some((wire.method.clone(), wire.params.clone()));
                    Ok(())
                }
            }
        })
        .map_err(|_| invalid("initial request scope lost"))??;
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("startup parameters must be an object"))?;
    let meta = params
        .entry(metadata::META)
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| invalid("startup metadata must be an object"))?;
    if meta.contains_key(metadata::KEY) {
        return Err(invalid("startup metadata namespace is occupied"));
    }
    meta.insert(
        metadata::KEY.into(),
        serde_json::to_value(&selection).map_err(std::io::Error::other)?,
    );
    let response = remote.request_json_rpc(wire).await;
    saved_resume_reply::record_received(saved_resume, &response);
    let mut response = match response? {
        Ok(response) => response,
        Err(mut error) => {
            if let Some(next) = selection_offer::take_offer(&mut error, &selection.claim)? {
                INITIAL_SCOPE
                    .try_with(|scope| {
                        *scope
                            .next_selection
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner) = Some(next)
                    })
                    .map_err(|_| invalid("initial error scope lost"))?;
            }
            return Ok(Err(error));
        }
    };
    let body = response
        .as_object_mut()
        .ok_or_else(|| invalid("initial response must be an object"))?;
    let meta = body
        .get_mut(metadata::META)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("initial response observation missing"))?;
    let result: InitialSessionResult = serde_json::from_value(
        meta.remove(metadata::RESULT_KEY)
            .ok_or_else(|| invalid("initial response observation missing"))?,
    )
    .map_err(|_| invalid("invalid initial response observation"))?;
    if result.claim != selection.claim
        || !result.source_selection.valid()
        || result.source_selection.preparation != selection.claim
        || result.thread_id.is_empty()
        || body
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(serde_json::Value::as_str)
            != Some(result.thread_id.as_str())
    {
        return Err(invalid("initial response observation does not match"));
    }
    // Preserve any response metadata owned by other extensions. Only this
    // direction's closed Source observation is removed before typed decoding.
    let empty = body
        .get(metadata::META)
        .and_then(serde_json::Value::as_object)
        .is_some_and(serde_json::Map::is_empty);
    if empty {
        body.remove(metadata::META);
    }
    INITIAL_SCOPE
        .try_with(|scope| {
            let mut stored = scope.result.lock().unwrap_or_else(PoisonError::into_inner);
            if stored.as_ref().is_some_and(|old| old != &result) {
                return Err(invalid("initial completion changed original operation"));
            }
            *stored = Some(result);
            Ok(())
        })
        .map_err(|_| invalid("initial response scope lost"))??;
    Ok(Ok(response))
}

#[path = "initial_selection_offer.rs"]
mod selection_offer;

#[cfg(test)]
#[path = "initial_selection_codec_tests.rs"]
mod selection_codec_tests;

#[path = "client_construction_client.rs"]
mod client_construction;
pub use client_construction::ClientConstructionCheckpoint;
pub use client_construction::ClientConstructionOperation;
pub use client_construction::ClientPreparationRetirement;
pub use client_construction::SavedResumeObservation;
pub use client_construction::SavedResumeQuery;
pub use client_construction::observe_client_construction;

pub use metadata::preparation::Offer as ClientConstructionOffer;
