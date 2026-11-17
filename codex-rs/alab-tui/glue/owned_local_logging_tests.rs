// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the shared original subscriber in an isolated real child; global
//! logging in the parent test process must not hide a failed installation.
use super::*;
use color_eyre::eyre::ensure;
use tempfile::TempDir;
const CHILD_HOME: &str = "ALAB_SHARED_LOGGING_TEST_HOME";

#[tokio::test]
async fn original_logging_subscriber_writes_private_configured_file() -> color_eyre::Result<()> {
    if let Some(home) = std::env::var_os(CHILD_HOME) {
        let home = PathBuf::from(home);
        let log_dir = home.join("actual-logs");
        let config = ConfigBuilder::default()
            .codex_home(home.clone())
            .fallback_cwd(Some(home))
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .cli_overrides(vec![(
                "log_dir".into(),
                toml::Value::String(log_dir.to_string_lossy().into_owned()),
            )])
            .build()
            .await?;
        let file = prepare_file(&config)?.ok_or_else(|| {
            color_eyre::eyre::eyre!("original configured file policy was not selected")
        })?;
        let guard = install(
            Some(file),
            &codex_feedback::CodexFeedback::new(),
            None,
            None,
        );
        tracing::info!(target: "codex_tui", "actual shared subscriber control");
        drop(guard); // Same original nonblocking writer observes its final flush.
        return Ok(());
    }
    let home = TempDir::new()?;
    let mut child = tokio::process::Command::new(std::env::current_exe()?);
    child.arg("--exact").arg("owned_local_logging::tests::original_logging_subscriber_writes_private_configured_file")
        .arg("--nocapture").env(CHILD_HOME, home.path()).env("RUST_LOG", "codex_tui=info").kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), child.output()).await??;
    ensure!(
        output.status.success(),
        "actual logging child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let file = home.path().join("actual-logs").join(TUI_LOG_FILE_NAME);
    ensure!(std::fs::read_to_string(&file)?.contains("actual shared subscriber control"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(std::fs::metadata(&file)?.permissions().mode() & 0o777 == 0o600);
    }
    Ok(())
}
