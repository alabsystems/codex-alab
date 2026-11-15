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
    // ALAB(toolchain): Probe language behavior instead of tying compatibility boundaries to
    // version strings. Keep the probes declaration-only: Trust verifies function bodies even in
    // compiler probes.
    println!("cargo:rustc-check-cfg=cfg(infallible_is_never)");
    println!("cargo:rustc-check-cfg=cfg(stable_never_type)");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let stable_never_type = probe(
        &output,
        "never_type_stable.rs",
        "const _: Option<!> = None;\n",
    );
    if stable_never_type {
        println!("cargo:rustc-cfg=stable_never_type");
    }
    let alias_source = if stable_never_type {
        "const _: Option<!> = None::<std::convert::Infallible>;\n"
    } else {
        "#![feature(never_type)]\nconst _: Option<!> = None::<std::convert::Infallible>;\n"
    };
    if probe(&output, "infallible_is_never.rs", alias_source) {
        println!("cargo:rustc-cfg=infallible_is_never");
    }
}

fn probe(output: &std::path::Path, name: &str, source: &str) -> bool {
    let probe = output.join(name);
    std::fs::write(&probe, source).unwrap();
    let result = std::process::Command::new(std::env::var_os("RUSTC").unwrap())
        .arg("--crate-type=lib")
        .arg("--emit=metadata")
        .arg("--out-dir")
        .arg(&output)
        .arg(probe)
        .output()
        .unwrap();
    result.status.success()
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
