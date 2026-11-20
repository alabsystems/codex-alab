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
use std::os::unix::fs::PermissionsExt;
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

fn baseline_commit(repo: &Path, release: &Release) -> Result<String> {
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
    Ok(baseline.to_owned())
}

/// A release worker starts with an origin-only clone. Acquire the official inputs in
/// the outer process so the protected compiler and existing assembly checks stay local.
pub fn fetch_upstream(repo: &Path, release: &Release) -> Result<Upstream> {
    let baseline = baseline_commit(repo, release)?;
    let scratch = tempfile::Builder::new()
        .prefix("codex-release-")
        .tempdir()?;
    let nonce = scratch
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid fetch directory")?;
    let main_ref = format!("refs/agh/{nonce}/main");
    let tag_ref = format!("refs/agh/{nonce}/baseline");
    let result = (|| {
        for (source, destination) in [
            (format!("refs/tags/rust-v{}", release.baseline), &tag_ref),
            ("refs/heads/main".to_owned(), &main_ref),
        ] {
            let status = std::process::Command::new("agh")
                .args(["repo", "fetch", "openai/codex"])
                .arg(repo)
                .args(["--ref", &source, "--into", destination, "--anonymous"])
                .status()?;
            if !status.success() {
                return Err(
                    format!("Agh could not fetch official Codex {source}: {status}").into(),
                );
            }
        }
        let official = admit_upstream(repo, &main_ref, &tag_ref, &baseline)?;
        upstream_at(repo, release, &baseline, &official)
    })();
    for temporary in [&main_ref, &tag_ref] {
        crate::git(repo, &["update-ref", "--no-deref", "-d", temporary])?;
    }
    result
}

fn admit_upstream(repo: &Path, main_ref: &str, tag_ref: &str, baseline: &str) -> Result<String> {
    let tag_commit = crate::git(
        repo,
        &["rev-parse", "--verify", &format!("{tag_ref}^{{commit}}")],
    )?;
    if tag_commit.trim() != baseline {
        return Err("official Codex release tag differs from the committed baseline".into());
    }
    let tip = crate::git(repo, &["rev-parse", "--verify", main_ref])?;
    let tip = tip.trim();
    if crate::git(repo, &["cat-file", "-t", tip])?.trim() != "commit" {
        return Err("official Codex main did not name a commit".into());
    }
    let tracking = "refs/remotes/upstream/main";
    let current = crate::git(
        repo,
        &["for-each-ref", "--format=%(objectname)%(symref)", tracking],
    )?;
    let current = current.trim();
    let expected = if current.is_empty() {
        "0000000000000000000000000000000000000000"
    } else {
        if !crate::source_id(current) {
            return Err("invalid upstream tracking reference".into());
        }
        crate::git(repo, &["merge-base", "--is-ancestor", current, tip])?;
        current
    };
    // A stable tag can live on a release-only branch. Keep its verified commit
    // reachable after the temporary Agh refs are released and the worker runs GC.
    crate::git(
        repo,
        &[
            "update-ref",
            "--no-deref",
            &format!("refs/agh/codex-release-baselines/{baseline}"),
            baseline,
        ],
    )?;
    crate::git(repo, &["update-ref", "--no-deref", tracking, tip, expected])?;
    Ok(tip.to_owned())
}

pub fn upstream(repo: &Path, release: &Release) -> Result<Upstream> {
    let baseline = baseline_commit(repo, release)?;
    let tip = crate::git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "refs/remotes/upstream/main^{commit}",
        ],
    )?;
    upstream_at(repo, release, &baseline, tip.trim())
}

fn upstream_at(repo: &Path, release: &Release, baseline: &str, tip: &str) -> Result<Upstream> {
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
}

impl Inputs {
    pub fn new(
        release: &Release,
        checkout: &crate::source::Checkout,
        v8: &crate::v8::Artifacts,
    ) -> Result<Self> {
        let (archive, binding) = v8.hashes();
        Ok(Self {
            intent: release.id()?,
            campaign: checkout.campaign.clone(),
            source: checkout.root.clone(),
            target: checkout.output()?,
            v8_archive: v8.archive.clone(),
            v8_binding: v8.binding.clone(),
            v8_archive_sha256: archive.to_owned(),
            v8_binding_sha256: binding.to_owned(),
        })
    }

