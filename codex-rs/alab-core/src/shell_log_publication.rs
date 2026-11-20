// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use sha2::Digest;
use sha2::Sha256;
use std::os::unix::fs::FileExt;

/// SHA-256 of the bytes the native reader actually retained, independent of later file edits.
#[derive(Clone, Debug, Default)]
pub struct PrefixDigest(Sha256);

impl PrefixDigest {
    pub fn append(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub fn hex(&self) -> String {
        format!("{:x}", self.0.clone().finalize())
    }
}

/// Serializes staging, SQLite intent changes, and recovery for one physical thread directory.
pub struct PublicationLock {
    directory: LogDirectory,
    lease: File,
}

impl PublicationLock {
    pub(super) fn new(directory: LogDirectory, lease: File) -> Self {
        Self { directory, lease }
    }

    fn ensure_active(&self) -> io::Result<()> {
        if self.directory.still_named()?
            && named_directory(&self.directory.thread, PUBLICATION_DIR, &self.lease)?
        {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "shell publication directory was replaced",
            ))
        }
    }

    pub fn prepare_copy(
        &self,
        expected_bytes: u64,
        populate: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> io::Result<(String, String)> {
        self.ensure_active()?;
        check_extent(expected_bytes)?;
        let (basename, file) = self.directory.create_named(populate, true)?;
        let name = basename
            .into_string()
            .map_err(|_| io::Error::other("invalid shell capture name"))?;
        match digest(&file, expected_bytes) {
            Ok(Some(hash)) => Ok((name, hash)),
            result => {
                self.cleanup(&name, false)?;
                Err(result
                    .err()
                    .unwrap_or_else(|| io::Error::other("shell capture length changed")))
            }
        }
    }

    pub fn contains_prefix(
        &self,
        name: &str,
        expected_bytes: u64,
        prefix_sha256: &str,
    ) -> io::Result<bool> {
        check_extent(expected_bytes)?;
        check_name(name.as_ref())?;
        for leaf in [name.to_owned(), format!("{STAGE_PREFIX}{name}")] {
            if let Some(file) = self.open_original(&leaf, name)?
                && digest(&file, expected_bytes)?.as_deref() == Some(prefix_sha256)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Link the exact staged prefix, or verify a previously linked/disclosed file.
    pub fn publish(
        &self,
        name: &str,
        expected_bytes: u64,
        prefix_sha256: &str,
    ) -> io::Result<bool> {
        self.ensure_active()?;
        Ok(self
            .directory
            .verified_link(name, expected_bytes, prefix_sha256, false)?
            .is_some())
    }

    /// A matching private stage proves a watcher created the final link. Ordinary tool
    /// publications never create that alias, so rejection cannot withdraw their original file.
    pub fn cleanup(&self, name: &str, attached: bool) -> io::Result<()> {
        check_name(name.as_ref())?;
        let stage = format!("{STAGE_PREFIX}{name}");
        let _slot = self.directory.slot_fd()?;
        let Some(file) = self.open_original(&stage, name)? else {
            return Ok(());
        };
        let stat = fstat(&file)?;
        if !attached {
            unlink_if_matching(&self.directory.thread, name.as_ref(), &stat)?;
        }
        unlink_if_matching(&self.directory.thread, reserve_name(name).as_ref(), &stat)?;
        unlink_if_matching(&self.directory.thread, stage.as_ref(), &stat)
    }

    /// Call with the SQLite intent names read while holding this lock. An orphan may be a
    /// successful attach whose writer died before removing its stage. Keep any final link.
    pub fn clean_orphan_stages(&self, pending_names: &[String]) -> io::Result<()> {
        self.ensure_active()?;
        for entry in Dir::read_from(&self.directory.thread)?.take(2 * MAX_THREAD_LOGS + 3) {
            let entry = entry?;
            let Some(name) = entry
                .file_name()
                .to_str()
                .ok()
                .and_then(|s| s.strip_prefix(STAGE_PREFIX))
            else {
                continue;
            };
            if capture_tag(name.as_ref()).is_some()
                && !pending_names.iter().any(|pending| pending == name)
            {
                self.cleanup(name, true)?;
            }
        }
        Ok(())
    }

    fn open_original(&self, leaf: &str, capture_name: &str) -> io::Result<Option<File>> {
        self.ensure_active()?;
        self.directory.open_original(leaf, capture_name)
    }
}

impl LogDirectory {
    /// An ordinary poll can acquire its watcher's same prefix, even while SQLite is
    /// outstanding. Removing the private marker under the short slot lock changes
    /// custody: future diagnostic rejection can no longer withdraw a disclosed name.
    pub fn claim_for_tool(
        &self,
        name: &str,
        expected_bytes: u64,
        prefix_sha256: &str,
    ) -> io::Result<Option<File>> {
        self.verified_link(name, expected_bytes, prefix_sha256, true)
    }

    fn verified_link(
        &self,
        name: &str,
        expected_bytes: u64,
        prefix_sha256: &str,
        disclose: bool,
    ) -> io::Result<Option<File>> {
        for _ in 0..3 {
            match self.verify_and_link(name, expected_bytes, prefix_sha256, disclose) {
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
        }
        Err(io::Error::from(io::ErrorKind::WouldBlock))
    }

    fn verify_and_link(
        &self,
        name: &str,
        expected_bytes: u64,
        prefix_sha256: &str,
        disclose: bool,
    ) -> io::Result<Option<File>> {
        check_extent(expected_bytes)?;
        check_name(name.as_ref())?;
        let stage = format!("{STAGE_PREFIX}{name}");
        let file = if let Some(file) = self.open_original(name, name)? {
            file
        } else if let Some(file) = self.open_original(&stage, name)? {
            file
        } else {
            return Ok(None);
        };
        let before = file.metadata()?;
        if digest(&file, expected_bytes)?.as_deref() != Some(prefix_sha256) {
            return Ok(None);
        }
        let _slot = self.slot_fd()?;
        if !unchanged(&before, &file.metadata()?) {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        let original = fstat(&file)?;
        if !self.names_file(name.as_ref(), &file)? {
            if verified_inode(&self.thread, stage.as_ref(), &file_tag(&original))?.is_none() {
                return Ok(None);
            }
            match linkat(&self.thread, &stage, &self.thread, name, AtFlags::empty()) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(Errno::NOENT) => return Ok(None),
                Err(err) => return Err(err.into()),
            }
        }
        if !self.names_file(name.as_ref(), &file)? {
            return Ok(None);
        }
        if disclose {
            unlink_if_matching(&self.thread, reserve_name(name).as_ref(), &original)?;
            unlink_if_matching(&self.thread, stage.as_ref(), &original)?;
        }
        Ok(Some(file))
    }

    fn open_original(&self, leaf: &str, capture_name: &str) -> io::Result<Option<File>> {
        let Some(tag) = capture_tag(capture_name.as_ref()) else {
            return Ok(None);
        };
        if !self.still_named()? {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        if verified_inode(&self.thread, leaf.as_ref(), tag)?.is_none() {
            return Ok(None);
        }
        // A concurrent pathname substitution must neither follow a symlink nor block on a FIFO.
        let file: File = match openat(
            &self.thread,
            leaf,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => fd.into(),
            Err(Errno::NOENT | Errno::LOOP) => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let held = fstat(&file)?;
        Ok(
            (FileType::from_raw_mode(held.st_mode) == FileType::RegularFile
                && held.st_uid == geteuid().as_raw()
                && held.st_mode & 0o7777 == 0o600
                && file_tag(&held) == tag
                && verified_inode(&self.thread, leaf.as_ref(), tag)?.is_some())
            .then_some(file),
        )
    }
}

impl Drop for PublicationLock {
    fn drop(&mut self) {
        // The directory is reclaimable only while this lease still excludes publishers.
        if let Ok(_slot) = self.directory.slot_fd() {
            let _ = remove_empty_directory(&self.directory.thread, PUBLICATION_DIR, &self.lease);
            let _ = self.directory.retire_if_empty();
        }
    }
}

fn check_extent(bytes: u64) -> io::Result<()> {
    if (1..=crate::SHELL_OUTPUT_PREFIX_BYTES as u64).contains(&bytes) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid shell diagnostic extent",
        ))
    }
}

fn digest(file: &File, expected: u64) -> io::Result<Option<String>> {
    let before = file.metadata()?;
    if before.len() != expected {
        return Ok(None);
    }
    let mut hash = Sha256::new();
    let mut offset = 0;
    let mut buf = [0_u8; 8192];
    while offset < expected {
        let take = (expected - offset).min(buf.len() as u64) as usize;
        match file.read_exact_at(&mut buf[..take], offset) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(err) => return Err(err),
        }
        hash.update(&buf[..take]);
        offset += take as u64;
    }
    if !unchanged(&before, &file.metadata()?) {
        // Linking the same verified prefix into native tool history also changes ctime.
        // A concurrent change is retryable, never definitive evidence of corruption.
        return Err(io::Error::from(io::ErrorKind::WouldBlock));
    }
    Ok(Some(format!("{:x}", hash.finalize())))
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    before.len() == after.len()
        && (
            after.mtime(),
            after.mtime_nsec(),
            after.ctime(),
            after.ctime_nsec(),
        ) == (
            before.mtime(),
            before.mtime_nsec(),
            before.ctime(),
            before.ctime_nsec(),
        )
}
