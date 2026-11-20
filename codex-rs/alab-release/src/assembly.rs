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

/// A published first-party binary must retain the bytes produced by the certified compiler.
fn copy_certified_product(input: &Path, dest: &Path, hash: &str) -> Result<()> {
    std::fs::copy(input, dest)?;
    if product::file_hash(dest)? != hash {
        return Err("product executable changed during assembly copy".into());
    }
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))?;
    std::fs::File::open(dest)?.sync_all()?;
    Ok(())
}

/// An exact existing package needs no staging space. A fresh package is built
/// beside its final name so the exclusive rename stays on the same filesystem.
fn stage_output(
    out: &Path,
    previous: Option<&Assembly>,
    admit: impl FnOnce(&Path) -> Result<()>,
) -> Result<Option<tempfile::TempDir>> {
    match std::fs::symlink_metadata(out) {
        Ok(_) => {
            let old = previous.ok_or("existing package has no matching assembly record")?;
            if tree(out)? != old.files {
                return Err("assembled package changed; preserved for inspection".into());
            }
            return Ok(None);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = out.parent().ok_or("package output has no parent")?;
    admit(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".codex-assembly-")
        .tempdir_in(parent)?;
    Ok(Some(stage))
}

/// Both assembly and a resumed scheduler consume the same observed bytes under
/// the original checkout lease. Keep their content addresses with the parsed builds.
struct Observations {
    product: product::Build,
    product_id: String,
    rg: crate::ripgrep::Build,
    rg_id: String,
}

impl Observations {
    fn read(
        repo: &Path,
        release: &Release,
        checkout: &crate::source::Checkout,
        product_path: &Path,
        rg_path: &Path,
    ) -> Result<Self> {
        let product_bytes = crate::record_bytes(product_path)?;
        let product = product::Build::from_observation(&product_bytes, repo, release, checkout)?;
        let rg_bytes = crate::record_bytes(rg_path)?;
        let rg: crate::ripgrep::Build = serde_json::from_slice(&rg_bytes)?;
        rg.check(repo, release, &product.compiler, &checkout.campaign)?;
        Ok(Self {
            product,
            product_id: format!("{:x}", sha2::Sha256::digest(&product_bytes)),
            rg,
            rg_id: format!("{:x}", sha2::Sha256::digest(&rg_bytes)),
        })
    }
}

impl crate::schedule::Ready {
    pub(crate) fn check_outputs(&self, repo: &Path, release: &Release, state: &Path) -> Result<()> {
        let checkout = crate::source::Checkout::reopen(repo, release, state)?;
        let observed = Observations::read(repo, release, &checkout, &self.product, &self.ripgrep)?;
        if self.product
            != checkout
                .campaign
                .join(format!("build-{}.json", observed.product_id))
            || self.ripgrep
                != checkout
                    .campaign
                    .join(format!("ripgrep-{}.json", observed.rg_id))
        {
            return Err("qualified build observations changed before reservation".into());
        }
        Ok(())
    }
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
    let checkout = crate::source::Checkout::reopen(repo, release, state)?;
    let Observations {
        product: build,
        product_id: observation_id,
        rg,
        rg_id,
    } = Observations::read(repo, release, &checkout, observation, ripgrep_observation)?;
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
    let Some(stage) = stage_output(&out, previous.as_ref(), crate::space::before_output_write)?
    else {
        println!(
            "Package verified for reuse: {}\nQualification and publication: unverified",
            out.display()
        );
        return Ok(());
    };
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
        copy_certified_product(&input, &dest, hash)?;
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
    let mut compiler = serde_json::to_value(&build.compiler)?;
    let evidence: std::collections::BTreeMap<_, _> = build
        .binaries
        .iter()
        .map(|(name, hash)| {
            (
                name,
                serde_json::json!({"built_sha256":hash,"packaged_sha256":hash}),
            )
        })
        .collect();
    compiler["verification"] = serde_json::json!({
        "source": release.source,
        "lane": "certify",
        "include_dependencies": true,
        "report_sha256": build.certification.sha256,
        "binaries": evidence,
        "authority": "observation of successful live certified compilation; saved report bytes confer no verifier authority"
    });
    json(
        &package.join("alab-build.json"),
        &serde_json::json!({
            "source": release.source, "version": release.version, "client_version": release.client_version,
            "baseline": release.baseline, "target": release.target, "updater": release.updater,
            "compiler": compiler, "upstream": build.upstream,
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
#[path = "assembly_ready_tests.rs"]
mod ready_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certified_product_bytes_survive_packaging_and_a_replaced_binary_refuses() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("built");
        let dest = root.path().join("packaged");
        std::fs::write(&source, b"full binary including symbols")?;
        let digest = product::file_hash(&source)?;
        copy_certified_product(&source, &dest, &digest)?;
        assert_eq!(std::fs::read(&dest)?, b"full binary including symbols");
        std::fs::write(&source, b"replaced")?;
        assert!(copy_certified_product(&source, &dest, &digest).is_err());
        Ok(())
    }

    #[test]
    fn fresh_assembly_admits_its_destination_before_creating_staging() -> Result<()> {
        let parent = tempfile::tempdir()?;
        let out = parent.path().join("package");
        let stopped = stage_output(&out, None, |destination| {
            assert_eq!(destination, parent.path());
            assert_eq!(std::fs::read_dir(destination)?.count(), 0);
            Err("low destination space".into())
        })
        .unwrap_err();
        assert_eq!(stopped.to_string(), "low destination space");
        assert_eq!(std::fs::read_dir(parent.path())?.count(), 0);
        let stage = stage_output(&out, None, |_| Ok(()))?.unwrap();
        assert_eq!(stage.path().parent(), Some(parent.path()));
        Ok(())
    }

    #[test]
    fn reuse_detects_extra_files_permissions_and_executable_replacement() -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))?;
        for directory in ["bin", "codex-path", "codex-resources"] {
            let directory = root.path().join(directory);
            std::fs::create_dir(&directory)?;
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755))?;
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
        let previous = Assembly {
            schema: 1,
            intent: "test intent".into(),
            observation: "test observation".into(),
            output: root.path().to_path_buf(),
            files: tree(root.path())?,
        };
        assert!(
            stage_output(root.path(), Some(&previous), |_| {
                panic!("a verified package needs no staging space")
            })?
            .is_none()
        );
        write(&root.path().join("codex-resources/extra"), b"extra")?;
        assert!(tree(root.path()).is_err());
        std::fs::remove_file(root.path().join("codex-resources/extra"))?;
        let executable = root.path().join("bin/codex");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o744))?;
        assert!(tree(root.path()).is_err());
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
        std::fs::write(&executable, b"changed executable")?;
        assert_ne!(tree(root.path())?, previous.files);
        std::fs::remove_file(&executable)?;
        std::os::unix::fs::symlink(root.path().join("LICENSE"), &executable)?;
        assert!(tree(root.path()).is_err());
        Ok(())
    }
}
