/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

fn main() {
    rust_nightly();
    // ALAB(toolchain): Infallible can be an alias of ! on newer compilers. Test the
    // actual types instead of tying this compatibility boundary to a version string.
    println!("cargo:rustc-check-cfg=cfg(infallible_is_never)");
    println!("cargo:rustc-check-cfg=cfg(stable_never_type)");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    // ALAB(toolchain): enable the feature only when this compiler still needs it.
    let stable_probe = output.join("stable_never_type.rs");
    std::fs::write(&stable_probe, "pub fn identity(value: !) -> ! { value }\n").unwrap();
    if std::process::Command::new(std::env::var_os("RUSTC").unwrap())
        .arg("--crate-type=lib")
        .arg("--emit=metadata")
        .arg("--out-dir")
        .arg(&output)
        .arg(stable_probe)
        .output()
        .unwrap()
        .status
        .success()
    {
        println!("cargo:rustc-cfg=stable_never_type");
    }
    let probe = output.join("infallible_is_never.rs");
    std::fs::write(
        &probe,
        "#![feature(never_type)]\npub fn alias(value: std::convert::Infallible) -> ! { value }\n",
    )
    .unwrap();
    let result = std::process::Command::new(std::env::var_os("RUSTC").unwrap())
        .arg("--crate-type=lib")
        .arg("--emit=metadata")
        .arg("--out-dir")
        .arg(&output)
        .arg(probe)
        .output()
        .unwrap();
    if result.status.success() {
        println!("cargo:rustc-cfg=infallible_is_never");
    }
}

fn rust_nightly() {
    let rustc = std::env::var("RUSTC").unwrap();
    let version = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .unwrap();

    assert!(version.status.success());

    // Nightly output:
    // rustc 1.64.0-nightly (affe0d3a0 2022-08-05)
    // Stable output:
    // rustc 1.64.0 (a55dd71d5 2022-09-19)
    // Meta internal output:
    // rustc 1.64.0-dev

    let stdout = String::from_utf8(version.stdout).unwrap();
    assert!(stdout.contains("rustc"), "Sanity check");
    let nightly = stdout.contains("nightly") || stdout.contains("dev");
    if nightly {
        println!("cargo:rustc-cfg=rust_nightly");
    }
}
