// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real earliest private C2 entry. Marker selectors never create permission.
//! Only actual Native Entered/E1 +whole decoded owner reaches original dispatch.
use std::ffi::OsString;
use std::io;

pub(super) fn arguments() -> io::Result<Vec<OsString>> {
    #[cfg(unix)]
    if let Some(actual) = codex_alab_tui::frontend_control::current_retained_frontend() {
        return Ok(actual.original_arguments().to_vec());
    }
    super::alab_retained_cli::arguments()
}
pub(super) fn validate_route(subcommand: Option<&super::Subcommand>) -> anyhow::Result<()> {
    #[cfg(unix)]
    if codex_alab_tui::frontend_control::current_retained_frontend().is_some()
        && !matches!(
            subcommand,
            None | Some(
                super::Subcommand::Agents(_)
                    | super::Subcommand::Resume(_)
                    | super::Subcommand::Fork(_)
            )
        )
    {
        anyhow::bail!("entered retained frontend requires original interactive CLI workflow");
    }
    let _ = subcommand;
    Ok(())
}
#[cfg(unix)]
pub(super) async fn run_if_requested(
    arg0: codex_arg0::Arg0DispatchPaths,
    remote_disabled: bool,
) -> anyhow::Result<bool> {
    let Some(entry) = codex_alab_tui::frontend_control::EnteredRetainedFrontend::acquire(
        std::env::args_os().collect(),
    )
    .await?
    else {
        return Ok(false);
    };
    // Caller retains SAME entry through every decode/restoration/dispatch error.
    let readers = codex_tui::RetainedFrontendReaders::from_entered(&entry)?;
    readers.restore_terminal_before_init()?;
    readers
        .scope(async move {
            let _original =
                codex_alab_tui::frontend_control::enter_source_when_unretained().await?;
            if let Some(tui) = Box::pin(super::dispatch_cli(arg0, remote_disabled)).await? {
                tui.await?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await?;
    Ok(true)
}
