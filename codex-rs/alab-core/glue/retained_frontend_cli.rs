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
pub(super) fn bind_picker(
    digest: Option<&str>,
    interactive: &mut super::TuiCli,
    root: &super::InteractiveRemoteOptions,
    command: Option<&super::Subcommand>,
) -> anyhow::Result<()> {
    if codex_alab_core::bound_picker::expected() != digest {
        return Err(codex_alab_core::bound_picker::unavailable().into());
    }
    if digest.is_none() {
        return Ok(());
    }
    let Some(super::Subcommand::Resume(resume)) = command else {
        return Err(codex_alab_core::bound_picker::unavailable().into());
    };
    if !local_picker_resume(root, resume) || codex_alab_core::native_source_launch::requested() {
        return Err(codex_alab_core::bound_picker::unavailable().into());
    }
    interactive.no_daemon = true;
    Ok(())
}

fn local_picker_resume(
    root: &super::InteractiveRemoteOptions,
    resume: &super::ResumeCommand,
) -> bool {
    root.remote.is_none()
        && root.remote_auth_token_env.is_none()
        && resume.remote.remote.is_none()
        && resume.remote.remote_auth_token_env.is_none()
        && !resume.last
        && resume
            .session_id
            .as_deref()
            .and_then(|id| codex_protocol::ThreadId::from_string(id).ok())
            .is_some()
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
    if codex_alab_tui::upgrade_state::pending().is_some() {
        anyhow::bail!("an independent frontend cannot open a process-local runtime");
    }
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

#[cfg(test)]
mod picker_tests {
    use super::super::MultitoolCli;
    use clap::CommandFactory;
    use clap::Parser;
    use std::ffi::OsString;

    #[test]
    fn confirmed_picker_flag_is_an_actual_root_option_and_never_enters_generic_resume() {
        const CHILD: &str = "ALAB_TEST_CONFIRMED_PICKER_CLI_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let name = std::thread::current().name().unwrap().to_owned();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture"])
                .env(CHILD, "1")
                .env_remove(codex_alab_core::native_source::LAUNCH_ENV)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "isolated picker CLI check failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
            return;
        }
        let digest = "d".repeat(64);
        let thread = "019a0000-0000-7000-8000-000000000001";
        let argv = [
            "codex",
            "--aterm-bound-picker-v1",
            &digest,
            "-m",
            "o3",
            "resume",
            thread,
        ];
        codex_alab_core::bound_picker::capture(argv.iter().map(OsString::from)).unwrap();
        let allowed = |parsed: &mut MultitoolCli| {
            super::bind_picker(
                parsed.aterm_bound_picker_v1.as_deref(),
                &mut parsed.interactive,
                &parsed.remote,
                parsed.subcommand.as_ref(),
            )
            .is_ok()
        };
        let mut parsed = MultitoolCli::try_parse_from(argv).unwrap();
        assert!(allowed(&mut parsed));
        assert!(parsed.interactive.no_daemon);
        parsed.aterm_bound_picker_v1 = None;
        assert!(!allowed(&mut parsed));
        for suffix in [
            &["resume", "--last"][..],
            &["resume", "nickname"],
            &["--remote", "ws://localhost:1234", "resume", thread],
            &["resume", "--remote", "ws://localhost:1234", thread],
            &["exec", "echo"],
        ] {
            let mut command = MultitoolCli::try_parse_from(
                ["codex", "--aterm-bound-picker-v1", &digest]
                    .into_iter()
                    .chain(suffix.iter().copied()),
            )
            .unwrap();
            assert!(!allowed(&mut command));
        }
        let args = argv.map(OsString::from);
        let rebuilt = codex_alab_tui::resume_argv::rebuild(
            MultitoolCli::command(),
            &args,
            "/pkg/bin/codex",
            thread,
        )
        .unwrap();
        assert_eq!(rebuilt, ["/pkg/bin/codex", "-m", "o3", "resume", thread]);
    }
}
