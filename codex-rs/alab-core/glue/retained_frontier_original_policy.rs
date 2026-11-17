// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One lexical constructor for the FIRST original Resume on the SAME client.
//! Original policy, hydration and typed conversion remain in their actual caller.
use super::*;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
enum Route {
    Runtime {
        frontier: String,
        candidate: Option<String>,
    },
    Source(crate::source_selection::metadata::Project),
}
enum Observed {
    Runtime(Receipt),
    Source(crate::source_selection::Receipt),
}
struct OriginalResume {
    handle: crate::remote::RemoteAppServerRequestHandle,
    thread_id: String,
    route: Route,
    attempted: Mutex<bool>,
    receipt: Mutex<Option<Observed>>,
}
tokio::task_local! { static ORIGINAL_RESUME: Arc<OriginalResume>; }
pub fn active() -> bool {
    ORIGINAL_RESUME.try_with(|_| ()).is_ok()
}
/// Source history DATA from the actual parsed response on this exact channel
/// and target. Absence or an unrelated ordinary request never means empty.
pub fn complete_memory_history(handle: &AppServerRequestHandle, thread_id: &str) -> bool {
    let Some(scope) = ORIGINAL_RESUME.try_with(Arc::clone).ok() else {
        return false;
    };
    let AppServerRequestHandle::Remote(actual) = handle else {
        return false;
    };
    if scope.thread_id != thread_id || !scope.handle.alab_same_channel(actual) {
        return false;
    }
    let observed = scope.receipt.lock().unwrap_or_else(PoisonError::into_inner);
    match observed.as_ref() {
        Some(Observed::Runtime(receipt)) => receipt.complete_memory_history(),
        Some(Observed::Source(receipt)) => receipt.complete_memory_history(),
        None => false,
    }
}
fn unavailable(message: &'static str) -> IoError {
    IoError::new(ErrorKind::InvalidData, message)
}
async fn scope<T>(
    handle: &AppServerRequestHandle,
    thread_id: &str,
    route: Route,
    work: impl std::future::Future<Output = T>,
) -> IoResult<(T, Option<Observed>)> {
    let AppServerRequestHandle::Remote(actual) = handle else {
        return Err(unavailable(
            "original projection policy requires actual physical client",
        ));
    };
    if active()
        || crate::retained_configured::original::active()
        || crate::alab_initial_session_client::owns_construction_fallback()
    {
        return Err(unavailable(
            "original projection scope overlaps construction or another projection",
        ));
    }
    let scope = Arc::new(OriginalResume {
        handle: actual.clone(),
        thread_id: thread_id.into(),
        route,
        attempted: Mutex::new(false),
        receipt: Mutex::new(None),
    });
    let output = ORIGINAL_RESUME.scope(scope.clone(), work).await;
    let receipt = scope
        .receipt
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    Ok((output, receipt))
}
pub async fn scope_original_resume<T>(
    handle: &AppServerRequestHandle,
    frontier: &str,
    thread_id: &str,
    candidate: Option<&str>,
    work: impl std::future::Future<Output = T>,
) -> IoResult<(T, Option<Receipt>)> {
    let selection = metadata::Selection::Project(metadata::Project {
        history: metadata::HistoryIntent::CurrentDisplay,
        frontier: frontier.into(),
        thread_id: thread_id.into(),
        candidate: candidate.map(Into::into),
    });
    if !selection.valid() {
        return Err(unavailable("invalid original R projection scope"));
    }
    let (output, receipt) = scope(
        handle,
        thread_id,
        Route::Runtime {
            frontier: frontier.into(),
            candidate: candidate.map(Into::into),
        },
        work,
    )
    .await?;
    let receipt = match receipt {
        Some(Observed::Runtime(value)) => Some(value),
        None => None,
        Some(Observed::Source(_)) => {
            return Err(unavailable("R scope observed another projection kind"));
        }
    };
    Ok((output, receipt))
}
pub(crate) async fn scope_source_resume<T>(
    handle: &AppServerRequestHandle,
    project: crate::source_selection::metadata::Project,
    work: impl std::future::Future<Output = T>,
) -> IoResult<(T, Option<crate::source_selection::Receipt>)> {
    if !crate::source_selection::metadata::Selection::Project(project.clone()).valid() {
        return Err(unavailable("invalid original S2 projection scope"));
    }
    let thread_id = project.thread_id.clone();
    let (output, receipt) = scope(handle, &thread_id, Route::Source(project), work).await?;
    let receipt = match receipt {
        Some(Observed::Source(value)) => Some(value),
        None => None,
        Some(Observed::Runtime(_)) => return Err(unavailable("S2 scope observed an R projection")),
    };
    Ok((output, receipt))
}
fn transport(error: TypedRequestError) -> IoResult<RequestResult> {
    match error {
        TypedRequestError::Server { source, .. } => Ok(Err(source)),
        TypedRequestError::Transport { source, .. } => Err(source),
        TypedRequestError::Deserialize { source, .. } => {
            Err(IoError::new(ErrorKind::InvalidData, source))
        }
    }
}
pub(crate) async fn request(
    handle: &AppServerRequestHandle,
    request: ClientRequest,
) -> IoResult<RequestResult> {
    let Some(scope) = ORIGINAL_RESUME.try_with(Arc::clone).ok() else {
        return crate::alab_initial_session_client::request(handle, request).await;
    };
    if matches!(request.method_name(), "thread/start" | "thread/fork") {
        return Err(unavailable("projection cannot construct another runtime"));
    }
    if request.method_name() != "thread/resume" {
        return crate::alab_initial_session_client::request(handle, request).await;
    }
    {
        let mut attempted = scope
            .attempted
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *attempted {
            return Err(unavailable("original projected resume already attempted"));
        }
        *attempted = true;
    }
    let AppServerRequestHandle::Remote(actual) = handle else {
        return Err(unavailable("original projection client changed"));
    };
    if !scope.handle.alab_same_channel(actual)
        || crate::alab_initial_session_client::owns_construction_fallback()
    {
        return Err(unavailable(
            "original projection channel or construction owner changed",
        ));
    }
    let wire: JSONRPCRequest = serde_json::from_value(
        serde_json::to_value(request)
            .map_err(|error| IoError::new(ErrorKind::InvalidData, error))?,
    )
    .map_err(|error| IoError::new(ErrorKind::InvalidData, error))?;
    let response = match &scope.route {
        Route::Runtime {
            frontier,
            candidate,
        } => {
            send(
                handle,
                wire,
                &metadata::Selection::Project(metadata::Project {
                    history: metadata::HistoryIntent::CurrentDisplay,
                    frontier: frontier.clone(),
                    thread_id: scope.thread_id.clone(),
                    candidate: candidate.clone(),
                }),
            )
            .await
        }
        Route::Source(project) => {
            crate::source_selection::send(
                handle,
                wire,
                &crate::source_selection::metadata::Selection::Project(project.clone()),
            )
            .await
        }
    };
    let mut value = match response {
        Ok(value) => value,
        Err(error) => return transport(error),
    };
    let receipt = match &scope.route {
        Route::Runtime { frontier, .. } => Observed::Runtime(
            take_projection_receipt(&mut value, frontier, &scope.thread_id)
                .map_err(|error| IoError::new(ErrorKind::InvalidData, error))?,
        ),
        Route::Source(project) => Observed::Source(
            crate::source_selection::take_receipt(&mut value, project)
                .map_err(|error| IoError::new(ErrorKind::InvalidData, error))?,
        ),
    };
    *scope.receipt.lock().unwrap_or_else(PoisonError::into_inner) = Some(receipt);
    Ok(Ok(value)) // SAME raw successful result; original typed conversion continues.
}
