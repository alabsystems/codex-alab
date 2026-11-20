// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::PtyCodex;
use super::write_test_config_with_bundled_models;
use anyhow::Context;
use anyhow::Result;
use pretty_assertions::assert_eq;
use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;

#[test]
fn startup_preserves_an_open_plaintext_log() -> Result<()> {
    let cwd = codex_utils_cargo_bin::repo_root()?;
    for configured in [false, true] {
        let home = codex_alab_tui::test_paths::short_tempdir()?;
        write_test_config_with_bundled_models(home.path(), &cwd)?;
        let log_dir = home.path().join("log");
        fs::create_dir_all(&log_dir)?;
        let path = log_dir.join("codex-tui.log");
        let mut writer = fs::OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)?;
        writeln!(writer, "before startup")?;
        let original = writer.metadata()?;
        let log_override = format!("log_dir={}", serde_json::to_string(&log_dir)?);
        let args: &[&str] = if configured {
            &["--no-daemon", "-c", &log_override]
        } else {
            &["--no-daemon"]
        };
        let mut terminal = PtyCodex::start(&cwd, home, args)?;
        terminal.wait_for_startup()?;
        // The draft header can appear before the running session installs logging.
        terminal.wait_for_screen("GPT-5.6-Terra")?;
        writeln!(writer, "after startup")?;
        let current = fs::metadata(&path).with_context(|| {
            format!("open log lost its pathname (log_dir configured: {configured})")
        })?;
        assert_eq!(
            (current.dev(), current.ino()),
            (original.dev(), original.ino())
        );
        let log = fs::read_to_string(&path)?;
        assert!(log.lines().any(|line| line == "before startup"));
        assert!(log.lines().any(|line| line == "after startup"));
    }
    Ok(())
}
