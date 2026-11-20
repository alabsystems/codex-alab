// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use crate::{ChildFds, TerminalSize, spawn_pty_process};
use std::collections::HashMap;

#[tokio::test]
async fn invalid_explicit_directory_never_starts_a_pty_command() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let ordinary_file = root.path().join("file");
    std::fs::write(&ordinary_file, b"not a directory")?;
    let marker = root.path().join("command-ran");
    let env = HashMap::from([("HOME".into(), root.path().to_string_lossy().into_owned())]);
    for cwd in [root.path().join("absent"), ordinary_file] {
        let args = vec![
            "-c".to_owned(),
            r#"printf ran > "$1""#.to_owned(),
            "fixture".to_owned(),
            marker.to_string_lossy().into_owned(),
        ];
        let result = spawn_pty_process(
            "/bin/sh",
            &args,
            &cwd,
            &env,
            &None,
            TerminalSize::default(),
            ChildFds::Inherited(&[]),
        )
        .await;
        anyhow::ensure!(result.is_err(), "a PTY child launched with cwd {cwd:?}");
        anyhow::ensure!(
            !marker.exists(),
            "the PTY command ran outside its requested directory"
        );
    }
    Ok(())
}

#[tokio::test]
async fn ordinary_pty_keeps_child_path_argv0_shell_and_controlling_terminal() -> anyhow::Result<()>
{
    let root = tempfile::tempdir()?;
    let cwd = root.path().join("work");
    let bin = cwd.join("tools");
    std::fs::create_dir_all(&bin)?;
    std::os::unix::fs::symlink("/bin/sh", bin.join("alias-shell"))?;
    let env = HashMap::from([
        ("PATH".into(), "tools".into()),
        ("HOME".into(), root.path().to_string_lossy().into_owned()),
        ("SHELL".into(), "/bin/sh".into()),
    ]);
    let script = r#"test -t 0 && test -t 1 && test -t 2 && ( : </dev/tty ) && printf '%s|%s|' "$0" "$SHELL" && /bin/pwd -P"#;
    let args = vec!["-c".into(), script.into()];
    let origin = std::fs::canonicalize(&cwd)?;
    for (program, arg0, expected) in [
        ("alias-shell", None, "alias-shell"),
        (
            "/not-the-program",
            Some("alias-shell".to_owned()),
            "alias-shell",
        ),
        ("./tools/alias-shell", None, "./tools/alias-shell"),
        ("/bin/sh", None, "/bin/sh"),
    ] {
        let mut child = spawn_pty_process(
            program,
            &args,
            &cwd,
            &env,
            &arg0,
            TerminalSize::default(),
            ChildFds::Inherited(&[]),
        )
        .await?;
        let mut text = Vec::new();
        while let Some(bytes) = child.stdout_rx.recv().await {
            text.extend(bytes);
        }
        let code = child.exit_rx.await?;
        assert_eq!(code, 0);
        let observed = String::from_utf8(text)?;
        assert_eq!(
            observed.trim_end(),
            format!("{expected}|/bin/sh|{}", origin.display())
        );
    }
    let no_path = HashMap::from([("HOME".into(), root.path().to_string_lossy().into_owned())]);
    assert!(
        spawn_pty_process(
            "sh",
            &args,
            &cwd,
            &no_path,
            &None,
            TerminalSize::default(),
            ChildFds::Inherited(&[])
        )
        .await
        .is_err()
    );
    Ok(())
}

#[test]
fn omitted_pty_directory_keeps_home_as_its_default() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.env_clear();
    command.env("HOME", home.path());
    command.args(["-c", r#"test "$(/bin/pwd -P)" = "$1""#, "fixture"]);
    command.arg(std::fs::canonicalize(home.path())?);
    let pair = portable_pty::native_pty_system().openpty(portable_pty::PtySize::default())?;
    let mut child = pair.slave.spawn_command(command)?;
    assert_eq!(child.wait()?.exit_code(), 0);
    Ok(())
}
