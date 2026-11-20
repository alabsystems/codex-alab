// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::Release;
use crate::Result;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;

const GIB: u64 = 1 << 30;
const DEFAULT_PACKAGE_GIB: u64 = 35;
const BUILD_FLOOR: u64 = 20 * GIB;

fn package_bytes(setting: Option<&std::ffi::OsStr>) -> Result<u64> {
    let gib = match setting {
        None => DEFAULT_PACKAGE_GIB,
        Some(value) if value.is_empty() => DEFAULT_PACKAGE_GIB,
        Some(value) => value
            .to_str()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("ALAB_MIN_FREE_GB must be an unsigned GiB count")?,
    };
    Ok(gib
        .checked_mul(GIB)
        .ok_or("ALAB_MIN_FREE_GB exceeds u64 bytes")?
        .max(BUILD_FLOOR))
}

#[derive(Debug)]
pub struct ResourceStop(String);

impl fmt::Display for ResourceStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "release disk admission: {}", self.0)
    }
}

impl std::error::Error for ResourceStop {}

// A fresh campaign has no output directory yet. Query the closest existing
// ancestor, including a mounted output on retry. Propagate inaccessible paths
// and dangling links: neither establishes a filesystem for new writes.
fn available(path: &Path) -> io::Result<u64> {
    let absolute = std::path::absolute(path)?;
    for ancestor in absolute.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                if !std::fs::metadata(ancestor)?.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        "expected a write directory",
                    ));
                }
                let fs = rustix::fs::statvfs(ancestor)?;
                return fs
                    .f_bavail
                    .checked_mul(fs.f_frsize)
                    .ok_or_else(|| io::Error::other("available disk space exceeds u64"));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("no existing parent directory"))
}

fn require<'a>(
    volumes: impl IntoIterator<Item = (&'a Path, u64)>,
    probe: impl Fn(&Path) -> io::Result<u64>,
) -> Result<()> {
    for (path, required) in volumes {
        let free = probe(path)
            .map_err(|error| ResourceStop(format!("cannot measure {}: {error}", path.display())))?;
        if free < required {
            return Err(ResourceStop(format!(
                "{} has {free} bytes available, requires {} GiB",
                path.display(),
                required / GIB
            ))
            .into());
        }
    }
    Ok(())
}

fn compiler_volumes(repo: &Path, cargo_home: &Path) -> Result<()> {
    let git = crate::source::common(repo)?;
    let scratch = crate::protected::scratch_parent(false)?;
    require(
        [
            (cargo_home, BUILD_FLOOR),
            (&git, BUILD_FLOOR),
            (&scratch, BUILD_FLOOR),
        ],
        available,
    )
}

// Recognize original public command forms only. Aliases and leading Cargo
// configuration options require the selected frontend's parser.
fn allocating_compile(arguments: &[OsString]) -> bool {
    let Some((first, rest)) = arguments.split_first() else {
        return false;
    };
    let (command, options) = if first == "--unverified" || first == "trust" {
        let Some((command, options)) = rest.split_first() else {
            return false;
        };
        if first == "trust" && !matches!(command.to_str(), Some("build" | "check" | "test")) {
            return false;
        }
        (command, options)
    } else {
        (first, rest)
    };
    // A later word can be an option value or a run/test program argument,
    // even without a separator (run's positional arguments consume its tail).
    match options.first().and_then(|word| word.to_str()) {
        Some("--help" | "-h") => return false,
        Some("--list") if command == "install" || command == "package" => return false,
        Some("-l") if command == "package" => return false,
        _ => {}
    }
    matches!(
        command.to_str(),
        Some(
            "build"
                | "check"
                | "test"
                | "run"
                | "bench"
                | "doc"
                | "fix"
                | "rustc"
                | "rustdoc"
                | "install"
                | "package"
                | "publish"
                | "fetch"
                | "vendor"
        )
    )
}

fn resolves_metadata(arguments: &[OsString]) -> bool {
    let mut args = arguments
        .iter()
        .skip_while(|word| matches!(word.to_str(), Some("--locked" | "--offline" | "--frozen")));
    if args.next().is_none_or(|word| word != "metadata") {
        return false;
    }
    // Metadata has no positional arguments or forwarded program. Its options
    // do not accept a separate hyphen-prefixed value. An attached option value
    // such as --manifest-path=--no-deps is still part of that single argument.
    // The pinned driver rejects explicit --unverified on this non-compiler
    // command, and accepts --version only at top level.
    !args.any(|word| {
        matches!(
            word.to_str(),
            Some("--no-deps" | "-h" | "--help" | "--version" | "-V" | "--unverified")
        )
    })
}

pub fn public_compile(
    repo: &Path,
    cargo_home: &mut PathBuf,
    writes: &mut [PathBuf],
    arguments: &[OsString],
) -> Result<()> {
    let builds = allocating_compile(arguments);
    if !builds && !resolves_metadata(arguments) {
        return Ok(());
    }
    if builds {
        // Keep the canonical paths we actually measured for the later worker
        // handoff. Its ordinary sandbox checks run again after selection.
        for output in writes {
            *output = crate::protected::owned_directory(output)?;
            before_output_write(output)?;
        }
    }
    *cargo_home = crate::protected::owned_directory(cargo_home)?;
    compiler_volumes(repo, cargo_home)
}

pub fn product(
    repo: &Path,
    release: &Release,
    state: &Path,
    cargo_home: &Path,
    v8_cache: &Path,
) -> Result<()> {
    let room = package_bytes(std::env::var_os("ALAB_MIN_FREE_GB").as_deref())?;
    checkout(repo, release, state, cargo_home, v8_cache, room)
}

pub fn reader_checkout(
    repo: &Path,
    release: &Release,
    state: &Path,
    cargo_home: &Path,
    v8_cache: &Path,
) -> Result<()> {
    checkout(repo, release, state, cargo_home, v8_cache, BUILD_FLOOR)
}

fn checkout(
    repo: &Path,
    release: &Release,
    state: &Path,
    cargo_home: &Path,
    v8_cache: &Path,
    room: u64,
) -> Result<()> {
    let campaign = state.join(release.id()?);
    let source = campaign.join(crate::source::SOURCE_DIR);
    let target = campaign.join(crate::source::BUILD_DIR);
    require(
        [
            (source.as_path(), room),
            (target.as_path(), room),
            (v8_cache, BUILD_FLOOR),
        ],
        available,
    )?;
    compiler_volumes(repo, cargo_home)
}

pub fn ripgrep(repo: &Path, release: &Release, state: &Path, cargo_home: &Path) -> Result<()> {
    let room = package_bytes(std::env::var_os("ALAB_MIN_FREE_GB").as_deref())?;
    // Each attempt creates its own install and build directories under this campaign.
    require([(state.join(release.id()?).as_path(), room)], available)?;
    compiler_volumes(repo, cargo_home)
}

pub fn predecessor(repo: &Path, state: &Path, cargo_home: &Path, v8_cache: &Path) -> Result<()> {
    require([(state, BUILD_FLOOR), (v8_cache, BUILD_FLOOR)], available)?;
    compiler_volumes(repo, cargo_home)
}

pub(crate) fn before_output_write(directory: &Path) -> Result<()> {
    require([(directory, BUILD_FLOOR)], available)
}

#[cfg(test)]
#[path = "space_tests.rs"]
mod tests;
