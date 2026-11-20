// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn public_compile_admits_original_allocating_commands_and_leaves_diagnostics_to_targo() {
    for (words, needs_disk) in [
        (&["--unverified", "build", "--target-dir", "out"][..], true),
        (&["trust", "check", "--locked"], true),
        (&["fetch", "--locked"], true),
        (&["--unverified", "install", "ripgrep"], true),
        (&["run", "--", "--help"], true),
        (&["run", "argument", "--help"], true),
        (&["run", "argument", "--version"], true),
        (&["test", "--", "--list"], true),
        (&["build", "--target-dir", "--help"], true),
        (&["install", "--root", "--list"], true),
        (&["--version"], false),
        (&["--unverified", "check", "--help"], false),
        (&["run", "-h"], false),
        (&["--unverified", "install", "--list"], false),
        (&["package", "-l"], false),
        (&["metadata", "--manifest-path", "build"], false),
        (&["--config", "build", "clean"], false),
        (&["clean", "--target-dir", "check"], false),
        (&["trust", "status", "build"], false),
        (&["custom-lint", "--", "test"], false),
    ] {
        let arguments: Vec<_> = words.iter().map(OsString::from).collect();
        assert_eq!(allocating_compile(&arguments), needs_disk, "{words:?}");
    }
}

#[test]
fn full_metadata_uses_the_dependency_cache_even_when_offline() {
    for (words, resolves) in [
        (&["metadata"][..], true),
        (&["metadata", "--locked", "--format-version", "1"], true),
        (&["--offline", "metadata", "--format-version=1"], true),
        (&["metadata", "--frozen", "--manifest-path=--no-deps"], true),
        (
            &["metadata", "--manifest-path", "build", "--no-deps"],
            false,
        ),
        (
            &["--locked", "metadata", "--no-deps", "--format-version=1"],
            false,
        ),
        (&["metadata", "--filter-platform", "arm", "--help"], false),
        (&["--version", "metadata"], false),
        (&["metadata", "--version"], false),
        (&["metadata", "-V"], false),
        (&["--unverified", "metadata"], false),
        (&["metadata", "--unverified"], false),
        (&["--config", "metadata", "check"], false),
        (&["run", "--", "metadata"], false),
    ] {
        let arguments: Vec<_> = words.iter().map(OsString::from).collect();
        assert_eq!(resolves_metadata(&arguments), resolves, "{words:?}");
    }
}

#[test]
fn every_write_volume_must_meet_its_own_floor() {
    let package = Path::new("target");
    let dependencies = Path::new("cargo");
    let room = DEFAULT_PACKAGE_GIB * GIB;
    let volumes = [(package, room), (dependencies, BUILD_FLOOR)];
    let probe = |path: &Path| {
        if path == package { room } else { BUILD_FLOOR }
    };
    require(volumes, |path| Ok(probe(path))).unwrap();
    for low in [package, dependencies] {
        let error = require(volumes, |path| Ok(probe(path) - u64::from(path == low))).unwrap_err();
        assert!(error.is::<ResourceStop>());
        assert!(error.to_string().contains(low.to_str().unwrap()));
    }
    let error = require(volumes, |_| Err(io::Error::other("unavailable"))).unwrap_err();
    assert!(error.is::<ResourceStop>());
}

#[test]
fn missing_outputs_use_the_existing_parent_and_dangling_links_cannot_admit() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("campaign").join("output");
    assert!(available(&missing).is_ok());
    assert!(!missing.exists());
    let file = temp.path().join("regular");
    std::fs::write(&file, b"").unwrap();
    assert!(available(&file).is_err());
    assert!(available(&file.join("child")).is_err());
    let dangling = temp.path().join("link");
    std::os::unix::fs::symlink(&missing, &dangling).unwrap();
    assert!(available(&dangling.join("output")).is_err());
}

#[test]
fn configured_package_room_respects_the_compiler_floor_and_rejects_overflow() {
    use std::ffi::OsStr;
    assert_eq!(package_bytes(None).unwrap(), 35 * GIB);
    assert_eq!(package_bytes(Some(OsStr::new(""))).unwrap(), 35 * GIB);
    assert_eq!(package_bytes(Some(OsStr::new("48"))).unwrap(), 48 * GIB);
    assert_eq!(package_bytes(Some(OsStr::new("1"))).unwrap(), BUILD_FLOOR);
    assert!(package_bytes(Some(OsStr::new("-1"))).is_err());
    assert!(package_bytes(Some(OsStr::new("18446744073709551615"))).is_err());
}
