// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native layout-v1 assembly. Only verified source-pinned auxiliary bytes and
//! observed build outputs enter the package. Assembly grants no signing authority.

use crate::Release;
use crate::Result;
use crate::product;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FileStamp {
    sha256: String,
    mode: u32,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Assembly {
    schema: u32,
    intent: String,
    observation: String,
    output: PathBuf,
    files: std::collections::BTreeMap<String, FileStamp>,
}

fn tree(root: &Path) -> Result<std::collections::BTreeMap<String, FileStamp>> {
    let mut files = std::collections::BTreeMap::new();
    for (directory, expected) in [
        (
            "",
            vec![
                "LICENSE",
                "NOTICE",
                "alab-build.json",
                "bin",
                "codex-package.json",
                "codex-path",
                "codex-resources",
            ],
        ),
        ("bin", product::BINARIES.to_vec()),
        ("codex-path", vec!["rg"]),
        ("codex-resources", vec![]),
    ] {
        let dir = root.join(directory);
        let info = std::fs::symlink_metadata(&dir)?;
        if !info.is_dir() || info.permissions().mode() & 0o7777 != 0o755 {
            return Err("package directory has links or unsafe permissions".into());
        }
        let actual: std::collections::BTreeSet<_> = std::fs::read_dir(&dir)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<_>>()?;
        let wanted = expected
            .iter()
            .map(|name| std::ffi::OsString::from(*name))
            .collect();
        if actual != wanted {
            return Err("package layout contains missing or unexpected entries".into());
        }
        for name in expected {
            let path = dir.join(name);
            let info = std::fs::symlink_metadata(&path)?;
            if info.is_dir() {
                continue;
            }
            let mode = info.permissions().mode() & 0o7777;
            let executable = directory == "bin" || directory == "codex-path";
            if mode != if executable { 0o755 } else { 0o644 } {
                return Err("package file has unexpected permissions".into());
            }
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("package path is not UTF-8")?
                .to_owned();
            files.insert(
                relative,
                FileStamp {
                    sha256: product::file_hash(&path)?,
                    mode,
                },
            );
        }
    }
    Ok(files)
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    platforms: std::collections::BTreeMap<String, Platform>,
}

#[derive(Deserialize)]
struct Platform {
    size: u64,
    hash: String,
    digest: String,
    format: String,
    path: String,
    providers: Vec<Provider>,
}

#[derive(Deserialize)]
struct Provider {
    url: String,
}

fn ripgrep_pin(repo: &Path, release: &Release) -> Result<Platform> {
    let blob = crate::git(
        repo,
        &[
            "cat-file",
            "blob",
            &format!("{}:scripts/codex_package/rg", release.source),
        ],
    )?;
    let (_, json) = blob
        .split_once('\n')
        .ok_or("ripgrep manifest has no body")?;
    let mut manifest: Manifest = serde_json::from_str(json)?;
    let pin = manifest
        .platforms
        .remove("macos-aarch64")
        .ok_or("no ARM macOS ripgrep")?;
    let provider = pin.providers.first().ok_or("ripgrep has no provider")?;
    if manifest.name != "rg"
        || release.target != "aarch64-apple-darwin"
        || pin.hash != "sha256"
        || pin.format != "tar.gz"
        || pin.size == 0
        || pin.size > 32 * 1024 * 1024
        || pin.digest.len() != 64
        || !pin
            .digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || pin.providers.len() != 1
        || !provider
            .url
            .starts_with("https://github.com/BurntSushi/ripgrep/releases/download/")
        || pin.path.contains("..")
        || pin.path.starts_with('/')
        || pin.path.contains('\\')
        || !pin.path.ends_with("/rg")
    {
        return Err("unsupported or invalid source-pinned ripgrep manifest".into());
    }
    Ok(pin)
}

pub fn ripgrep_version(repo: &Path, release: &Release) -> Result<String> {
    let pin = ripgrep_pin(repo, release)?;
    let version = pin
        .path
        .strip_prefix("ripgrep-")
        .and_then(|path| path.strip_suffix("-aarch64-apple-darwin/rg"))
        .ok_or("invalid source-pinned ripgrep version")?;
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("invalid source-pinned ripgrep version".into());
    }
    Ok(version.to_owned())
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
    file.sync_all()?;
    Ok(())
}

