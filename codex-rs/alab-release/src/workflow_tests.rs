// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
    publisher: PathBuf,
    release: Release,
}

fn fixture() -> Result<Fixture> {
    let root = tempfile::tempdir()?;
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir(&repo)?;
    std::fs::create_dir(&state)?;
    crate::git(&repo, &["init", "--quiet"])?;
    crate::git(&repo, &["config", "user.name", "release fixture"])?;
    crate::git(&repo, &["config", "user.email", "release@example.invalid"])?;
    crate::git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/source-fixture/codex-alab.git",
        ],
    )?;
    std::fs::create_dir(repo.join("alab"))?;
    std::fs::create_dir(repo.join("codex-rs"))?;
    for (path, bytes) in [
        ("VERSION", "0.6.0\n"),
        ("alab/client-version", "0.168.0\n"),
        (
            "alab/versions.toml",
            "[[release]]\nversion = '0.6.0'\nbaseline = '0.167.1'\n",
        ),
        ("trust.lock", "compiler fixture\n"),
        ("codex-rs/Cargo.lock", "dependency fixture\n"),
    ] {
        std::fs::write(repo.join(path), bytes)?;
    }
    crate::git(&repo, &["add", "."])?;
    crate::git(&repo, &["commit", "--quiet", "-m", "fixture"])?;
    let source = crate::git(&repo, &["rev-parse", "HEAD"])?;
    let release = Release::from_commit(&repo, source.trim(), "aarch64-apple-darwin")?;
    let publisher = root.path().join("publisher");
    std::fs::copy("/usr/bin/true", &publisher)?;
    std::fs::set_permissions(&publisher, std::fs::Permissions::from_mode(0o755))?;
    Ok(Fixture {
        _root: root,
        repo,
        state,
        publisher,
        release,
    })
}

#[test]
fn workflow_reopens_exact_observations_and_refuses_changed_or_foreign_stage_bytes() -> Result<()> {
    let f = fixture()?;
    let campaign = Campaign::open(&f.repo, &f.release, &f.state, 2026111202, &f.publisher)?;
    assert!(campaign.stage("product")?.is_none());
    let record = campaign.parent.join("observed-build.json");
    crate::save_record(&record, b"observed native build")?;
    campaign.retain_stage("product", &record)?;
    drop(campaign);
    let reopened = Campaign::open(&f.repo, &f.release, &f.state, 2026111202, &f.publisher)?;
    assert_eq!(reopened.stage("product")?, Some(record.clone()));
    assert!(reopened.retain_stage("ripgrep", &f.publisher).is_err());
    std::fs::write(&record, b"changed build observation")?;
    assert!(reopened.stage("product").is_err());
    Ok(())
}

#[test]
fn workflow_excludes_another_writer_and_refuses_a_replaced_lease() -> Result<()> {
    let f = fixture()?;
    let campaign = Campaign::open(&f.repo, &f.release, &f.state, 2026111202, &f.publisher)?;
    assert!(Campaign::open(&f.repo, &f.release, &f.state, 2026111202, &f.publisher).is_err());
    std::fs::rename(
        campaign.root.join(".lease"),
        campaign.root.join("old-lease"),
    )?;
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(campaign.root.join(".lease"))?;
    assert!(campaign.check().is_err());
    Ok(())
}

#[test]
fn workflow_refuses_a_changed_publisher_instead_of_rebinding_an_existing_campaign() -> Result<()> {
    let f = fixture()?;
    let campaign = Campaign::open(&f.repo, &f.release, &f.state, 2026111202, &f.publisher)?;
    std::fs::write(&f.publisher, b"another executable")?;
    assert!(campaign.check().is_err());
    drop(campaign);
    assert!(Campaign::open(&f.repo, &f.release, &f.state, 2026111202, &f.publisher).is_err());
    Ok(())
}
