// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) struct Fixture {
    _root: tempfile::TempDir,
    pub(crate) repo: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) publisher: PathBuf,
    pub(crate) release: Release,
}

pub(crate) fn fixture() -> Result<Fixture> {
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
fn denied_fresh_publication_creates_no_campaign_or_git_registration() -> Result<()> {
    let f = fixture()?;
    let list = || crate::git(&f.repo, &["worktree", "list", "--porcelain"]);
    let before = list()?;
    assert!(
        Campaign::open_with_source_admission(
            &f.repo,
            &f.release,
            &f.state,
            2026111202,
            &f.publisher,
            || Err("insufficient disk".into()),
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(&f.state)?.count(), 0);
    assert_eq!(list()?, before);
    Ok(())
}

#[test]
fn caller_loss_prevents_new_publication_steps() -> Result<()> {
    let f = fixture()?;
    let calls = f.publisher.with_extension("calls");
    std::fs::write(
        &f.publisher,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n",
    )?;
    let campaign = Campaign::open(&f.repo, &f.release, &f.state, 2026111202, &f.publisher)?;
    let run = |already_gone: bool| {
        campaign.publish(
            &f.repo,
            &f.release,
            || {
                if already_gone || calls.exists() {
                    return Err("release owner closed".into());
                }
                Ok(())
            },
            |_, _, _| panic!("cancelled publication started a reader"),
        )
    };
    assert_eq!(run(true).unwrap_err().to_string(), "release owner closed");
    assert!(!calls.exists());
    assert_eq!(run(false).unwrap_err().to_string(), "release owner closed");
    let calls = std::fs::read_to_string(calls)?;
    assert_eq!(calls.lines().count(), 1);
    assert!(calls.trim_end().ends_with("--prepare-codex-protocol"));
    Ok(())
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
    let reopened = Campaign::open_with_source_admission(
        &f.repo,
        &f.release,
        &f.state,
        2026111202,
        &f.publisher,
        || Err("a retained checkout must not request new build space".into()),
    )?;
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

#[test]
fn signed_pair_must_name_the_assembled_bytes_and_coherent_predecessor() -> Result<()> {
    let f = fixture()?;
    let hash = product::file_hash(&f.publisher)?;
    let endpoint = |version: &str, build: u64, source: &str| {
        serde_json::json!({
            "version": version, "build": build, "source": source, "executable_sha256": hash,
            "manifest_sha256":"1".repeat(64), "tree_root":"2".repeat(64)
        })
    };
    let new = endpoint(&f.release.version, 8, &f.release.source);
    let old = endpoint("0.5.0", 7, &"a".repeat(40));
    let wire = |relation: &str, previous: serde_json::Value| -> Result<Vec<u8>> {
        Ok(serde_json::to_vec_pretty(&serde_json::json!({
            "schema":1, "target":f.release.target, "successor":new, "predecessor":previous,
            "relation":relation, "candidate":f.publisher.canonicalize()?
        }))?)
    };
    let input = |relation: &str, previous| -> Result<ProtocolInput> {
        Ok(serde_json::from_slice(&wire(relation, previous)?)?)
    };
    let upgrade = wire("upgrade", old.clone())?;
    assert!(
        ProtocolInput::qualify(&upgrade, 8, &f.release, &f.publisher, |_, _, _| Err(
            "old reader rejected candidate".into()
        ))
        .is_err()
    );
    let authorized = ProtocolInput::qualify(
        &upgrade,
        8,
        &f.release,
        &f.publisher,
        |previous, successor, path| {
            assert_eq!(previous.build, 7);
            assert_eq!(successor.build, 8);
            assert_eq!(path, f.publisher.canonicalize()?);
            Ok(())
        },
    )?;
    use sha2::Digest;
    use sha2::Sha256;
    assert_eq!(authorized, format!("{:x}", Sha256::digest(&upgrade)));
    let no_pair = wire("first", serde_json::Value::Null)?;
    ProtocolInput::qualify(&no_pair, 8, &f.release, &f.publisher, |_, _, _| {
        panic!("first publication has no previous reader")
    })?;
    let check = |pair: ProtocolInput| pair.check(8, &f.release, &f.publisher);
    check(input("first", serde_json::Value::Null)?)?;
    check(input("already-selected", new.clone())?)?;
    check(input("upgrade", old.clone())?)?;
    assert!(check(input("first", old.clone())?).is_err());
    assert!(check(input("already-selected", old.clone())?).is_err());
    assert!(check(input("upgrade", serde_json::Value::Null)?).is_err());
    assert!(check(input("upgrade", new.clone())?).is_err());
    let mut invalid_old = old.clone();
    invalid_old["source"] = "garbage".into();
    assert!(check(input("upgrade", invalid_old)?).is_err());
    let mut forged = input("upgrade", old.clone())?;
    forged.successor.executable_sha256 = "a".repeat(64);
    assert!(check(forged).is_err());
    let different = f.state.join("different");
    std::fs::write(&different, b"different candidate")?;
    assert!(
        input("upgrade", old)?
            .check(8, &f.release, &different)
            .is_err()
    );
    Ok(())
}
