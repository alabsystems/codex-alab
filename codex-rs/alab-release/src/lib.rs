// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Immutable input identity for the native release controller. Product numbering is
//! independent of the upstream client version and of update ownership. Preparing a
//! release grants no publication authority and records no successful build or check.

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::fs::File;
use std::fs::Metadata;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

#[cfg(unix)]
pub mod assembly;
#[cfg(unix)]
mod lease;
#[cfg(unix)]
pub mod product;
#[cfg(unix)]
pub mod protected;
#[cfg(unix)]
pub mod ripgrep;
#[cfg(unix)]
pub mod schedule;
#[cfg(unix)]
pub mod source;
#[cfg(unix)]
pub mod space;
#[cfg(unix)]
pub mod v8;
#[cfg(unix)]
pub mod workflow;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn open_record(path: &Path, named: &Metadata) -> Result<File> {
    if !named.is_file() {
        return Err("release intent must be a regular file".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    Ok(options.open(path)?)
}

fn read_record(file: &File) -> Result<Vec<u8>> {
    use std::io::Read;
    let info = file.metadata()?;
    if !info.is_file() || info.len() > 64 * 1024 {
        return Err("release intent must be a bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no preconditions.
        if info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o022 != 0 {
            return Err("release intent must be owned and protected from shared writes".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        return Err("release intent grew beyond its byte limit".into());
    }
    Ok(bytes)
}

/// Read bounded, owned control-plane data without following a final link or waiting on a pipe.
pub fn record_bytes(path: &Path) -> Result<Vec<u8>> {
    read_record(&open_record(path, &path.symlink_metadata()?)?)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub schema: u32,
    pub repository: String,
    pub source: String,
    pub version: String,
    pub client_version: String,
    pub baseline: String,
    pub target: String,
    pub updater: Updater,
    pub inputs: Vec<Input>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Updater {
    #[serde(rename = "atpkg")]
    Atpkg,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub path: String,
    pub sha256: String,
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("GIT_"))
    {
        command.env_remove(key);
    }
    let output = command
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-C",
        ])
        .arg(repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("TZ", "UTC")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn core(text: &str) -> bool {
    let parts: Vec<_> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 9
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// The product ledger allocates MAJOR.MINOR.0 on its own release line.
fn check_product_version(version: &str) -> Result<()> {
    if core(version) && version.ends_with(".0") {
        Ok(())
    } else {
        Err("codex-alab VERSION must be a regular MAJOR.MINOR.0 number".into())
    }
}

/// The MAJOR.MINOR line of an already validated regular version.
fn line(text: &str) -> Result<(u64, u64)> {
    let mut parts = text.split('.');
    let mut next = || -> Result<u64> { Ok(parts.next().ok_or("version has no line")?.parse()?) };
    Ok((next()?, next()?))
}

fn source_id(text: &str) -> bool {
    text.len() == 40
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn github_repository(origin: &str) -> Result<String> {
    let origin = origin.trim().trim_end_matches('/').trim_end_matches(".git");
    let (host, path) = if let Some(path) = origin.strip_prefix("https://github.com/") {
        ("github.com", path)
    } else if let Some(rest) = origin.strip_prefix("git@") {
        rest.split_once(':').ok_or("invalid GitHub origin")?
    } else if let Some(rest) = origin.strip_prefix("ssh://git@") {
        rest.split_once('/').ok_or("invalid GitHub origin")?
    } else {
        return Err("origin must name an admitted GitHub codex-alab repository".into());
    };
    let (owner, name) = path
        .split_once('/')
        .ok_or("origin has no repository owner")?;
    if owner.is_empty()
        || owner.len() > 100
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        || !owner.as_bytes()[0].is_ascii_alphanumeric()
        || !name.eq_ignore_ascii_case("codex-alab")
        || !(host.eq_ignore_ascii_case("github.com")
            || host.eq_ignore_ascii_case(&format!("gh-{owner}")))
    {
        return Err("origin must name an admitted GitHub codex-alab repository".into());
    }
    Ok(format!("{}/codex-alab", owner.to_ascii_lowercase()))
}

fn repository(repo: &Path) -> Result<String> {
    github_repository(&git(repo, &["remote", "get-url", "origin"])?)
}

fn fetched_main(repo: &Path, output: &[u8]) -> Result<String> {
    #[derive(Deserialize)]
    struct Snapshot<'a> {
        #[serde(rename = "ref")]
        reference: &'a str,
        oid: &'a str,
    }
    let snapshot: Snapshot<'_> = serde_json::from_slice(output)?;
    if snapshot.reference != "refs/agh/fetched-main" || !source_id(snapshot.oid) {
        return Err("agh fetch-main returned an invalid snapshot".into());
    }
    if git(repo, &["cat-file", "-t", snapshot.oid])?.trim_end() != "commit" {
        return Err("agh fetch-main did not name a commit object".into());
    }
    // Another fetch may already have moved the shared ref. Only this
    // authenticated fetch supplied both the observed tip and its ancestry.
    Ok(snapshot.oid.to_owned())
}

fn check_fetched_ancestry(repo: &Path, source: &str, output: &[u8]) -> Result<()> {
    git(
        repo,
        &[
            "merge-base",
            "--is-ancestor",
            source,
            &fetched_main(repo, output)?,
        ],
    )?;
    Ok(())
}

impl Release {
    /// Read bounded regular intent data without following its final symlink or
    /// waiting on a pipe. Source authority is rechecked separately before acts.
    pub fn load(path: &Path) -> Result<Self> {
        let release: Self = serde_json::from_slice(&record_bytes(path)?)?;
        check_product_version(&release.version)?;
        if release.schema != 2
            || !source_id(&release.source)
            || release.target != "aarch64-apple-darwin"
            || !core(&release.client_version)
            || !core(&release.baseline)
            || github_repository(&format!("https://github.com/{}", release.repository))?
                != release.repository
        {
            return Err("unsupported or malformed release intent".into());
        }
        Ok(release)
    }
    /// Read committed inputs, never a dirty working file. This performs a fresh fetch
    /// before checking source ancestry; a local tracking ref alone is insufficient.
    pub fn prepare(repo: &Path, source: &str, target: &str) -> Result<Self> {
        if !source_id(source) {
            return Err("--source must be a full lowercase commit ID".into());
        }
        if target != "aarch64-apple-darwin" {
            return Err("codex-alab currently distributes only aarch64-apple-darwin".into());
        }
        let repository = repository(repo)?;
        let fetched = Command::new("agh")
            .args(["repo", "fetch-main", &repository])
            .arg(repo)
            .output()?;
        if !fetched.status.success() {
            return Err(format!(
                "agh source fetch failed: {}",
                String::from_utf8_lossy(&fetched.stderr)
            )
            .into());
        }
        check_fetched_ancestry(repo, source, &fetched.stdout)?;
        let release = Self::from_commit(repo, source, target)?;
        if release.repository != repository {
            return Err("source origin changed during admission".into());
        }
        Ok(release)
    }

    fn from_commit(repo: &Path, source: &str, target: &str) -> Result<Self> {
        let mut inputs = Vec::new();
        let mut read = |path: &str| -> Result<String> {
            let object = format!("{source}:{path}");
            let mode = git(repo, &["ls-tree", source, "--", path])?;
            if !mode.starts_with("100644 blob ") {
                return Err(format!("{path} must be a committed regular file").into());
            }
            let text = git(repo, &["cat-file", "blob", &object])?;
            inputs.push(Input {
                path: path.to_owned(),
                sha256: format!("{:x}", Sha256::digest(text.as_bytes())),
            });
            Ok(text)
        };
        let version = read("VERSION")?.trim().to_owned();
        let client_version = read("alab/client-version")?.trim().to_owned();
        let ledger: toml::Value = toml::from_str(&read("alab/versions.toml")?)?;
        read("trust.lock")?;
        read("codex-rs/Cargo.lock")?;
        check_product_version(&version)?;
        if !core(&client_version) {
            return Err("alab/client-version must be a regular MAJOR.MINOR.PATCH number".into());
        }
        let rows = ledger
            .get("release")
            .and_then(toml::Value::as_array)
            .ok_or("missing release ledger")?;
        let rows: Vec<_> = rows
            .iter()
            .filter(|row| row.get("version").and_then(toml::Value::as_str) == Some(&version))
            .collect();
        if rows.len() != 1 {
            return Err("release must have exactly one committed version ledger entry".into());
        }
        let baseline = rows[0]
            .get("baseline")
            .and_then(toml::Value::as_str)
            .filter(|text| core(text))
            .ok_or("invalid upstream baseline")?
            .to_owned();
        // The floor is frozen with the claim: the client line must be above the line of
        // the baseline the ledger row records. No upstream ref is read here, so a stable
        // tag that appears after the claim cannot change what this source releases as.
        if line(&client_version)? <= line(&baseline)? {
            return Err(format!(
                "alab/client-version {client_version} is not above its recorded upstream \
                 baseline {baseline}: a release must not call itself older than the upstream \
                 code it holds"
            )
            .into());
        }
        Ok(Self {
            schema: 2,
            repository: repository(repo)?,
            source: source.to_owned(),
            version,
            client_version,
            baseline,
            target: target.to_owned(),
            updater: Updater::Atpkg,
            inputs,
        })
    }

    /// The identity covers every declared input, including target and update ownership.
    pub fn id(&self) -> Result<String> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }

    /// Revalidate a loaded intent against its immutable local source objects.
    /// The outer atpkg runner separately authenticates remote source and compiler
    /// authority and retains both across the complete controller process group.
    pub fn check_local(&self, repo: &Path) -> Result<()> {
        if self.schema != 2 || !source_id(&self.source) || self.target != "aarch64-apple-darwin" {
            return Err("unsupported or malformed release intent".into());
        }
        if Self::from_commit(repo, &self.source, &self.target)? != *self {
            return Err("release intent differs from its committed source inputs".into());
        }
        Ok(())
    }

    /// Create once. A resume must read the same bytes, rather than silently selecting
    /// a new main, version, target, or toolchain after a failure.
    pub fn save(&self, path: &Path) -> Result<()> {
        check_product_version(&self.version)?;
        save_record(path, &serde_json::to_vec_pretty(self)?)
    }
}

fn flush_existing_record(path: &Path, parent: &Path, named: &Metadata, bytes: &[u8]) -> Result<()> {
    let file = open_record(path, named)?;
    if read_record(&file)? != bytes {
        return Err("release intent already exists with different inputs".into());
    }
    file.sync_all()?;
    File::open(parent)?.sync_all()?;
    let current = path.symlink_metadata()?;
    if !current.is_file() || current.len() != bytes.len() as u64 {
        return Err("release record changed during its retry flush".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let identity = |m: &Metadata| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.mode(),
                m.uid(),
                m.mtime(),
                m.mtime_nsec(),
            )
        };
        if identity(named) != identity(&current)
            || identity(&file.metadata()?) != identity(&current)
        {
            return Err("release record changed during its retry flush".into());
        }
    }
    Ok(())
}

/// Atomically create durable private controller data, or flush an identical record
/// in place on retry even when the volume has no space for another temporary file.
pub fn save_record(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    if bytes.len() > 64 * 1024 {
        return Err("release record exceeds its byte limit".into());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    match path.symlink_metadata() {
        Ok(named) => return flush_existing_record(path, parent, &named, bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(path) {
        Ok(_) => File::open(parent)?.sync_all()?,
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
            flush_existing_record(path, parent, &path.symlink_metadata()?, bytes)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_ancestry_uses_the_commit_reported_by_its_own_fetch() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let repo = temporary.path();
        git(repo, &["init", "--quiet"])?;
        git(repo, &["config", "user.name", "release fixture"])?;
        git(
            repo,
            &["config", "user.email", "release-fixture@example.invalid"],
        )?;
        let commit = |message: &str| -> Result<String> {
            git(repo, &["commit", "--quiet", "--allow-empty", "-m", message])?;
            Ok(git(repo, &["rev-parse", "HEAD"])?.trim().to_owned())
        };
        let older = commit("older main")?;
        let source = commit("release candidate")?;
        let fetched = commit("main observed by this fetch")?;
        let later = commit("main observed afterward")?;
        let reference = "refs/agh/fetched-main";
        let receipt = serde_json::to_vec(&serde_json::json!({"ref":reference, "oid":fetched}))?;
        // Other worktrees can overwrite the snapshot in either direction.
        git(repo, &["update-ref", reference, &older])?;
        check_fetched_ancestry(repo, &source, &receipt)?;
        git(repo, &["update-ref", reference, &later])?;
        assert!(check_fetched_ancestry(repo, &later, &receipt).is_err());
        // A newer observation absent locally, a missing/invalid receipt, or a
        // tag that peels to the right commit cannot borrow the available ref.
        git(repo, &["tag", "-a", "tagged", "-m", "fixture", &fetched])?;
        let tag = git(repo, &["rev-parse", "refs/tags/tagged"])?;
        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"ref":reference}),
            serde_json::json!({"ref":"refs/heads/main", "oid":fetched}),
            serde_json::json!({"ref":reference, "oid":"HEAD"}),
            serde_json::json!({"ref":reference, "oid":"f".repeat(40)}),
            serde_json::json!({"ref":reference, "oid":tag.trim()}),
        ] {
            assert!(check_fetched_ancestry(repo, &source, &serde_json::to_vec(&invalid)?).is_err());
        }
        assert!(check_fetched_ancestry(repo, &source, b"truncated{").is_err());
        Ok(())
    }

    #[test]
    fn preparation_binds_committed_locks_and_ignores_dirty_working_files() -> Result<()> {
        let repo = tempfile::tempdir()?;
        git(repo.path(), &["init", "--quiet"])?;
        git(
            repo.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/source-fixture/codex-alab.git",
            ],
        )?;
        git(
            repo.path(),
            &["config", "user.email", "release-fixture@example.invalid"],
        )?;
        git(repo.path(), &["config", "user.name", "release fixture"])?;
        std::fs::create_dir(repo.path().join("alab"))?;
        std::fs::create_dir(repo.path().join("codex-rs"))?;
        for (path, text) in [
            ("VERSION", "0.5.0\n"),
            ("alab/client-version", "0.168.0\n"),
            (
                "alab/versions.toml",
                "[[release]]\nversion = '0.5.0'\nbaseline = '0.167.1'\n",
            ),
            ("trust.lock", "compiler fixture\n"),
            ("codex-rs/Cargo.lock", "dependencies fixture\n"),
        ] {
            std::fs::write(repo.path().join(path), text)?;
        }
        git(repo.path(), &["add", "."])?;
        git(repo.path(), &["commit", "--quiet", "-m", "fixture"])?;
        let source = git(repo.path(), &["rev-parse", "HEAD"])?;
        let original = Release::from_commit(repo.path(), source.trim(), "aarch64-apple-darwin")?;
        original.check_local(repo.path())?;
        git(
            repo.path(),
            &[
                "remote",
                "set-url",
                "origin",
                "https://github.com/other-fixture/codex-alab.git",
            ],
        )?;
        assert!(original.check_local(repo.path()).is_err());
        git(
            repo.path(),
            &[
                "remote",
                "set-url",
                "origin",
                "https://github.com/source-fixture/codex-alab.git",
            ],
        )?;
        let mut forged = original.clone();
        forged.version = "0.6.0".into();
        assert!(forged.check_local(repo.path()).is_err());
        std::fs::write(repo.path().join("VERSION"), "9.9.9\n")?;
        std::fs::write(repo.path().join("trust.lock"), "different compiler\n")?;
        assert_eq!(
            Release::from_commit(repo.path(), source.trim(), "aarch64-apple-darwin")?,
            original
        );
        original.check_local(repo.path())?;
        git(repo.path(), &["add", "."])?;
        git(
            repo.path(),
            &["commit", "--quiet", "-m", "incomplete release"],
        )?;
        let newer = git(repo.path(), &["rev-parse", "HEAD"])?;
        assert!(Release::from_commit(repo.path(), newer.trim(), "aarch64-apple-darwin").is_err());
        std::fs::write(repo.path().join("VERSION"), "0.5.0\n")?;
        git(repo.path(), &["add", "."])?;
        git(
            repo.path(),
            &["commit", "--quiet", "-m", "changed compiler"],
        )?;
        let newer = git(repo.path(), &["rev-parse", "HEAD"])?;
        let changed = Release::from_commit(repo.path(), newer.trim(), "aarch64-apple-darwin")?;
        assert_ne!(changed.id()?, original.id()?);
        assert_ne!(changed.inputs, original.inputs);
        Ok(())
    }

    fn frozen_fixture(client: &str) -> Result<(tempfile::TempDir, String)> {
        let repo = tempfile::tempdir()?;
        git(repo.path(), &["init", "--quiet"])?;
        git(
            repo.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/source-fixture/codex-alab.git",
            ],
        )?;
        git(
            repo.path(),
            &["config", "user.email", "release-fixture@example.invalid"],
        )?;
        git(repo.path(), &["config", "user.name", "release fixture"])?;
        std::fs::create_dir(repo.path().join("alab"))?;
        std::fs::create_dir(repo.path().join("codex-rs"))?;
        for (path, text) in [
            ("VERSION", "0.6.0\n"),
            ("alab/client-version", client),
            (
                "alab/versions.toml",
                "[[release]]\nversion = '0.5.0'\nbaseline = '0.167.1'\n\
                 [[release]]\nversion = '0.6.0'\nbaseline = '0.167.2'\n",
            ),
            ("trust.lock", "compiler fixture\n"),
            ("codex-rs/Cargo.lock", "dependencies fixture\n"),
        ] {
            std::fs::write(repo.path().join(path), text)?;
        }
        git(repo.path(), &["add", "."])?;
        git(repo.path(), &["commit", "--quiet", "-m", "claim 0.6.0"])?;
        let source = git(repo.path(), &["rev-parse", "HEAD"])?.trim().to_owned();
        Ok((repo, source))
    }

    #[test]
    fn a_stable_tagged_after_the_claim_cannot_change_the_frozen_release() -> Result<()> {
        let (repo, source) = frozen_fixture("0.168.0\n")?;
        let frozen = Release::from_commit(repo.path(), &source, "aarch64-apple-darwin")?;
        assert_eq!(
            (frozen.baseline.as_str(), frozen.client_version.as_str()),
            ("0.167.2", "0.168.0")
        );
        // Upstream then names code this source already holds: its next minor's stable,
        // the tag that refused 0.5.0's source release, and a patch of the recorded line.
        for late in ["rust-v0.168.0", "rust-v0.167.3", "rust-v0.169.0"] {
            git(
                repo.path(),
                &["update-ref", &format!("refs/upstream-tags/{late}"), &source],
            )?;
        }
        git(
            repo.path(),
            &["update-ref", "refs/remotes/upstream/main", &source],
        )?;
        let later = Release::from_commit(repo.path(), &source, "aarch64-apple-darwin")?;
        assert_eq!(later, frozen);
        assert_eq!(later.id()?, frozen.id()?);
        frozen.check_local(repo.path())?;
        Ok(())
    }

    #[test]
    fn a_client_line_at_or_below_its_recorded_baseline_is_refused() -> Result<()> {
        for behind in ["0.167.0\n", "0.167.9\n", "0.166.0\n"] {
            let (repo, source) = frozen_fixture(behind)?;
            let refused = Release::from_commit(repo.path(), &source, "aarch64-apple-darwin")
                .err()
                .ok_or("a client line not above its baseline was frozen")?;
            assert!(
                refused
                    .to_string()
                    .contains("recorded upstream baseline 0.167.2"),
                "{refused}"
            );
        }
        // The next minor, any later minor and a later major are all above the line.
        for above in ["0.168.0\n", "0.170.0\n", "1.0.0\n"] {
            let (repo, source) = frozen_fixture(above)?;
            Release::from_commit(repo.path(), &source, "aarch64-apple-darwin")?;
        }
        Ok(())
    }

    #[test]
    fn only_the_product_version_requires_a_zero_patch() -> Result<()> {
        let (repo, source) = frozen_fixture("0.168.7\n")?;
        let mut release = Release::from_commit(repo.path(), &source, "aarch64-apple-darwin")?;
        let intent = repo.path().join("intent.json");
        release.save(&intent)?;
        assert_eq!(Release::load(&intent)?, release);

        // Commit the matching ledger row as well, so the version itself causes the refusal.
        std::fs::write(repo.path().join("VERSION"), "0.6.1\n")?;
        std::fs::write(
            repo.path().join("alab/versions.toml"),
            "[[release]]\nversion = '0.6.1'\nbaseline = '0.167.2'\n",
        )?;
        git(
            repo.path(),
            &["commit", "--quiet", "-am", "nonzero product patch"],
        )?;
        let patched_source = git(repo.path(), &["rev-parse", "HEAD"])?;
        assert!(Release::from_commit(repo.path(), patched_source.trim(), &release.target).is_err());

        release.version = "0.6.1".into();
        let malformed = repo.path().join("patched-intent.json");
        assert!(release.save(&malformed).is_err());
        save_record(&malformed, &serde_json::to_vec(&release)?)?;
        assert!(Release::load(&malformed).is_err());
        Ok(())
    }

    #[test]
    fn regular_product_numbers_do_not_select_the_updater() -> Result<()> {
        for valid in ["0.5.0", "1.2.0", "2026.1111.0"] {
            check_product_version(valid)?;
        }
        for invalid in [
            "0.5.0-dev.1",
            "0.5.0+gabcdef012",
            "01.2.3",
            "1.2",
            "1.2.3\n",
            "0.12.1",
            "0.1234567890.0",
        ] {
            assert!(check_product_version(invalid).is_err());
        }
        let mut release = Release {
            schema: 2,
            repository: "source-fixture/codex-alab".into(),
            source: "a".repeat(40),
            version: "0.5.0".into(),
            client_version: "0.168.0".into(),
            baseline: "0.167.1".into(),
            target: "aarch64-apple-darwin".into(),
            updater: Updater::Atpkg,
            inputs: Vec::new(),
        };
        let original = release.id()?;
        let dir = tempfile::tempdir()?;
        let intent = dir.path().join("intent.json");
        release.save(&intent)?;
        release.save(&intent)?;
        release.source = "b".repeat(40);
        assert_ne!(original, release.id()?);
        assert!(release.save(&intent).is_err());
        let saved: Release = serde_json::from_slice(&std::fs::read(intent)?)?;
        assert_eq!(saved.source, "a".repeat(40));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_release_intent_replays_without_staging_in_its_directory() -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;
        let (repo, source) = frozen_fixture("0.168.0\n")?;
        let release = Release::from_commit(repo.path(), &source, "aarch64-apple-darwin")?;
        let journal = tempfile::tempdir()?;
        let path = journal.path().join("intent.json");
        release.save(&path)?;
        let original = record_bytes(&path)?;
        let inode = path.metadata()?.ino();
        let link = journal.path().join("linked-intent");
        std::os::unix::fs::symlink(&path, &link)?;
        let mut changed = release.clone();
        changed.source = "0".repeat(40);
        let dir = std::fs::File::open(journal.path())?;
        dir.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000))?;
        let before = dir.metadata()?.modified()?;
        dir.set_permissions(std::fs::Permissions::from_mode(0o500))?;
        let replay = release.save(&path);
        let conflict = changed.save(&path);
        let linked = release.save(&link);
        // A privileged process can still create names in this directory. The mtime
        // control detects even temporary staging for an existing record in either case.
        let missing = journal.path().join("new.json");
        // SAFETY: geteuid has no preconditions.
        let create = (unsafe { libc::geteuid() } != 0).then(|| release.save(&missing));
        dir.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        replay?;
        assert!(conflict.is_err());
        assert!(linked.is_err());
        if let Some(create) = create {
            assert!(create.is_err());
            assert!(!missing.exists());
        }
        assert_eq!(dir.metadata()?.modified()?, before);
        assert_eq!(path.metadata()?.ino(), inode);
        assert_eq!(record_bytes(&path)?, original);
        assert_eq!(Release::load(&path)?, release);
        Ok(())
    }

