// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Credential-free earliest C2 entry on the actual Native constructor.
//! Argv selects an attempt only. Only real Entered + physical E1 constructs
//! the opaque owner; an unresolved attempt never falls through to Source seed.
use super::Controller;
use super::retained_service::EntryObservation;
use super::retained_service::RetainedEntry;
use codex_alab_aterm::frontend_control::FrontendFrontier;
use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::sync::Arc;

const MARKER: &str = "--alab-retained-frontend";
struct Selection {
    service: String,
    go: String,
    original: Vec<OsString>,
}
fn unavailable() -> io::Error {
    io::Error::other("retained frontend entry unavailable")
}
fn token(value: &OsString) -> io::Result<String> {
    let value = value.to_str().ok_or_else(unavailable)?;
    if value.len() != 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(unavailable());
    }
    Ok(value.into())
}
fn select(arguments: Vec<OsString>) -> io::Result<Option<Selection>> {
    if !arguments.get(1).is_some_and(|word| word == MARKER) {
        return Ok(None);
    }
    let go = token(arguments.get(2).ok_or_else(unavailable)?)?;
    let service = token(arguments.get(3).ok_or_else(unavailable)?)?;
    let mut original = Vec::with_capacity(arguments.len().saturating_sub(3));
    original.push(arguments[0].clone());
    original.extend_from_slice(&arguments[4..]);
    Ok(Some(Selection {
        service,
        go,
        original,
    }))
}

/// Historical genuine Native entry plus SAME independent final-F1 controller.
/// Its input and selector getters are DATA, never UI/model/source permission.
pub struct EnteredRetainedFrontend {
    controller: Controller,
    entry: RetainedEntry,
    service: String,
    original: Vec<OsString>,
    input: Arc<[u8]>,
}
impl std::fmt::Debug for EnteredRetainedFrontend {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("EnteredRetainedFrontend(<actual Native E1; no UI/model grant>)")
    }
}
impl EnteredRetainedFrontend {
    /// Invoke before native_backend_startup::enter / ANY credential constructor.
    /// Only absence of the marker chooses ordinary startup. Presence with any
    /// malformed/refused/uncertain Native entry is an error, with no fallback.
    pub async fn acquire(arguments: Vec<OsString>) -> io::Result<Option<Arc<Self>>> {
        let Some(selected) = select(arguments)? else {
            return Ok(None);
        };
        let controller = super::open().await?; // Independent control, no Source member.
        let operation = format!(
            "{:016x}{:016x}",
            codex_alab_core::native_source::connection_request_prefix(),
            codex_alab_core::native_source::connection_request_prefix()
        );
        let mut entry =
            controller.prepare_frontend_entry(&operation, &selected.service, &selected.go)?;
        match entry.observe().await {
            EntryObservation::Entered => {
                let input = entry.take_input()?;
                Ok(Some(Arc::new(Self {
                    controller,
                    entry,
                    service: selected.service,
                    original: selected.original,
                    input: input.into(),
                })))
            }
            EntryObservation::Unresolved(kind) => Err(io::Error::new(
                kind,
                "retained frontend Native entry unresolved",
            )),
        }
    }
    pub fn original_arguments(&self) -> &[OsString] {
        &self.original
    }
    pub fn service_data(&self) -> &str {
        &self.service
    }
    pub fn controller(&self) -> &Controller {
        &self.controller
    }
    pub fn same_controller(&self, actual: &Controller) -> bool {
        self.entry.is_for_controller(actual)
    }
    /// The WHOLE actual E1 remains owned here, including decode/install errors.
    /// A mandatory retained UI decoder borrows this DATA and the original UI
    /// owner applies it before actual UI ACK. Reading or parsing these bytes
    /// establishes no installation, reader retirement or frontier evidence.
    pub fn input_data(&self) -> &[u8] {
        &self.input
    }
    /// First final F1 retains exact entry-controller identity. Reconnect uses
    /// a distinct API requesting fresh Native validation of new F1/currentness;
    /// historical entry custody cannot itself grant that new frontier.
    pub async fn request_frontier(&self, actual: &Controller) -> io::Result<FrontendFrontier> {
        if self.same_controller(actual) {
            self.entry.begin_frontier(actual).await
        } else {
            self.entry.rebind_frontier(actual).await
        }
    }
}

tokio::task_local! { static CURRENT: Arc<EnteredRetainedFrontend>; }
pub async fn with_retained_frontend<F: Future>(
    entry: Arc<EnteredRetainedFrontend>,
    work: F,
) -> F::Output {
    CURRENT.scope(entry, work).await
}
pub fn current_retained_frontend() -> Option<Arc<EnteredRetainedFrontend>> {
    CURRENT.try_with(Arc::clone).ok()
}

/// Preserve the original Source constructor on every ordinary route. Only the
/// private task-local installed from actual Native Entered/E1 can omit it. The
/// caller retains the returned original Startup through dispatch.
pub async fn enter_source_when_unretained()
-> io::Result<Option<codex_alab_core::native_backend_startup::Startup>> {
    if current_retained_frontend().is_some() {
        return Ok(None);
    }
    codex_alab_core::native_backend_startup::enter()
        .await
        .map(Some)
}

#[cfg(test)]
#[path = "retained_frontend_entry_tests.rs"]
mod tests;
