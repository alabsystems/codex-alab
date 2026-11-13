// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A frozen product checkout, retained across compiler-group lifetime. Reuse
//! requires the same intent, Git registration, directory identity and clean
//! tracked/untracked contents. No checkout is reset or removed on failure.

use crate::Release;
use crate::Result;
use serde::Deserialize;
use serde::Serialize;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema: u32,
    intent: String,
    source: String,
    common: PathBuf,
    identity: [u64; 2],
}

pub struct Checkout {
    pub root: PathBuf,
    pub campaign: PathBuf,
    repo: PathBuf,
    binding: Binding,
    _lease: Option<File>,
}

fn owned(path: &Path) -> Result<std::fs::Metadata> {
    let info = std::fs::symlink_metadata(path)?;
    // SAFETY: geteuid has no preconditions.
    if !info.is_dir() || info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o022 != 0 {
        return Err(
            "release source state needs an owned real directory without shared writes".into(),
        );
    }
    Ok(info)
}

fn make_owned(path: &Path) -> Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    owned(path)?;
    Ok(())
}

fn common(repo: &Path) -> Result<PathBuf> {
    Ok(std::fs::canonicalize(
        crate::git(
            repo,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?
        .trim(),
    )?)
}

impl Checkout {
    pub fn prepare(repo: &Path, release: &Release, state: &Path) -> Result<Self> {
        release.check_local(repo)?;
        owned(state)?;
        let state = std::fs::canonicalize(state)?;
        let repo = std::fs::canonicalize(repo)?;
        let git_common = common(&repo)?;
        let home = std::env::var_os("HOME").ok_or("HOME is unset")?;
        let home = std::fs::canonicalize(home)?;
        if home.starts_with(&state)
            || repo.starts_with(&state)
            || state.starts_with(&repo)
            || state.starts_with(&git_common)
            || git_common.starts_with(&state)
        {
            return Err("release source state overlaps a home or driving repository".into());
        }
        let intent = release.id()?;
        let campaign = state.join(&intent);
        make_owned(&campaign)?;
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(campaign.join(".lease"))?;
        let info = lease.metadata()?;
        // SAFETY: geteuid has no preconditions.
        if !info.is_file() || info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o022 != 0 {
            return Err("invalid release source lease".into());
        }
        // SAFETY: lease owns a live descriptor. Its lifetime spans native group drain.
        if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let root = campaign.join("source.noindex");
        let reason = format!("native-release:{intent}");
        match std::fs::symlink_metadata(&root) {
            Ok(_) => {
                owned(&root)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                crate::git(
                    &repo,
                    &[
                        "worktree",
                        "add",
                        "--detach",
                        "--lock",
                        "--reason",
                        &reason,
                        root.to_str().ok_or("source path is not UTF-8")?,
                        &release.source,
                    ],
                )?;
            }
            Err(error) => return Err(error.into()),
        }
        let info = owned(&root)?;
        let binding = Binding {
            schema: 1,
            intent,
            source: release.source.clone(),
            common: git_common,
            identity: [info.dev(), info.ino()],
        };
        let checkout = Self {
            root,
            campaign,
            repo,
            binding,
            _lease: Some(lease),
        };
        checkout.check(release)?;
        // An interrupted initial creation can recover only through all actual
        // Git/directory/content checks above; a leftover file alone proves none.
        crate::save_record(
            &checkout.campaign.join("source.json"),
            &serde_json::to_vec_pretty(&checkout.binding)?,
        )?;
        Ok(checkout)
    }

    /// The worker rechecks the parent's recorded snapshot without acquiring a
    /// second lease. This is content admission, not evidence of a live owner.
    pub fn recorded(repo: &Path, release: &Release, campaign: &Path) -> Result<PathBuf> {
        owned(campaign)?;
        let binding: Binding =
            serde_json::from_slice(&crate::record_bytes(&campaign.join("source.json"))?)?;
        let checkout = Self {
            root: campaign.join("source.noindex"),
            campaign: campaign.to_path_buf(),
            repo: repo.to_path_buf(),
            binding,
            _lease: None,
        };
        checkout.check(release)?;
        Ok(checkout.root)
    }

    pub fn output(&self) -> Result<PathBuf> {
        let output = self.campaign.join("build.noindex");
        make_owned(&output)?;
        Ok(output)
    }

    pub fn check(&self, release: &Release) -> Result<()> {
        let info = owned(&self.root)?;
        if [info.dev(), info.ino()] != self.binding.identity
            || common(&self.root)? != self.binding.common
            || release.id()? != self.binding.intent
            || release.source != self.binding.source
        {
            return Err("release source directory or intent changed".into());
        }
        let rows = crate::git(&self.repo, &["worktree", "list", "--porcelain", "-z"])?;
        let name = format!("worktree {}", self.root.display());
        let head = format!("HEAD {}", self.binding.source);
        let reason = format!("locked native-release:{}", self.binding.intent);
        let registered = rows.split("\0\0").any(|row| {
            let fields: Vec<_> = row.split('\0').collect();
            fields.contains(&name.as_str())
                && fields.contains(&head.as_str())
                && fields.contains(&"detached")
                && fields.contains(&reason.as_str())
        });
        if !registered {
            return Err("release source lacks its exact locked Git registration".into());
        }
        if !crate::git(
            &self.root,
            &[
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--ignored=matching",
            ],
        )?
        .is_empty()
        {
            return Err("release source has changed or contains extra files; preserving it".into());
        }
        crate::git(
            &self.root,
            &[
                "diff",
                "--no-ext-diff",
                "--quiet",
                &self.binding.source,
                "--",
            ],
        )?;
        release.check_local(&self.root)?;
        let record = self.campaign.join("source.json");
        match std::fs::symlink_metadata(&record) {
            Ok(_) => {
                let saved: Binding = serde_json::from_slice(&crate::record_bytes(&record)?)?;
                if saved != self.binding {
                    return Err("release source binding changed".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_bind_frozen_source_and_refuse_dirty_or_live_reuse() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let repo = temporary.path().join("repo");
        let state = temporary.path().join("state");
        std::fs::create_dir(&repo)?;
        std::fs::create_dir(&state)?;
        crate::git(&repo, &["init", "--quiet"])?;
        crate::git(&repo, &["config", "user.email", "fixture@example.invalid"])?;
        crate::git(&repo, &["config", "user.name", "release fixture"])?;
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
        for (path, text) in [
            ("VERSION", "0.5.0\n"),
            ("alab/client-version", "0.168.0\n"),
            (
                "alab/versions.toml",
                "[[release]]\nversion='0.5.0'\nbaseline='0.167.1'\n",
            ),
            ("trust.lock", "compiler fixture\n"),
            ("codex-rs/Cargo.lock", "dependencies fixture\n"),
        ] {
            std::fs::write(repo.join(path), text)?;
        }
        crate::git(&repo, &["add", "."])?;
        crate::git(&repo, &["commit", "--quiet", "-m", "source fixture"])?;
        let source = crate::git(&repo, &["rev-parse", "HEAD"])?;
        let release = Release::from_commit(&repo, source.trim(), "aarch64-apple-darwin")?;
        let first = Checkout::prepare(&repo, &release, &state)?;
        first.check(&release)?;
        assert!(Checkout::prepare(&repo, &release, &state).is_err());
        let root = first.root.clone();
        drop(first);
        let second = Checkout::prepare(&repo, &release, &state)?;
        assert_eq!(second.root, root);
        std::fs::write(root.join("VERSION"), "0.6.0\n")?;
        assert!(second.check(&release).is_err());
        drop(second);
        assert!(Checkout::prepare(&repo, &release, &state).is_err());
        assert_eq!(std::fs::read_to_string(root.join("VERSION"))?, "0.6.0\n");
        Ok(())
    }
}