    #[test]
    fn concurrent_record_creators_accept_only_the_winning_bytes() -> Result<()> {
        let journal = tempfile::tempdir()?;
        let path = journal.path().join("record");
        let candidates = [b"one", b"two", b"one", b"two"];
        let gate = std::sync::Barrier::new(candidates.len());
        let outcomes = std::thread::scope(|scope| {
            let writers: Vec<_> = candidates
                .into_iter()
                .map(|bytes| {
                    let gate = &gate;
                    let path = &path;
                    scope.spawn(move || {
                        gate.wait();
                        (bytes, save_record(path, bytes).is_ok())
                    })
                })
                .collect();
            writers
                .into_iter()
                .map(|writer| writer.join().expect("record writer"))
                .collect::<Vec<_>>()
        });
        let winner = record_bytes(&path)?;
        assert!(candidates.iter().any(|bytes| bytes.as_slice() == winner));
        for (bytes, accepted) in outcomes {
            assert_eq!(accepted, bytes.as_slice() == winner);
        }
        assert_eq!(journal.path().read_dir()?.count(), 1);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn intent_reads_refuse_links_pipes_and_oversized_data() -> Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let root = tempfile::tempdir()?;
        let ordinary = root.path().join("ordinary");
        std::fs::write(&ordinary, b"{}")?;
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&ordinary, &link)?;
        assert!(record_bytes(&link).is_err());
        let huge = root.path().join("huge");
        std::fs::File::create(&huge)?.set_len(64 * 1024 + 1)?;
        assert!(record_bytes(&huge).is_err());
        let fifo = root.path().join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes())?;
        // SAFETY: name is a live NUL-terminated private fixture pathname.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (answer, result) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = answer.send(record_bytes(&fifo).is_err());
        });
        assert!(result.recv_timeout(std::time::Duration::from_secs(60))?);
        Ok(())
    }

    #[test]
    fn repository_identity_accepts_only_canonical_github_transports() -> Result<()> {
        for origin in [
            "https://github.com/source-fixture/codex-alab.git",
            "git@github.com:source-fixture/codex-alab.git",
            "git@gh-source-fixture:source-fixture/codex-alab.git",
            "ssh://git@github.com/source-fixture/codex-alab.git",
            "ssh://git@gh-source-fixture/source-fixture/codex-alab.git",
        ] {
            assert_eq!(github_repository(origin)?, "source-fixture/codex-alab");
        }
        for origin in [
            "https://example.invalid/source-fixture/codex-alab.git",
            "https://user@github.com/source-fixture/codex-alab.git",
            "git@gh-other:source-fixture/codex-alab.git",
            "https://github.com/source-fixture/other.git",
            "https://github.com/source-fixture/codex-alab/extra.git",
            "https://github.com/../codex-alab.git",
        ] {
            assert!(github_repository(origin).is_err());
        }
        Ok(())
    }
}
