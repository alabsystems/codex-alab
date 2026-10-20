// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The launch snapshot's `argv[0]` resolution.

use crate::life;
use pretty_assertions::assert_eq;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

// ---- argv[0] through the launch PATH

fn executable(dir: &Path, name: &str, mode: u32) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, "#!/bin/sh\n").unwrap_or_else(|err| panic!("{err}"));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|err| panic!("{err}"));
    path
}

#[test]
fn argv0_resolves_through_the_launch_path() {
    let root = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let first = root.path().join("first");
    let second = root.path().join("second");
    for dir in [&first, &second] {
        std::fs::create_dir(dir).unwrap_or_else(|err| panic!("{err}"));
    }
    executable(&first, "codex", 0o644);
    let twin = executable(&second, "codex", 0o755);
    let path = std::env::join_paths([&first, &second]).unwrap_or_else(|err| panic!("{err}"));
    let cwd = Some(Path::new("/launch/cwd"));
    assert_eq!(
        life::resolve_argv0("codex".as_ref(), Some(&path), cwd),
        Some(twin)
    );
    assert_eq!(
        life::resolve_argv0("./bin/codex".as_ref(), Some(&path), cwd),
        Some(PathBuf::from("/launch/cwd/./bin/codex"))
    );
    assert_eq!(
        life::resolve_argv0("/abs/codex".as_ref(), None, None),
        Some(PathBuf::from("/abs/codex"))
    );
    assert_eq!(
        life::resolve_argv0("missing".as_ref(), Some(&path), cwd),
        None
    );
}
