// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep process-global ALab activation out of shared upstream test processes.

use anyhow::Context as _;
use std::path::Path;
use std::path::PathBuf;

fn checkout(directory: &Path) -> Option<&Path> {
    directory
        .ancestors()
        .find(|root| root.join("codex-rs/Cargo.toml").is_file())
}

/// A content-reused test executable must run the guardian from the checkout under test.
pub fn command_guard() -> anyhow::Result<PathBuf> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(|path| PathBuf::from(path).canonicalize())
        .transpose()?;
    let cwd_root = checkout(&cwd);
    let root = if let Some(manifest) = manifest.as_deref() {
        let root = checkout(manifest).context("runtime manifest is outside the Codex checkout")?;
        anyhow::ensure!(
            cwd_root.is_none_or(|cwd_root| cwd_root == root),
            "test working directory and runtime manifest name different checkouts"
        );
        root
    } else {
        cwd_root.context("run the test from its Codex checkout or set CARGO_MANIFEST_DIR")?
    };
    let guard = root.join("alab/command_guard.py");
    anyhow::ensure!(
        guard.is_file(),
        "missing process guardian at {}",
        guard.display()
    );
    Ok(guard)
}

/// Runs the named test in an isolated process and returns true inside the child.
pub fn child(test: &str) -> bool {
    child_with_stderr(test, |_| {})
}

/// Check the successful child's captured stderr in the parent.
pub fn child_with_stderr(test: &str, check: impl FnOnce(&str)) -> bool {
    let (_, exact) = test.split_once("::").expect("fully qualified test name");
    let args = ["--exact", exact, "--nocapture"];
    if std::env::var("ALAB_EXEC_TEST_CHILD").as_deref() == Ok(exact) {
        assert_eq!(std::env::args().skip(1).collect::<Vec<_>>(), args);
        return true;
    }
    let root = tempfile::tempdir().expect("isolated test home");
    let guard = command_guard().expect("runtime process guardian");
    let output = std::process::Command::new("/usr/bin/python3")
        .arg(&guard)
        .args(["--", "/usr/bin/python3"])
        .arg(&guard)
        .args(["--inherited-group-timeout", "60", "--"])
        .arg(std::env::current_exe().expect("test executable"))
        .args(args)
        .env_clear()
        .env("ALAB_EXEC_TEST_CHILD", exact)
        .env("HOME", root.path())
        .env("TMPDIR", root.path())
        .env("PATH", "/usr/bin:/bin")
        .env("RUST_MIN_STACK", "16777216")
        .output()
        .expect("isolated test child");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "isolated test failed: {stdout}\n{stderr}"
    );
    assert!(stdout.contains("1 passed; 0 failed"), "{stdout}\n{stderr}");
    check(&stderr);
    false
}
