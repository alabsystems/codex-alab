// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Locate the authenticated control peer's native image, never a PATH match.

use codex_alab_aterm::Client;
use codex_alab_core::session::ProcessIdentity;
use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

pub(crate) struct Host {
    pid: u32,
    identity: ProcessIdentity,
    path: PathBuf,
    image: File,
}

impl Host {
    pub fn capture(client: &Client) -> Result<Self, String> {
        let pid = client
            .server_pid()
            .filter(|pid| *pid > 1 && *pid != std::process::id())
            .ok_or("the native control host has no independent kernel identity")?;
        let identity = crate::identity::of_pid(pid).map_err(|error| error.to_string())?;
        let path = image_path(pid)
            .map_err(|error| error.to_string())?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let image = File::open(&path).map_err(|error| error.to_string())?;
        let host = Self {
            pid,
            identity,
            path,
            image,
        };
        host.validate(client)?;
        Ok(host)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn validate(&self, client: &Client) -> Result<(), String> {
        if !client.is_open()
            || !client.holds_claim()
            || client.server_pid() != Some(self.pid)
            || crate::identity::of_pid(self.pid).map_err(|error| error.to_string())?
                != self.identity
            || image_path(self.pid)
                .map_err(|error| error.to_string())?
                .canonicalize()
                .map_err(|error| error.to_string())?
                != self.path
        {
            return Err("the authenticated native control host changed".into());
        }
        let opened = self.image.metadata().map_err(|error| error.to_string())?;
        let named = std::fs::symlink_metadata(&self.path).map_err(|error| error.to_string())?;
        let identity = |metadata: &std::fs::Metadata| {
            (
                metadata.dev(),
                metadata.ino(),
                metadata.len(),
                metadata.mtime(),
                metadata.mtime_nsec(),
            )
        };
        if !opened.is_file()
            || !named.is_file()
            || opened.permissions().mode() & 0o111 == 0
            || identity(&opened) != identity(&named)
        {
            return Err("the native control host executable changed".into());
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn image_path(pid: u32) -> std::io::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let pid = libc::pid_t::try_from(pid).map_err(std::io::Error::other)?;
    let mut bytes = vec![0; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: proc_pidpath writes into the initialized, bounded byte buffer.
    let size = unsafe { libc::proc_pidpath(pid, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if size <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| std::io::Error::other("unterminated native host image path"))?;
    bytes.truncate(end);
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(target_os = "linux")]
fn image_path(pid: u32) -> std::io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn image_path(_pid: u32) -> std::io::Result<PathBuf> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "native host image identity unavailable",
    ))
}
