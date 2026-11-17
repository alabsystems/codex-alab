// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Open a distinct native controller without creating a credential reader.
#[cfg(unix)]
pub use codex_alab_aterm::frontend_control::AppFrontendOwner;
pub use codex_alab_aterm::frontend_control::Controller;
#[cfg(unix)]
pub use codex_alab_aterm::frontend_control::FrontendFrontier;
#[cfg(unix)]
pub use codex_alab_aterm::frontend_control::FrontendSelectionAppOwner;
#[cfg(unix)]
pub use codex_alab_aterm::frontend_control::FrontendSelectionFrontier;
#[cfg(unix)]
pub use codex_alab_aterm::frontend_control::retained_service;
pub use codex_alab_aterm::source_retained_service::StartupSettings;

pub async fn open() -> std::io::Result<Controller> {
    let unavailable = codex_alab_core::managed_auth::unavailable;
    let endpoint = codex_alab_aterm::Endpoint::managed_auth_from_env().ok_or_else(unavailable)?;
    let identity = crate::identity::current().map_err(|_| unavailable())?;
    let birth = crate::identity::proc_start_utc(&identity).ok_or_else(unavailable)?;
    let operation = format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    );
    Controller::open_at(&endpoint, &operation, &birth).await
}

/// A request to try Native control, never an SDK/foreground permission check.
pub fn requested() -> bool {
    codex_alab_aterm::Endpoint::managed_auth_from_env().is_some()
}

#[cfg(unix)]
#[path = "retained_frontend_entry.rs"]
mod retained_entry;
#[cfg(unix)]
pub use retained_entry::EnteredRetainedFrontend;
#[cfg(unix)]
pub use retained_entry::current_retained_frontend;
#[cfg(unix)]
pub use retained_entry::enter_source_when_unretained;
#[cfg(unix)]
pub use retained_entry::with_retained_frontend;

#[cfg(unix)]
pub use codex_alab_aterm::frontend_control::ControllerPackageHostPeer;
