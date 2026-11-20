// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::tools::runtimes::RuntimePathPrepends;
use crate::tools::runtimes::maybe_wrap_shell_lc_with_snapshot;
use core_test_support::PathExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[tokio::test]
async fn canceling_validation_preserves_the_first_captured_value_until_its_owner_releases_it() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let home = dir.path().abs();
    let session = ThreadId::new();
    let generation = dir.path().join("capture-generation");
    std::fs::write(&generation, "first")?;
    let shell_path = dir.path().join("test-shell");
    std::fs::write(
        &shell_path,
        r#"#!/bin/sh
if [ "$1" = "-c" ] && [ -f pause-validation ]; then
    : > validation-started
    exec /bin/sleep 30
fi
if [ -f capture-generation ]; then
    exec /usr/bin/env -i HOME="$PWD" PATH=/usr/bin:/bin SNAPSHOT_FIXTURE_GENERATION="$(/bin/cat capture-generation)" /bin/bash --noprofile --norc "$@"
fi
exec /bin/bash --noprofile --norc "$@"
"#,
    )?;
    std::fs::set_permissions(&shell_path, std::fs::Permissions::from_mode(0o700))?;
    let shell = Shell {
        shell_type: ShellType::Bash,
        shell_path,
    };
    let policy = ShellEnvironmentPolicy::default();
    let initial = ShellSnapshot::try_create(
        &home, session, &home, &policy, &shell, /*state_db*/ None,
        /*credential_broker*/ None, /*sandbox*/ None,
    )
    .await
    .expect("initial snapshot should be published");
    let snapshot_dir = home.join(SNAPSHOT_DIR);
    let snapshot_paths = || {
        std::fs::read_dir(&snapshot_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<PathBuf>>>()
    };
    std::fs::write(home.join("pause-validation"), "")?;
    std::fs::write(&generation, "second")?;
    let validation_started = home.join("validation-started");
    let pending_shell = shell.clone();
    let task = tokio::spawn(async move {
        ShellSnapshot::try_create(
            &home,
            session,
            &home,
            &policy,
            &pending_shell,
            /*state_db*/ None,
            /*credential_broker*/ None,
            /*sandbox*/ None,
        )
        .await
    });
    timeout(SNAPSHOT_TIMEOUT, async {
        while !validation_started.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("second shell never started validation")?;
    assert_eq!(snapshot_paths()?.len(), 2);

    task.abort();
    assert!(matches!(task.await, Err(err) if err.is_cancelled()));
    assert_eq!(snapshot_paths()?, vec![initial.path.to_path_buf()]);
    std::fs::remove_file(generation)?;
    std::fs::remove_file(dir.path().join("pause-validation"))?;
    let snapshot = initial.path();
    let env = HashMap::from([
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
        (
            "SNAPSHOT_FIXTURE_GENERATION".to_string(),
            "fresh-shell".to_string(),
        ),
    ]);
    let command = shell.derive_exec_args(
        r#"printf '%s' "$SNAPSHOT_FIXTURE_GENERATION""#,
        /*use_login_shell*/ true,
    );
    let replay = || -> Result<Vec<u8>> {
        let argv = maybe_wrap_shell_lc_with_snapshot(
            &command,
            &shell,
            Some(&snapshot),
            &HashMap::new(),
            &env,
            &RuntimePathPrepends::default(),
        );
        let result = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(dir.path())
            .env_clear()
            .envs(&env)
            .output()?;
        anyhow::ensure!(
            result.status.success(),
            "replay failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(result.stdout)
    };
    assert_eq!(replay()?, b"first");
    drop(initial);
    assert!(snapshot_paths()?.is_empty());
    assert_eq!(replay()?, b"fresh-shell");
    Ok(())
}
