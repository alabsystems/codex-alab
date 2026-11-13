// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled against the actual CLI parser after fresh native constructor admission.
use super::*;

pub(super) fn stage() -> std::io::Result<codex_alab_tui::upgrade_state::Recovery> {
    Ok(codex_alab_tui::upgrade_state::stage_before_runtime()?
        .with_cleanup(codex_tui::restore_after_failed_upgrade))
}

pub(super) fn parse(result: Result<MultitoolCli, clap::Error>) -> anyhow::Result<MultitoolCli> {
    let Some(pending) = codex_alab_tui::upgrade_state::pending() else {
        return Ok(result.unwrap_or_else(|error| error.exit()));
    };
    // Returning the error unwinds the real main-stack terminal recovery owner.
    // clap::Error::exit would bypass that recovery with process::exit.
    let cli = result?;
    if !codex_alab_core::native_source_launch::requested() {
        anyhow::bail!(
            "custodied runtime configuration requires fresh native constructor admission"
        );
    }
    let primary = explicit_local_resume(&cli)?;
    pending.validate_primary(&primary.to_string())?;
    // Full state must enter the all-root bootstrap API. Never treat a validated
    // capsule as permission for ordinary disk resume, which loses private roots.
    anyhow::bail!("custodied runtime state requires all-root startup; ordinary resume is refused")
}
fn explicit_local_resume(cli: &MultitoolCli) -> anyhow::Result<codex_protocol::ThreadId> {
    let Some(Subcommand::Resume(command)) = &cli.subcommand else {
        anyhow::bail!("runtime restoration requires explicit local resume routing");
    };
    if command.last || cli.remote.remote.is_some() || command.remote.remote.is_some() {
        anyhow::bail!("runtime restoration cannot select a remote or implicit session");
    }
    let primary = command
        .session_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("runtime restoration requires its original thread ID"))?;
    codex_protocol::ThreadId::from_string(primary).map_err(Into::into)
}
#[cfg(test)]
mod tests {
    use super::*;
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
