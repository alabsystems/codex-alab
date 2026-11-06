// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep process-global ALab activation out of shared upstream test processes.

pub(super) fn child(test: &str) -> bool {
    let (_, exact) = test.split_once("::").expect("fully qualified test name");
    let args = ["--exact", exact, "--nocapture"];
    if std::env::var("ALAB_EXEC_TEST_CHILD").as_deref() == Ok(exact) {
        assert_eq!(std::env::args().skip(1).collect::<Vec<_>>(), args);
        return true;
    }
    let root = tempfile::tempdir().expect("isolated test home");
    let guard =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../alab/command_guard.py");
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
    false
}
