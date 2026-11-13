// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private source-controller observations for one native publication. These
//! records never replace the publisher's source, compiler or signature gates.

use crate::Release;
use crate::Result;
use crate::product;
use serde::Deserialize;
use serde::Serialize;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Inputs {
    schema: u32,
    intent: String,
    build: u64,
    publisher: PathBuf,
    publisher_sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stage {
    path: PathBuf,
    sha256: String,
}

/// An exclusively leased publication workflow. Stage paths are observations;
/// callers must also revalidate their native typed build and assembly records.
pub struct Campaign {
    root: PathBuf,
    parent: PathBuf,
    directory: [u64; 2],
    lease: File,
    lease_identity: [u64; 2],
    inputs: Inputs,
}

impl Campaign {
    pub fn open(
        repo: &Path,
        release: &Release,
        state: &Path,
        build: u64,
        publisher: &Path,
    ) -> Result<Self> {
        if build == 0 {
            return Err("publication requires an explicit nonzero package build".into());
        }
        let checkout = crate::source::Checkout::prepare(repo, release, state)?;
        let parent = checkout.campaign;
        let root = parent.join(format!("publication-{build}"));
        match std::fs::create_dir(&root) {
            Ok(()) => std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let directory = directory_identity(&root)?;
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join(".lease"))?;
        let lease_identity = file_identity(&lease.metadata()?)?;
        // SAFETY: lease owns a live descriptor and stays held for the workflow.
        if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let publisher = std::fs::canonicalize(publisher)?;
        let metadata = std::fs::metadata(&publisher)?;
        if !metadata.is_file() || metadata.mode() & 0o111 == 0 {
            return Err("the native publisher must be an executable file".into());
        }
        let inputs = Inputs {
            schema: 1,
            intent: release.id()?,
            build,
            publisher_sha256: product::file_hash(&publisher)?,
            publisher,
        };
        crate::save_record(&root.join("intent.json"), &serde_json::to_vec(&inputs)?)?;
        std::fs::File::open(&parent)?.sync_all()?;
        let campaign = Self {
            root,
            parent,
            directory,
            lease,
            lease_identity,
            inputs,
        };
        campaign.check()?;
        Ok(campaign)
    }

    pub fn check(&self) -> Result<()> {
        if directory_identity(&self.root)? != self.directory
            || file_identity(&self.lease.metadata()?)? != self.lease_identity
            || file_identity(&std::fs::symlink_metadata(self.root.join(".lease"))?)?
                != self.lease_identity
        {
            return Err("publication workflow custody changed".into());
        }
        let actual: Inputs =
            serde_json::from_slice(&crate::record_bytes(&self.root.join("intent.json"))?)?;
        if actual != self.inputs
            || product::file_hash(&self.inputs.publisher)? != self.inputs.publisher_sha256
        {
            return Err("publication inputs or native publisher changed".into());
        }
        Ok(())
    }

    pub fn stage(&self, name: &str) -> Result<Option<PathBuf>> {
        self.check()?;
        let record = self.stage_path(name)?;
        match std::fs::symlink_metadata(&record) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let bytes = crate::record_bytes(&record)?;
        let stage: Stage = serde_json::from_slice(&bytes)?;
        self.check_stage(&stage)?;
        // Finish an earlier ambiguous receipt flush without replacing its bytes.
        crate::save_record(&record, &bytes)?;
        Ok(Some(stage.path))
    }

    pub fn retain_stage(&self, name: &str, path: &Path) -> Result<()> {
        self.check()?;
        let stage = Stage {
            path: std::fs::canonicalize(path)?,
            sha256: product::file_hash(path)?,
        };
        self.check_stage(&stage)?;
        crate::save_record(&self.stage_path(name)?, &serde_json::to_vec(&stage)?)?;
        self.check()
    }

    fn check_stage(&self, stage: &Stage) -> Result<()> {
        if std::fs::canonicalize(&stage.path)? != stage.path
            || !stage.path.starts_with(&self.parent)
            || product::file_hash(&stage.path)? != stage.sha256
        {
            return Err(
                "publication stage observation changed or belongs to another campaign".into(),
            );
        }
        Ok(())
    }

    fn stage_path(&self, name: &str) -> Result<PathBuf> {
        if !matches!(name, "product" | "ripgrep") {
            return Err("unknown publication stage".into());
        }
        Ok(self.root.join(format!("{name}.json")))
    }

    pub fn package(&self) -> PathBuf {
        self.root.join("package")
    }

    /// Invoke the same explicit native publisher that the workflow froze.
    /// A zero exit is its publication report, never installation/adoption proof.
    pub fn publish(&self, repo: &Path, release: &Release) -> Result<()> {
        self.check()?;
        release.check_local(repo)?;
        let status = std::process::Command::new(&self.inputs.publisher)
            .args(["--host", "agh", "publish-package"])
            .arg(self.package())
            .args(["--program", "codex", "--build"])
            .arg(self.inputs.build.to_string())
            .arg("--source-repo")
            .arg(std::fs::canonicalize(repo)?)
            .arg("--state")
            .arg(self.root.join("native-publication"))
            .status()?;
        self.check()?;
        release.check_local(repo)?;
        if !status.success() {
            return Err(format!("native package publisher failed: {status}").into());
        }
        Ok(())
    }
}

fn directory_identity(path: &Path) -> Result<[u64; 2]> {
    let metadata = std::fs::symlink_metadata(path)?;
    // SAFETY: geteuid has no preconditions.
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err("publication workflow needs a private owned real directory".into());
    }
    Ok([metadata.dev(), metadata.ino()])
}

fn file_identity(metadata: &std::fs::Metadata) -> Result<[u64; 2]> {
    // SAFETY: geteuid has no preconditions.
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err("publication workflow needs a private owned single-link lease".into());
    }
    Ok([metadata.dev(), metadata.ino()])
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;