    pub fn check(&self, repo: &Path, release: &Release) -> Result<()> {
        if self.intent != release.id()?
            || self.source != crate::source::Checkout::recorded(repo, release, &self.campaign)?
            || self.target != self.campaign.join("build.noindex")
            || file_hash(&self.v8_archive)? != self.v8_archive_sha256
            || file_hash(&self.v8_binding)? != self.v8_binding_sha256
        {
            return Err("canonical product inputs changed".into());
        }
        Ok(())
    }
}

/// Arguments to the source checkout's audited certified-build.sh. Its own target
/// selector supplies the admitted Trust compiler, locked policy and report boundary.
pub fn certified_arguments(source: &str, report: &Path) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        report.as_os_str().to_owned(),
        source.into(),
        "--release".into(),
        "--locked".into(),
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

/// Private observation from a completed live certified compilation. An observation never
/// selects the proof lane or substitutes for executing its compiler.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Certification {
    pub report: PathBuf,
    pub sha256: String,
}

impl Certification {
    /// Allocate a new attempt before entering the compiler. The shell boundary
    /// itself creates the report directory and refuses any preexisting one.
    pub fn fresh(target: &Path) -> Result<PathBuf> {
        target_root(target)?;
        Ok(tempfile::Builder::new()
            .prefix("certified-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(target)?
            .keep()
            .join("report"))
    }

    fn location(target: &Path, directory: &Path) -> Result<()> {
        let attempt = directory.parent().ok_or("missing private proof attempt")?;
        if attempt.parent() != Some(target)
            || directory.file_name() != Some(std::ffi::OsStr::new("report"))
            || !attempt
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("certified-"))
            || std::fs::canonicalize(attempt)? != attempt
            || std::fs::metadata(attempt)?.mode() & 0o077 != 0
        {
            return Err("certified report is outside its private build attempt".into());
        }
        Ok(())
    }

    pub fn check_fresh(target: &Path, directory: &Path) -> Result<()> {
        Self::location(target, directory)?;
        match std::fs::symlink_metadata(directory) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
            Ok(_) => Err("certified build requires a fresh report directory".into()),
        }
    }

    pub fn observe(target: &Path, directory: &Path) -> Result<Self> {
        Self::location(target, directory)?;
        if std::fs::canonicalize(directory)? != directory {
            return Err("linked proof report directory".into());
        }
        let report = directory.join("report.json");
        let sha256 = file_hash(&report)?;
        Ok(Self { report, sha256 })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub intent: String,
    pub source: String,
    pub version: String,
    pub target: PathBuf,
    pub compiler: Compiler,
    pub certification: Certification,
    pub binaries: std::collections::BTreeMap<String, String>,
    pub upstream: Upstream,
}

impl Build {
    pub fn from_observation(
        bytes: &[u8],
        repo: &Path,
        release: &Release,
        checkout: &crate::source::Checkout,
    ) -> Result<Self> {
        let build: Self = serde_json::from_slice(bytes)?;
        if build.target != checkout.campaign.join(crate::source::BUILD_DIR)
            || build.upstream != upstream(repo, release)?
        {
            return Err("product observation belongs to another campaign or upstream".into());
        }
        build.check(release)?;
        Ok(build)
    }