fn json(path: &Path, value: &serde_json::Value) -> Result<()> {
    write(path, &serde_json::to_vec_pretty(value)?)
}

pub fn assemble(
    repo: &Path,
    release: &Release,
    observation: &Path,
    ripgrep_observation: &Path,
    state: &Path,
    out: &Path,
) -> Result<()> {
    release.check_local(repo)?;
    let checkout = crate::source::Checkout::prepare(repo, release, state)?;
    let observation_bytes = crate::record_bytes(observation)?;
    let observation_id = format!("{:x}", sha2::Sha256::digest(&observation_bytes));
    let build: product::Build = serde_json::from_slice(&observation_bytes)?;
    if build.target != checkout.output()? || build.upstream != product::upstream(repo, release)? {
        return Err("assembly observation belongs to another campaign".into());
    }
    build.check(release)?;
    let rg_bytes = crate::record_bytes(ripgrep_observation)?;
    let rg_id = format!("{:x}", sha2::Sha256::digest(&rg_bytes));
    let rg: crate::ripgrep::Build = serde_json::from_slice(&rg_bytes)?;
    rg.check(repo, release, &build.compiler)?;
    if !out.is_absolute() {
        return Err("package output must be absolute".into());
    }
    let parent = std::fs::canonicalize(out.parent().ok_or("package output has no parent")?)?;
    let out = parent.join(out.file_name().ok_or("package output has no name")?);
    let intent_id = release.id()?;
    let receipt = checkout
        .campaign
        .join(format!("assembly-trust-rg-{rg_id}.json"));
    let previous: Option<Assembly> = match std::fs::symlink_metadata(&receipt) {
        Ok(_) => Some(serde_json::from_slice(&crate::record_bytes(&receipt)?)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if previous.as_ref().is_some_and(|old| {
        old.schema != 1
            || old.intent != intent_id
            || old.observation != observation_id
            || old.output != out
    }) {
        return Err("assembly campaign already records another operation".into());
    }
    match std::fs::symlink_metadata(&out) {
        Ok(_) => {
            let old = previous.ok_or("existing package has no matching assembly record")?;
            if tree(&out)? != old.files {
                return Err("assembled package changed; preserved for inspection".into());
            }
            println!(
                "Package verified for reuse: {}\nQualification and publication: unverified",
                out.display()
            );
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let stage = tempfile::Builder::new()
        .prefix(".codex-assembly-")
        .tempdir_in(&parent)?;
    let package = stage.path().join("package");
    std::fs::create_dir(&package)?;
    for directory in ["bin", "codex-path", "codex-resources"] {
        std::fs::create_dir(package.join(directory))?;
        std::fs::set_permissions(
            package.join(directory),
            std::fs::Permissions::from_mode(0o755),
        )?;
    }
    std::fs::set_permissions(&package, std::fs::Permissions::from_mode(0o755))?;
    for (binary, hash) in &build.binaries {
        if !product::BINARIES.contains(&binary.as_str()) {
            return Err("unexpected product executable in build observation".into());
        }
        let input = build.target.join("release").join(binary);
        let dest = package.join("bin").join(binary);
        std::fs::copy(&input, &dest)?;
        if product::file_hash(&dest)? != *hash {
            return Err("product executable changed during assembly copy".into());
        }
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
        let status = std::process::Command::new("/usr/bin/xcrun")
            .args(["strip", "-S", "-x"])
            .arg(&dest)
            .status()?;
        if !status.success() {
            return Err("could not strip packaged product copy".into());
        }
        std::fs::File::open(&dest)?.sync_all()?;
    }
    let rg_out = package.join("codex-path/rg");
    std::fs::copy(&rg.binary, &rg_out)?;
    if product::file_hash(&rg_out)? != rg.sha256 {
        return Err("ripgrep changed during assembly copy".into());
    }
    std::fs::set_permissions(&rg_out, std::fs::Permissions::from_mode(0o755))?;
    let status = std::process::Command::new("/usr/bin/xcrun")
        .args(["strip", "-S", "-x"])
        .arg(&rg_out)
        .status()?;
    if !status.success() {
        return Err("could not strip packaged ripgrep".into());
    }
    std::fs::File::open(&rg_out)?.sync_all()?;
    for name in ["LICENSE", "NOTICE"] {
        let bytes = crate::git(
            repo,
            &["cat-file", "blob", &format!("{}:{name}", release.source)],
        )?;
        write(&package.join(name), bytes.as_bytes())?;
    }
    json(
        &package.join("codex-package.json"),
        &serde_json::json!({
            "layoutVersion": 1, "version": release.version, "target": release.target,
            "variant": "codex", "entrypoint": "bin/codex", "execServer": "bin/codex-exec-server",
            "resourcesDir": "codex-resources", "pathDir": "codex-path"
        }),
    )?;
    json(
        &package.join("alab-build.json"),
        &serde_json::json!({
            "source": release.source, "version": release.version, "client_version": release.client_version,
            "baseline": release.baseline, "target": release.target, "updater": release.updater,
            "compiler": build.compiler, "upstream": build.upstream,
            "ripgrep": format!("ripgrep@{}", rg.version), "ripgrep_features": "pcre2",
            "ripgrep_build_observation_sha256": rg_id,
            "packager": "codex-release/native-layout-v1",
            "build_observation_sha256": observation_id
        }),
    )?;
    checkout.check(release)?;
    build.check(release)?;
    let assembled = Assembly {
        schema: 1,
        intent: release.id()?,
        observation: observation_id,
        output: out.clone(),
        files: tree(&package)?,
    };
    if previous.as_ref().is_some_and(|old| old != &assembled) {
        return Err("retry produced different package bytes; earlier record preserved".into());
    }
    for directory in ["bin", "codex-path", "codex-resources", ""] {
        std::fs::File::open(package.join(directory))?.sync_all()?;
    }
    crate::save_record(&receipt, &serde_json::to_vec_pretty(&assembled)?)?;
    // Atomic directory rename is create-only on macOS, including an empty target.
    // SAFETY: both C strings are NUL-checked, and the flag requests exclusive creation.
    use std::os::unix::ffi::OsStrExt;
    let source = std::ffi::CString::new(package.as_os_str().as_bytes())?;
    let dest = std::ffi::CString::new(out.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    if unsafe { libc::renamex_np(source.as_ptr(), dest.as_ptr(), libc::RENAME_EXCL) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    #[cfg(not(target_os = "macos"))]
    return Err("native assembly currently requires macOS".into());
    std::fs::File::open(parent)?.sync_all()?;
    println!(
        "Package assembled: {}\nQualification and publication: unverified",
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuse_detects_extra_files_permissions_and_executable_replacement() -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))?;
        for directory in ["bin", "codex-path", "codex-resources"] {
            std::fs::create_dir(root.path().join(directory))?;
        }
        for name in ["LICENSE", "NOTICE", "alab-build.json", "codex-package.json"] {
            write(&root.path().join(name), b"record")?;
        }
        for name in [
            "bin/codex",
            "bin/codex-code-mode-host",
            "bin/codex-exec-server",
            "codex-path/rg",
        ] {
            write(&root.path().join(name), b"executable")?;
            std::fs::set_permissions(
                root.path().join(name),
                std::fs::Permissions::from_mode(0o755),
            )?;
        }
        let expected = tree(root.path())?;
        write(&root.path().join("codex-resources/extra"), b"extra")?;
        assert!(tree(root.path()).is_err());
        std::fs::remove_file(root.path().join("codex-resources/extra"))?;
        let executable = root.path().join("bin/codex");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o744))?;
        assert!(tree(root.path()).is_err());
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
        std::fs::write(&executable, b"changed executable")?;
        assert_ne!(tree(root.path())?, expected);
        std::fs::remove_file(&executable)?;
        std::os::unix::fs::symlink(root.path().join("LICENSE"), &executable)?;
        assert!(tree(root.path()).is_err());
        Ok(())
    }
}
