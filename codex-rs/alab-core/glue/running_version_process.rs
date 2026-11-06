// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep global running identities inside exact, privately configured test children.

use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

pub(crate) const COMPLETE: &str = "ALAB_RUNNING_VERSION_CONSUMERS_OK";
const CHILD: &str = "ALAB_RUNNING_VERSION_CHILD";

pub(crate) fn child_name(module: &str) -> String {
    let (_, module) = module.split_once("::").expect("test module path");
    format!("{module}::running_version_consumer_child")
}

/// Run the module's child once per identity; each child's stderr, in order (what a child
/// printed to a person).
pub(crate) fn run(module: &str) -> std::io::Result<Vec<String>> {
    let name = child_name(module);
    let mut stderr = Vec::new();
    for mode in ["a", "b"] {
        let world = tempfile::Builder::new()
            .prefix("alab-running-version-")
            .tempdir()?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["--exact", &name, "--nocapture", "--test-threads=1"])
            .env_clear()
            .env(CHILD, mode)
            .env("HOME", world.path())
            .env("CODEX_HOME", world.path().join("codex"))
            .env("TMPDIR", world.path())
            .env("USER", "alab-test")
            .env("LOGNAME", "alab-test")
            .env("PATH", "/usr/bin:/bin")
            .current_dir(world.path())
            .stdin(Stdio::null());
        for (key, directory) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "runtime"),
        ] {
            command.env(key, world.path().join(directory));
        }
        // File-backed logs cannot hold a pipe drain open. One minute detects a
        // stuck child; it is not a latency budget.
        let stdout_path = world.path().join("child.stdout");
        let stderr_path = world.path().join("child.stderr");
        command
            .stdout(std::fs::File::create(&stdout_path)?)
            .stderr(std::fs::File::create(&stderr_path)?);
        let mut child = command.spawn()?;
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "version child exceeded the one-minute hang detector",
                        ));
                    }
                    std::thread::sleep(remaining.min(Duration::from_millis(10)));
                }
                Err(error) => break Err(error),
            }
        };
        let status = status.map_err(|error| {
            // Attempt both operations even when waiting or killing fails, so
            // an error never leaves the child running or unreaped by choice.
            let killed = child.kill();
            let reaped = child.wait();
            std::io::Error::new(
                error.kind(),
                format!("version child {mode}: {error}; kill={killed:?}; reap={reaped:?}"),
            )
        });
        let stdout_bytes = std::fs::read(stdout_path)?;
        let stderr_bytes = std::fs::read(stderr_path)?;
        let stdout = String::from_utf8_lossy(&stdout_bytes);
        let child_stderr = String::from_utf8_lossy(&stderr_bytes);
        let status = status.map_err(|error| {
            std::io::Error::new(error.kind(), format!("{error}\n{stdout}\n{child_stderr}"))
        })?;
        assert!(
            status.success(),
            "version child {mode}: {stdout}\n{child_stderr}"
        );
        assert!(
            stdout.contains(COMPLETE),
            "version child did not reach consumers: {stdout}"
        );
        assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
        stderr.push(child_stderr.into_owned());
    }
    Ok(stderr)
}

pub(crate) fn identity(module: &str) -> Option<(&'static str, &'static str)> {
    let name = child_name(module);
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args != ["--exact", &name, "--nocapture", "--test-threads=1"] {
        return None;
    }
    match std::env::var(CHILD).as_deref() {
        Ok("a") => Some(("0.165.0-dev.1+gaaaaaaaaa", "0.165.0")),
        Ok("b") => Some(("0.166.0-dev.2+gbbbbbbbbb", "0.166.0")),
        _ => None,
    }
}
