// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Frontend-only preparation on actual Native deliveries, before starting its
//! normal TUI event/input owner. Backend launch/bootstrap stays in the backend.
use super::owned_local_frontend::OwnedLocalTarget;
use super::owned_local_frontend::StartingFrontend;
use codex_alab_tui::frontend_control::StartupSettings;
use std::ffi::OsString;
use std::io;

/// Preserve the original invocation, not a reconstructed TUI argument subset.
/// These bytes select data only; Native observes/adopts the real caller image.
pub(crate) struct OriginalLaunch {
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
}
impl OriginalLaunch {
    pub(crate) fn capture() -> Self {
        let arguments = codex_alab_tui::frontend_control::current_retained_frontend()
            .map(|entry| entry.original_arguments().iter().skip(1).cloned().collect())
            .unwrap_or_else(|| std::env::args_os().skip(1).collect());
        Self {
            arguments,
            environment: std::env::vars_os().collect(),
        }
    }
}

/// The caller has completed its original config-server foreground interaction
/// and has not started a competing TUI input reader. The backend parses the
/// SAME original CLI, so only genuine OSS invocations consume the picker phase.
pub(crate) async fn prepare(launch: OriginalLaunch, oss: bool) -> io::Result<OwnedLocalTarget> {
    if codex_alab_tui::frontend_control::current_retained_frontend().is_some() {
        return Err(io::Error::other(
            "entered frontend requires its original retained UI decoder, not fresh backend bootstrap",
        ));
    }
    let controller = codex_alab_tui::frontend_control::open().await?;
    let settings = StartupSettings::new(&launch.arguments, &launch.environment)?;
    let mut starting = StartingFrontend::new(controller, settings)?;
    let target = starting.observe().await?;
    if oss {
        // The original picker itself uses the existing protected input owner;
        // an automatic provider does not create a new input/idle barrier.
        let mut endpoint = target.preparation_endpoint().await?;
        let prepared =
            super::owned_local_prepare::channel::serve_frontend(endpoint.take_stream()?).await;
        let closed = endpoint.close().await;
        prepared?;
        closed?;
    }
    // A distinct admitted delivery conveys exact backend configuration DATA.
    // It closes before the final, separately admitted WebSocket endpoint.
    let mut endpoint = target.preparation_endpoint().await?;
    let snapshot = super::owned_local_ui_snapshot::read(endpoint.stream_mut()?).await;
    let closed = endpoint.close().await;
    let snapshot = snapshot?;
    closed?;
    target.install_snapshot(snapshot)?;
    Ok(target)
}

pub(crate) fn selected_cwd(
    target: &crate::AppServerTarget,
    original: Option<std::path::PathBuf>,
) -> io::Result<Option<std::path::PathBuf>> {
    match target {
        crate::AppServerTarget::OwnedLocal { target } => Ok(Some(
            target.snapshot()?.selected_cwd.as_path().to_path_buf(),
        )),
        _ => Ok(original),
    }
}

pub(crate) fn oss_choice(
    target: &crate::AppServerTarget,
    oss: bool,
    original: Option<String>,
    manually_selected: &mut Option<String>,
) -> io::Result<Option<String>> {
    let crate::AppServerTarget::OwnedLocal { target } = target else {
        return Ok(original);
    };
    match (oss, target.snapshot()?.oss.as_ref()) {
        (false, None) => Ok(None),
        (true, Some(choice)) => {
            if choice.manually_selected {
                *manually_selected = Some(choice.provider.clone());
            }
            Ok(Some(choice.provider.clone()))
        }
        _ => Err(io::Error::other("owned local provider selection mismatch")),
    }
}

/// The SAME authenticated E1 UI envelope decoder must supply this snapshot.
/// This only prepares target/controller custody. It neither applies E1 UI state
/// nor authorizes original fresh startup/config/auth policy to be replayed.
pub(crate) fn prepare_entered(
    entry: std::sync::Arc<codex_alab_tui::frontend_control::EnteredRetainedFrontend>,
    snapshot: super::owned_local_ui_snapshot::UiSnapshot,
) -> io::Result<OwnedLocalTarget> {
    OwnedLocalTarget::from_entered(entry, snapshot)
}
