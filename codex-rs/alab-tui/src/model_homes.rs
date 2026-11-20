// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Per-login registration of explicit CODEX_HOME locations. Application data stays
//! with the creating login and the receipt sent off the computer exposes no paths.
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::io::{self};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Home {
    pub path: PathBuf,
    dev: u64,
    ino: u64,
}

impl Home {
    pub fn owned(path: &Path) -> io::Result<Self> {
        let path = path.canonicalize()?;
        let m = fs::symlink_metadata(&path)?;
        // SAFETY: geteuid reads the calling process identity.
        if !m.is_dir() || m.mode() & 0o022 != 0 || m.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "model settings home belongs to another account",
            ));
        }
        Ok(Self {
            path,
            dev: m.dev(),
            ino: m.ino(),
        })
    }
    pub fn current(&self) -> bool {
        Self::owned(&self.path).as_ref().ok() == Some(self)
    }
}

fn registry() -> io::Result<PathBuf> {
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| io::Error::other("no account home"))?;
    Ok(base.join(".codex/alab/model-homes"))
}

pub fn register(path: &Path) -> io::Result<()> {
    let home = Home::owned(path)?;
    let dir = registry()?;
    if dir
        .parent()
        .and_then(Path::parent)
        .and_then(|p| p.canonicalize().ok())
        .as_ref()
        == Some(&home.path)
    {
        return Ok(());
    }
    crate::record::ensure_dir(&dir)?;
    let name = format!("{:x}", Sha256::digest(home.path.as_os_str().as_bytes()));
    let dest = dir.join(name);
    if read(&dest).as_ref().ok() == Some(&home) {
        return Ok(());
    }
    let mut file = tempfile::NamedTempFile::new_in(&dir)?;
    serde_json::to_writer(file.as_file_mut(), &home)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(&dest)?;
    fs::File::open(dir)?.sync_all()
}

fn read(path: &Path) -> io::Result<Home> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let info = file.metadata()?;
    if !info.is_file() || info.len() > 16 * 1024 {
        return Err(io::Error::other("invalid home registration"));
    }
    let mut data = Vec::new();
    file.take(16385).read_to_end(&mut data)?;
    Ok(serde_json::from_slice(&data)?)
}

pub fn known(primary: &Path) -> io::Result<Vec<Home>> {
    let mut homes = vec![Home::owned(primary)?];
    let dir = registry()?;
    if let Some(default) = dir
        .parent()
        .and_then(Path::parent)
        .and_then(|p| Home::owned(p).ok())
        && !homes.contains(&default)
    {
        homes.push(default);
    }
    let entries = match fs::read_dir(&dir) {
        Ok(list) => list,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(homes),
        Err(e) => return Err(e),
    };
    let mut names = entries
        .map(|e| e.map(|v| v.file_name()))
        .collect::<io::Result<Vec<OsString>>>()?;
    names.sort();
    for name in names {
        if !name.as_bytes().iter().all(u8::is_ascii_hexdigit) || name.as_bytes().len() != 64 {
            continue;
        }
        let registered = read(&dir.join(name))?;
        if registered.current() && !homes.contains(&registered) {
            homes.push(registered);
        }
    }
    Ok(homes)
}