    pub fn check(&self, release: &Release) -> Result<()> {
        if self.intent != release.id()?
            || self.source != release.source
            || self.version != release.version
            || self.binaries != binaries(&self.target)?
            || self.certification
                != Certification::observe(
                    &self.target,
                    self.certification
                        .report
                        .parent()
                        .ok_or("certification has no report directory")?,
                )?
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
    fn origin_only_release_uses_the_official_tag_branch_and_integrated_main() -> Result<()> {
        let fixture = crate::workflow::tests::fixture()?;
        let repo = &fixture.repo;
        let base = &fixture.release.source;
        let tag = "refs/agh/test/baseline";
        let main = "refs/agh/test/main";
        let tracking = "refs/remotes/upstream/main";
        crate::git(
            repo,
            &[
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "release-only commit",
            ],
        )?;
        let baseline = crate::git(repo, &["rev-parse", "HEAD"])?;
        let baseline = baseline.trim();
        crate::git(
            repo,
            &["tag", "-a", "stable", "-m", "official annotated tag"],
        )?;
        crate::git(repo, &["update-ref", tag, "refs/tags/stable"])?;
        crate::git(repo, &["checkout", "--quiet", "--detach", base])?;
        crate::git(
            repo,
            &["commit", "--quiet", "--allow-empty", "-m", "official main"],
        )?;
        let official = crate::git(repo, &["rev-parse", "HEAD"])?;
        let official = official.trim();
        crate::git(repo, &["update-ref", main, official])?;
        let ledger = repo.join("alab/versions.toml");
        let mut row = std::fs::read_to_string(&ledger)?;
        row.push_str(&format!("baseline_commit = '{baseline}'\n"));
        std::fs::write(ledger, row)?;
        crate::git(
            repo,
            &["commit", "--quiet", "-am", "select release baseline"],
        )?;
        let source = crate::git(repo, &["rev-parse", "HEAD"])?;
        let release = Release::from_commit(repo, source.trim(), &fixture.release.target)?;
        assert!(upstream(repo, &release).is_err());
        assert!(admit_upstream(repo, main, tag, base).is_err());
        assert!(crate::git(repo, &["rev-parse", "--verify", tracking]).is_err());
        let observed = admit_upstream(repo, main, tag, baseline)?;
        let actual = upstream_at(repo, &release, baseline, &observed)?;
        assert_eq!(upstream(repo, &release)?, actual);
        assert_eq!(actual.commit, official);
        assert_eq!(actual.commits_since_release, 1);
        admit_upstream(repo, main, tag, baseline)?;
        crate::git(repo, &["update-ref", main, base])?;
        assert!(admit_upstream(repo, main, tag, baseline).is_err());
        assert_eq!(crate::git(repo, &["rev-parse", tracking])?.trim(), official);
        crate::git(repo, &["update-ref", tracking, source.trim()])?;
        assert_eq!(upstream_at(repo, &release, baseline, &observed)?, actual);
        assert_ne!(upstream(repo, &release)?, actual);
        crate::git(repo, &["symbolic-ref", tracking, "refs/heads/retained"])?;
        crate::git(repo, &["update-ref", "refs/heads/retained", base])?;
        assert!(admit_upstream(repo, main, tag, baseline).is_err());
        assert_eq!(
            crate::git(repo, &["rev-parse", "refs/heads/retained"])?.trim(),
            base
        );
        Ok(())
    }

    #[test]
    fn certification_uses_the_shared_driver_for_every_shipping_binary() {
        let dir = Path::new("/proof/report");
        let args: Vec<_> = certified_arguments("frozen", dir)
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "/proof/report",
                "frozen",
                "--release",
                "--locked",
                "-p",
                "codex-cli",
                "--bin",
                "codex",
                "-p",
                "codex-code-mode-host",
                "--bin",
                "codex-code-mode-host",
                "-p",
                "codex-exec-server-cli",
                "--bin",
                "codex-exec-server"
            ]
        );
    }

    #[test]
    fn an_attempt_records_the_private_live_report_and_detects_replacement() -> Result<()> {
        let root = tempfile::tempdir()?;
        let target = std::fs::canonicalize(root.path())?;
        let dir = Certification::fresh(&target)?;
        Certification::check_fresh(&target, &dir)?;
        std::fs::create_dir(&dir)?;
        assert!(Certification::check_fresh(&target, &dir).is_err());
        assert!(Certification::observe(&target, &dir).is_err());
        let path = dir.join("report.json");
        std::fs::write(&path, b"first")?;
        let first = Certification::observe(&target, &dir)?;
        std::fs::write(&path, b"second")?;
        assert_ne!(Certification::observe(&target, &dir)?, first);
        std::fs::remove_file(&path)?;
        std::os::unix::fs::symlink("/dev/null", &path)?;
        assert!(Certification::observe(&target, &dir).is_err());
        assert!(Certification::check_fresh(&target, &target.join("other/report")).is_err());
        Ok(())
    }

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
