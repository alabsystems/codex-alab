// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled against the actual CLI parser after fresh native constructor admission.
use super::*;

pub(super) fn stage() -> std::io::Result<codex_alab_tui::upgrade_state::Recovery> {
    Ok(codex_alab_tui::upgrade_state::stage_before_runtime()?
        .with_cleanup(codex_tui::restore_after_failed_upgrade))
}

pub(super) async fn parse(
    result: Result<MultitoolCli, clap::Error>,
) -> anyhow::Result<MultitoolCli> {
    let Some(pending) = codex_alab_tui::upgrade_state::pending() else {
        return Ok(result.unwrap_or_else(|error| error.exit()));
    };
    // Returning the error unwinds the real main-stack terminal recovery owner.
    // clap::Error::exit would bypass that recovery with process::exit.
    let mut cli = result?;
    if !codex_alab_core::native_source_launch::requested() {
        anyhow::bail!(
            "custodied runtime configuration requires fresh native constructor admission"
        );
    }
    let primary = explicit_local_resume(&cli)?;
    pending.validate_primary(&primary.to_string())?;
    pending.admit().await?;
    // The forest lives in the embedded server even when another local daemon
    // is running. finalize_resume_interactive preserves this launch choice.
    cli.interactive.no_daemon = true;
    Ok(cli)
}
fn explicit_local_resume(cli: &MultitoolCli) -> anyhow::Result<codex_protocol::ThreadId> {
    let Some(Subcommand::Resume(command)) = &cli.subcommand else {
        anyhow::bail!("runtime restoration requires explicit local resume routing");
    };
    if command.last
        || cli.remote.remote.is_some()
        || command.remote.remote.is_some()
        || cli.remote.remote_auth_token_env.is_some()
        || command.remote.remote_auth_token_env.is_some()
        || [&cli.interactive, &command.config_overrides.0]
            .iter()
            .any(|options| {
                options.shared.worktree
                    || options.prompt.is_some()
                    || !options.shared.images.is_empty()
            })
    {
        anyhow::bail!("runtime restoration requires the original local session without new input");
    }
    let primary = command
        .session_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("runtime restoration requires its original thread ID"))?;
    codex_protocol::ThreadId::from_string(primary).map_err(Into::into)
}
#[cfg(all(test, unix))]
#[path = "upgrade_cli_route_pty_tests.rs"]
mod pty;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::ffi::OsString;

    #[test]
    fn source_resume_argv_routes_the_original_primary_without_replaying_input() {
        let primary = "00000000-0000-4000-8000-000000000001";
        let previous = "019a0000-0000-7000-8000-000000000002";
        let original = [
            "codex",
            "--no-alt-screen",
            "--image=old.png",
            "resume",
            "-m",
            "gpt-6.2-sol",
            previous,
            "already submitted",
        ]
        .map(OsString::from);
        let rebuilt = codex_alab_tui::resume_argv::rebuild(
            MultitoolCli::command(),
            &original,
            "codex",
            primary,
        )
        .expect("original CLI can resume its primary");
        assert_eq!(
            rebuilt,
            [
                "codex",
                "--no-alt-screen",
                "resume",
                "-m",
                "gpt-6.2-sol",
                primary
            ]
        );
        let cli = MultitoolCli::try_parse_from(rebuilt).unwrap();
        assert_eq!(explicit_local_resume(&cli).unwrap().to_string(), primary);

        for arguments in [
            vec!["codex", "resume", primary, "unsubmitted prompt"],
            vec!["codex", "--image=new.png", "resume", primary],
            vec!["codex", "resume", "--image=new.png", primary],
            vec!["codex", "--worktree", "resume", primary],
            vec!["codex", "resume", "--worktree", primary],
        ] {
            let cli = MultitoolCli::try_parse_from(&arguments).unwrap();
            assert!(explicit_local_resume(&cli).is_err(), "{arguments:?}");
        }
    }

    #[test]
    fn runtime_resume_routing_requires_the_original_explicit_local_id() {
        let primary = "00000000-0000-4000-8000-000000000001";
        let cli = MultitoolCli::try_parse_from(["codex", "resume", primary]).unwrap();
        assert_eq!(explicit_local_resume(&cli).unwrap().to_string(), primary);
        for args in [
            vec!["codex"],
            vec!["codex", "resume"],
            vec!["codex", "resume", "--last"],
            vec!["codex", "resume", "session-name"],
            vec!["codex", "--remote", "ssh://example.test", "resume", primary],
            vec!["codex", "resume", "--remote", "ssh://example.test", primary],
        ] {
            let cli = MultitoolCli::try_parse_from(args).unwrap();
            assert!(explicit_local_resume(&cli).is_err());
        }
    }
}
