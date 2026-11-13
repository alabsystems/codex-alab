// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Canonical product inputs and local build observations. These private records
//! are not package payload, qualification receipts or publication authority.

use crate::Release;
use crate::Result;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

pub const BINARIES: [&str; 3] = ["codex", "codex-code-mode-host", "codex-exec-server"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    pub release: String,
    pub commit: String,
    pub commits_since_release: u64,
    pub commit_date: String,
}

pub fn upstream(repo: &Path, release: &Release) -> Result<Upstream> {
    let ledger: toml::Value = toml::from_str(&crate::git(
        repo,
        &[
            "cat-file",
            "blob",
            &format!("{}:alab/versions.toml", release.source),
        ],
    )?)?;
    let row = ledger
        .get("release")
        .and_then(toml::Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("version").and_then(toml::Value::as_str) == Some(release.version.as_str())
            })
        })
        .ok_or("source has no release baseline")?;
    let baseline = row
        .get("baseline_commit")
        .and_then(toml::Value::as_str)
        .filter(|value| crate::source_id(value))
        .ok_or("source has no valid baseline commit")?;
    let tip = crate::git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "refs/remotes/upstream/main^{commit}",
        ],
    )?;
    let tip = tip.trim();
    let commit = crate::git(repo, &["merge-base", &release.source, tip])?
        .trim()
        .to_owned();
    let branch = crate::git(repo, &["merge-base", baseline, &commit])?;
    crate::git(
        repo,
        &["merge-base", "--is-ancestor", branch.trim(), &commit],
    )?;
    let count = crate::git(
        repo,
        &[
            "rev-list",
            "--count",
            &format!("{}..{commit}", branch.trim()),
        ],
    )?
    .trim()
    .parse()?;
    let date = crate::git(
        repo,
        &[
            "show",
            "-s",
            "--date=format-local:%Y-%m-%dT%H:%M:%SZ",
            "--format=%cd",
            &commit,
        ],
    )?
    .trim()
    .to_owned();
    Ok(Upstream {
        release: format!("rust-v{}", release.baseline),
        commit,
        commits_since_release: count,
        commit_date: date,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    pub intent: String,
    pub campaign: PathBuf,
    pub source: PathBuf,
    pub target: PathBuf,
    pub v8_archive: PathBuf,
    pub v8_binding: PathBuf,
    pub v8_archive_sha256: String,
    pub v8_binding_sha256: String,
    pub upstream: Upstream,
}

impl Inputs {
    pub fn check(&self, repo: &Path, release: &Release) -> Result<()> {
        if self.intent != release.id()?
            || self.source != crate::source::Checkout::recorded(repo, release, &self.campaign)?
            || self.target != self.campaign.join("build.noindex")
            || self.upstream != upstream(repo, release)?
            || file_hash(&self.v8_archive)? != self.v8_archive_sha256
            || file_hash(&self.v8_binding)? != self.v8_binding_sha256
        {
            return Err("canonical product inputs changed".into());
        }
        Ok(())
    }

    pub fn arguments(&self) -> Vec<std::ffi::OsString> {
        let mut args = vec![
            "--unverified".into(),
            "build".into(),
            "--release".into(),
            "--locked".into(),
            "--manifest-path".into(),
            self.source.join("codex-rs/Cargo.toml").into_os_string(),
            "--target-dir".into(),
            self.target.clone().into_os_string(),
        ];
        for (package, binary) in [
            ("codex-cli", BINARIES[0]),
            ("codex-code-mode-host", BINARIES[1]),
            ("codex-exec-server-cli", BINARIES[2]),
        ] {
            args.extend(["-p".into(), package.into(), "--bin".into(), binary.into()]);
        }
        args
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compiler {
    pub rustc: String,
    #[serde(rename = "rustc_vV")]
    pub rustc_vv: String,
    pub sysroot: String,
    pub binaries_sha256: String,
    pub id: String,
    pub admitted: bool,
}

pub fn file_hash(path: &Path) -> Result<String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    hash_file(file)
}

fn hash_file(mut file: File) -> Result<String> {
    let before = file.metadata()?;
    if !before.is_file() || before.len() == 0 || before.mode() & 0o7022 != 0 {
        return Err("build observation needs a nonempty protected regular file".into());
    }
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    let identity = |info: &std::fs::Metadata| {
        (
            info.dev(),
            info.ino(),
            info.len(),
            info.mode(),
            info.mtime(),
            info.mtime_nsec(),
            info.ctime(),
            info.ctime_nsec(),
        )
    };
    if identity(&before) != identity(&after) {
        return Err("build output changed while hashing".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn open_at(parent: &File, name: &str, directory: bool) -> Result<File> {
    let name = std::ffi::CString::new(name)?;
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: parent is held open and name is a live NUL-terminated single component.
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: openat returned a fresh descriptor, whose sole owner is this File.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn target_root(path: &Path) -> Result<File> {
    Ok(std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?)
}

pub fn binaries(target: &Path) -> Result<std::collections::BTreeMap<String, String>> {
    let root = target_root(target)?;
    let release = open_at(&root, "release", true)?;
    BINARIES
        .iter()
        .map(|name| {
            Ok((
                (*name).to_owned(),
                hash_file(open_at(&release, name, false)?)?,
            ))
        })
        .collect()
}

pub fn observe_compiler(target: &Path, vv: &str, sysroot: &str) -> Result<Compiler> {
    if !vv.starts_with("rustc ") || !vv.lines().any(|line| line == "binary: trustc") {
        return Err("native compiler observation is not Trust".into());
    }
    let root = target_root(target)?;
    let mut text = Vec::new();
    open_at(&root, ".rustc_info.json", false)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut text)?;
    if text.len() > 8 * 1024 * 1024 {
        return Err("compiler observation exceeds its byte limit".into());
    }
    let info: serde_json::Value = serde_json::from_slice(&text)?;
    let outputs = info
        .get("outputs")
        .and_then(serde_json::Value::as_object)
        .ok_or("missing target compiler observation")?;
    let mut observed = false;
    for output in outputs.values() {
        if let Some(text) = output.get("stdout").and_then(serde_json::Value::as_str) {
            if text.starts_with("rustc ") && text.contains("\nbinary: ") {
                if text != vv {
                    return Err("build used another compiler version".into());
                }
                observed = true;
            }
            for line in text.lines().filter(|line| line.starts_with('/')) {
                if line != sysroot {
                    return Err("build used another compiler sysroot".into());
                }
            }
        }
    }
    if !observed {
        return Err("build recorded no compiler identity".into());
    }
    let root = Path::new(sysroot);
    let mut paths = vec![root.join("bin/trustc")];
    let mut libraries: Vec<_> = std::fs::read_dir(root.join("lib"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    libraries.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("librustc_driver-") && name.ends_with(".dylib"))
    });
    libraries.sort();
    paths.extend(libraries);
    let mut digest = Sha256::new();
    for path in paths {
        let bytes = file_hash(&path)?;
        digest.update(
            path.file_name()
                .ok_or("compiler input has no name")?
                .as_encoded_bytes(),
        );
        digest.update([0]);
        for pair in bytes.as_bytes().as_chunks::<2>().0 {
            let byte = u8::from_str_radix(std::str::from_utf8(pair)?, 16)?;
            digest.update([byte]);
        }
    }
    let binaries_sha256 = format!("{:x}", digest.finalize());
    let id = format!(
        "{:x}",
        Sha256::digest(format!("{vv}\0{sysroot}\0{binaries_sha256}").as_bytes())
    );
    Ok(Compiler {
        rustc: vv
            .lines()
            .next()
            .ok_or("empty compiler version")?
            .to_owned(),
        rustc_vv: vv.to_owned(),
        sysroot: sysroot.to_owned(),
        binaries_sha256,
        id: id[..16].to_owned(),
        admitted: true,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub intent: String,
    pub source: String,
    pub version: String,
    pub target: PathBuf,
    pub compiler: Compiler,
    pub binaries: std::collections::BTreeMap<String, String>,
    pub upstream: Upstream,
}

impl Build {
    pub fn check(&self, release: &Release) -> Result<()> {
        if self.intent != release.id()?
            || self.source != release.source
            || self.version != release.version
            || self.binaries != binaries(&self.target)?
        {
            return Err("canonical build outputs do not match their observation".into());
        }
        let compiler = observe_compiler(
            &self.target,
            &self.compiler.rustc_vv,
            &self.compiler.sysroot,
        )?;
        if compiler != self.compiler {
            return Err("build compiler observation changed".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_observation_matches_native_publisher_digest_and_refuses_drift() -> Result<()> {
        let root = tempfile::tempdir()?;
        let sdk = root.path().join("sdk");
        let target = root.path().join("target");
        for path in [sdk.join("bin"), sdk.join("lib"), target.clone()] {
            std::fs::create_dir_all(path)?;
        }
        for (path, bytes) in [
            ("bin/trustc", b"trustc-bytes".as_slice()),
            ("lib/librustc_driver-a.dylib", b"d1".as_slice()),
            ("lib/librustc_driver-b.dylib", b"d2".as_slice()),
            ("lib/librustc_driver-c.so", b"ignored".as_slice()),
        ] {
            std::fs::write(sdk.join(path), bytes)?;
        }
        let vv = "rustc 1.99.0-dev (trustc 0.1.0)\nbinary: trustc\n";
        let sysroot = sdk.to_str().ok_or("fixture path is not UTF-8")?;
        let info = serde_json::json!({"outputs": {"version": {"stdout": vv}, "sysroot": {"stdout": format!("{sysroot}\n")}}});
        std::fs::write(target.join(".rustc_info.json"), serde_json::to_vec(&info)?)?;
        let observed = observe_compiler(&target, vv, sysroot)?;
        assert_eq!(
            observed.binaries_sha256,
            "4ea994d24d878bc8adef5e0e4ad3b70da66e08cc8d0b77db1c424dc88eb67054"
        );
        assert!(observe_compiler(&target, &vv.replace("1.99.0", "1.98.0"), sysroot).is_err());
        assert!(observe_compiler(&target, vv, "/different/compiler").is_err());
        std::fs::remove_file(target.join(".rustc_info.json"))?;
        let elsewhere = root.path().join("elsewhere");
        std::fs::write(&elsewhere, serde_json::to_vec(&info)?)?;
        std::os::unix::fs::symlink(elsewhere, target.join(".rustc_info.json"))?;
        assert!(observe_compiler(&target, vv, sysroot).is_err());
        Ok(())
    }

    #[test]
    fn product_hashes_refuse_symlinked_binaries_and_parent_directories() -> Result<()> {
        let root = tempfile::tempdir()?;
        let target = root.path().join("target");
        let release = target.join("release");
        std::fs::create_dir_all(&release)?;
        for binary in BINARIES {
            std::fs::write(release.join(binary), binary)?;
        }
        assert_eq!(binaries(&target)?.len(), 3);
        std::fs::remove_file(release.join("codex"))?;
        std::os::unix::fs::symlink("codex-code-mode-host", release.join("codex"))?;
        assert!(binaries(&target).is_err());
        std::fs::remove_file(release.join("codex"))?;
        std::fs::write(release.join("codex"), "codex")?;
        std::fs::rename(&release, target.join("elsewhere"))?;
        std::os::unix::fs::symlink("elsewhere", &release)?;
        assert!(binaries(&target).is_err());
        Ok(())
    }
}
