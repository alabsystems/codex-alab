// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::schedule::Ready;
use crate::source::Checkout;
use std::fs;

fn observation(campaign: &Path, stage: &str, value: &impl Serialize) -> Result<PathBuf> {
    let bytes = serde_json::to_vec(value)?;
    let digest = sha2::Sha256::digest(&bytes);
    let path = campaign.join(format!("{stage}-{digest:x}.json"));
    crate::save_record(&path, &bytes)?;
    Ok(path)
}

#[test]
fn resuming_checks_both_observations_against_the_same_checkout_and_actual_outputs() -> Result<()> {
    let f = crate::workflow::tests::fixture()?;
    let repo = &f.repo;
    let state = &f.state;
    let pins = repo.join("scripts/codex_package");
    fs::create_dir_all(&pins)?;
    fs::write(
        pins.join("rg"),
        include_bytes!("../../../scripts/codex_package/rg"),
    )?;
    fs::write(
        repo.join("alab/versions.toml"),
        format!(
            "[[release]]\nversion='0.6.0'\nbaseline='0.167.1'\nbaseline_commit='{}'\n",
            f.release.source
        ),
    )?;
    crate::git(
        repo,
        &[
            "update-ref",
            "refs/remotes/upstream/main",
            &f.release.source,
        ],
    )?;
    crate::git(repo, &["add", "."])?;
    crate::git(
        repo,
        &["commit", "--quiet", "-m", "record fixture release inputs"],
    )?;
    let source = crate::git(repo, &["rev-parse", "HEAD"])?;
    let release = Release::from_commit(repo, source.trim(), &f.release.target)?;
    let checkout = Checkout::prepare(repo, &release, state)?;
    let target = checkout.output()?;
    fs::create_dir(target.join("release"))?;
    for name in product::BINARIES {
        fs::write(target.join("release").join(name), name.as_bytes())?;
    }
    let attempt = checkout.campaign.join("ripgrep-fixture");
    let rg_target = attempt.join("target.noindex");
    let rg_bin = attempt.join("install.noindex/bin");
    fs::create_dir_all(&rg_target)?;
    fs::create_dir_all(&rg_bin)?;
    let rg = rg_bin.join("rg");
    fs::write(&rg, b"fixture ripgrep")?;
    let sdk = state.canonicalize()?.join("test-compiler");
    fs::create_dir_all(sdk.join("bin"))?;
    fs::create_dir_all(sdk.join("lib"))?;
    fs::write(sdk.join("bin/trustc"), b"test compiler bytes")?;
    fs::write(
        sdk.join("lib/librustc_driver-fixture.dylib"),
        b"test driver",
    )?;
    let vv = "rustc 1.99.0-dev\nbinary: trustc\n";
    let sysroot = sdk.to_str().ok_or("non-UTF-8 fixture root")?;
    let info = serde_json::json!({"outputs":{"version":{"stdout":vv},"sysroot":{"stdout":format!("{sysroot}\n")}}});
    for path in [&target, &rg_target] {
        fs::write(path.join(".rustc_info.json"), serde_json::to_vec(&info)?)?;
    }
    let report = product::Certification::fresh(&target)?;
    fs::create_dir(&report)?;
    fs::write(report.join("report.json"), b"fixture report")?;
    let compiler = product::observe_compiler(&target, vv, sysroot)?;
    let product = product::Build {
        intent: release.id()?,
        source: release.source.clone(),
        version: release.version.clone(),
        upstream: product::upstream(repo, &release)?,
        binaries: product::binaries(&target)?,
        certification: product::Certification::observe(&target, &report)?,
        target,
        compiler: compiler.clone(),
    };
    let aux = crate::ripgrep::Build {
        intent: release.id()?,
        version: ripgrep_version(repo, &release)?,
        sha256: product::file_hash(&rg)?,
        binary: rg.clone(),
        target: rg_target,
        compiler,
    };
    let ready = Ready {
        product: observation(&checkout.campaign, "build", &product)?,
        ripgrep: observation(&checkout.campaign, "ripgrep", &aux)?,
    };
    drop(checkout);
    let check = || ready.check_outputs(repo, &release, state);
    check()?;
    fs::write(&rg, b"changed ripgrep")?;
    assert!(check().is_err());
    fs::write(&rg, b"fixture ripgrep")?;
    let outside = state.join("copied-install-bin");
    fs::rename(&rg_bin, &outside)?;
    std::os::unix::fs::symlink(&outside, &rg_bin)?;
    assert!(check().is_err());
    fs::remove_file(&rg_bin)?;
    fs::rename(&outside, &rg_bin)?;
    check()?;
    fs::write(report.join("report.json"), b"changed proof report")?;
    assert!(check().is_err());
    Ok(())
}
