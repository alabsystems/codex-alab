// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Filesystem custody of disclosed shell captures across same-host process replacement.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::fs::Metadata;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;

use rustix::fs::AtFlags;
use rustix::fs::Dir;
use rustix::fs::FileType;
use rustix::fs::FlockOperation;
use rustix::fs::Mode;
use rustix::fs::OFlags;
use rustix::fs::Stat;
use rustix::fs::flock;
use rustix::fs::fstat;
use rustix::fs::linkat;
use rustix::fs::mkdirat;
use rustix::fs::open;
use rustix::fs::openat;
use rustix::fs::statat;
use rustix::fs::unlinkat;
use rustix::io::Errno;
use rustix::process::geteuid;
use uuid::Uuid;

pub const MAX_THREAD_LOGS: usize = 32;
const STAGE_PREFIX: &str = ".pending-";
const PUBLICATION_DIR: &str = ".publication";

#[path = "shell_log_publication.rs"]
mod publication;
pub use publication::PrefixDigest;
pub use publication::PublicationLock;

// Darwin's /tmp is a system symlink. Open the known physical directory directly.
#[cfg(target_os = "macos")]
const TEMP_ROOT: &str = "/private/tmp";
#[cfg(not(target_os = "macos"))]
const TEMP_ROOT: &str = "/tmp";

fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

fn private_child(parent: &File, name: &str, create: bool) -> io::Result<File> {
    if create {
        match mkdirat(parent, name, Mode::RWXU) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(err) => return Err(err.into()),
        }
    }
    let child: File = openat(parent, name, directory_flags(), Mode::empty())?.into();
    let metadata = child.metadata()?;
    if metadata.uid() != geteuid().as_raw() || metadata.mode() & 0o7777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "shell log directory is not private to the effective user",
        ));
    }
    Ok(child)
}

fn named_directory(parent: &File, name: &str, held: &File) -> io::Result<bool> {
    let named = match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(named) => named,
        Err(Errno::NOENT) => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    let original = fstat(held)?;
    Ok(
        FileType::from_raw_mode(named.st_mode) == FileType::Directory
            && named.st_dev == original.st_dev
            && named.st_ino == original.st_ino
            && named.st_uid == geteuid().as_raw()
            && named.st_mode & 0o7777 == 0o700,
    )
}

/// A directory and its validated ancestry. The display path is never used to mutate files.
#[derive(Debug)]
pub struct LogDirectory {
    temp: File,
    thread: File,
    thread_name: String,
    path: PathBuf,
}

impl LogDirectory {
    /// Resolve the local store home into a stable private namespace, independent of TMPDIR.
    pub fn open(home: &Path, thread_id: &str, create: bool) -> io::Result<Self> {
        let thread_id = Uuid::parse_str(thread_id).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid shell log thread ID")
        })?;
        let home = home.canonicalize()?;
        let home_key = Uuid::new_v5(&Uuid::NAMESPACE_OID, home.as_os_str().as_bytes());
        let thread_key = Uuid::new_v5(&home_key, thread_id.as_bytes());
        let thread_name = format!("codex-alab-shell-{}-{thread_key}", geteuid().as_raw());
        let temp: File = open(TEMP_ROOT, directory_flags(), Mode::empty())?.into();
        if !create {
            // A vanilla thread with nothing here needs no private-store privileges.
            statat(&temp, &thread_name, AtFlags::SYMLINK_NOFOLLOW)?;
        }
        let metadata = temp.metadata()?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "untrusted system temporary directory",
            ));
        }
        let thread = private_child(&temp, &thread_name, create)?;
        let path = Path::new(TEMP_ROOT).join(&thread_name);
        Ok(Self {
            temp,
            thread,
            thread_name,
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn still_named(&self) -> io::Result<bool> {
        named_directory(&self.temp, &self.thread_name, &self.thread)
    }

    pub fn count(&self, limit: usize) -> io::Result<usize> {
        if !self.still_named()? {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "shell log directory was replaced",
            ));
        }
        let mut count = 0;
        for entry in Dir::read_from(&self.thread)? {
            if count == limit {
                break;
            }
            let entry = entry?;
            let name = OsStr::from_bytes(entry.file_name().to_bytes());
            if matches!(name.as_bytes(), b"." | b"..") {
                continue;
            }
            if name == OsStr::new(PUBLICATION_DIR)
                && private_child(&self.thread, PUBLICATION_DIR, false).is_ok()
            {
                continue;
            }
            if let Some(final_name) = name
                .to_str()
                .and_then(|name| name.strip_prefix(STAGE_PREFIX))
                && let Some(tag) = capture_tag(final_name.as_ref())
                && verified_inode(&self.thread, name, tag)?.is_some()
                && self.is_capture(final_name.as_ref())?
            {
                // A pending and final hard link to the same inode use one slot.
                continue;
            }
            count += 1;
        }
        Ok(count)
    }

    /// Fill the initial prefix before publishing it under an exclusive, inode-bound name.
    /// A failed copy never occupies a retained slot.
    pub fn create_capture(
        &self,
        populate: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> io::Result<(OsString, File)> {
        self.create_named(populate, false)
    }

    /// This short lock guards directory entries and slot admission only. Nothing
    /// holds it across a database call, a data copy or hashing the retained bytes.
    fn slot_fd(&self) -> io::Result<File> {
        let fd = private_child(&self.temp, &self.thread_name, false)?;
        flock(&fd, FlockOperation::LockExclusive)?;
        let named = fstat(&fd)?;
        let held = fstat(&self.thread)?;
        if named.st_dev != held.st_dev || named.st_ino != held.st_ino || !self.still_named()? {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(fd)
    }

    fn publication_fd(&self, wait: bool) -> io::Result<Option<File>> {
        loop {
            let slot = self.slot_fd()?;
            let fd = private_child(&self.thread, PUBLICATION_DIR, true)?;
            match flock(&fd, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => return Ok(Some(fd)),
                Err(Errno::WOULDBLOCK) if !wait => return Ok(None),
                Err(Errno::WOULDBLOCK) => {}
                Err(err) => return Err(err.into()),
            }
            drop(slot);
            flock(&fd, FlockOperation::LockExclusive)?;
            if self.still_named()? && named_directory(&self.thread, PUBLICATION_DIR, &fd)? {
                return Ok(Some(fd));
            }
        }
    }

    pub fn try_publication_lock(self) -> io::Result<Option<PublicationLock>> {
        Ok(self
            .publication_fd(false)?
            .map(|lease| PublicationLock::new(self, lease)))
    }

    fn require_slot(&self) -> io::Result<()> {
        if self.count(MAX_THREAD_LOGS)? == MAX_THREAD_LOGS {
            Err(io::Error::other("retained shell log slots are full"))
        } else {
            Ok(())
        }
    }

    fn link_new_capture(&self, stage_parent: &File, stage: &str, name: &str) -> io::Result<()> {
        let _slot = self.slot_fd()?;
        self.require_slot()?;
        Ok(linkat(
            stage_parent,
            stage,
            &self.thread,
            name,
            AtFlags::empty(),
        )?)
    }

    fn create_named(
        &self,
        populate: impl FnOnce(&mut File) -> io::Result<()>,
        pending: bool,
    ) -> io::Result<(OsString, File)> {
        // A diagnostic's bootstrap link is inside the private, quota-counted thread
        // directory. Bind its inode before writing any bytes. Even a crash in the
        // first pair of syscalls can leave only a charged, empty unidentified entry.
        let mut admission = if pending { Some(self.slot_fd()?) } else { None };
        let stage_parent = if pending { &self.thread } else { &self.temp };
        self.require_slot()?;
        let nonce = Uuid::new_v4();
        let stage = format!(".codex-shell-reserve-{nonce}");
        let mut file: File = openat(
            stage_parent,
            &stage,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?
        .into();
        let original = fstat(&file)?;
        let final_name = format!("{nonce}-{}.log", file_tag(&original));
        let name = if pending {
            format!("{STAGE_PREFIX}{final_name}")
        } else {
            final_name.clone()
        };
        // Reserve the private anchor before copying. If a diagnostic writer dies during
        // the copy, the next locked scan can identify both links even without a DB intent.
        let result = (|| {
            if !pending {
                populate(&mut file)?;
                self.link_new_capture(&self.temp, &stage, &name)?;
            } else {
                linkat(stage_parent, &stage, &self.thread, &name, AtFlags::empty())?;
                unlink_if_matching(stage_parent, stage.as_ref(), &original)?;
                drop(admission.take());
                populate(&mut file)?;
            }
            if !self.still_named()?
                || verified_inode(&self.thread, name.as_ref(), &file_tag(&original))?.is_none()
            {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            Ok(())
        })();
        if let Err(err) =
            result.and_then(|()| unlink_if_matching(stage_parent, stage.as_ref(), &original))
        {
            unlink_if_matching(stage_parent, stage.as_ref(), &original)?;
            unlink_if_matching(&self.thread, name.as_ref(), &original)?;
            return Err(err);
        }
        Ok((OsString::from(final_name), file))
    }

    pub fn names_metadata(&self, name: &OsStr, original: &Metadata) -> io::Result<bool> {
        check_name(name)?;
        if !self.still_named()? {
            return Ok(false);
        }
        // Inspect a substituted device, FIFO or symlink without opening it.
        let named = match statat(&self.thread, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(named) => named,
            Err(Errno::NOENT) => return Ok(false),
            Err(err) => return Err(err.into()),
        };
        // MetadataExt exposes u64 on every Unix, whereas struct stat uses the platform ABI.
        #[allow(
            clippy::unnecessary_cast,
            reason = "dev_t and ino_t widths vary across Unix targets"
        )]
        let same = named.st_dev as u64 == original.dev() && named.st_ino as u64 == original.ino();
        Ok(FileType::from_raw_mode(named.st_mode) == FileType::RegularFile && same)
    }

    pub fn names_file(&self, name: &OsStr, file: &File) -> io::Result<bool> {
        self.names_metadata(name, &file.metadata()?)
    }

    /// Roll back only the stored name of this open file in the directory we hold.
    pub fn remove(&self, name: &OsStr, file: &File) -> io::Result<()> {
        if self.names_file(name, file)? {
            match unlinkat(&self.thread, name, AtFlags::empty()) {
                Ok(()) | Err(Errno::NOENT) => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(())
    }

    /// Verify that the recorded basename still names its original regular file.
    pub fn retains_capture(&self, name: &OsStr) -> io::Result<bool> {
        Ok(self.still_named()? && self.is_capture(name)?)
    }

    fn is_capture(&self, name: &OsStr) -> io::Result<bool> {
        let Some(tag) = capture_tag(name) else {
            return Ok(false);
        };
        let stat = match statat(&self.thread, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(Errno::NOENT) => return Ok(false),
            Err(err) => return Err(err.into()),
        };
        Ok(
            FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
                && stat.st_uid == geteuid().as_raw()
                && tag == file_tag(&stat),
        )
    }

    /// Delete capture entries without traversing or removing unrelated entries.
    pub fn retire(self) -> io::Result<()> {
        let lease = self
            .publication_fd(true)?
            .ok_or_else(|| io::Error::from(io::ErrorKind::WouldBlock))?;
        let _slot = self.slot_fd()?;
        if !self.still_named()? {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "shell log directory was replaced",
            ));
        }
        for entry in Dir::read_from(&self.thread)? {
            let entry = entry?;
            let name = OsStr::from_bytes(entry.file_name().to_bytes());
            if let Some(final_name) = name
                .to_str()
                .and_then(|name| name.strip_prefix(STAGE_PREFIX))
                && let Some(tag) = capture_tag(final_name.as_ref())
                && let Some(original) = verified_inode(&self.thread, name, tag)?
            {
                unlink_if_matching(&self.thread, reserve_name(final_name).as_ref(), &original)?;
                unlink_if_matching(&self.thread, name, &original)?;
            } else if self.is_capture(name)? {
                match unlinkat(&self.thread, name, AtFlags::empty()) {
                    Ok(()) | Err(Errno::NOENT) => {}
                    Err(err) => return Err(err.into()),
                }
            }
        }
        remove_empty_directory(&self.thread, PUBLICATION_DIR, &lease)?;
        self.retire_if_empty()
    }

    /// Reclaim the directory of a withdrawn capture only if its original name
    /// still identifies this directory and no other entries occupy it.
    pub fn retire_if_empty(&self) -> io::Result<()> {
        remove_empty_directory(&self.temp, &self.thread_name, &self.thread)
    }
}

fn remove_empty_directory(parent: &File, name: &str, held: &File) -> io::Result<()> {
    if named_directory(parent, name, held)? {
        match unlinkat(parent, name, AtFlags::REMOVEDIR) {
            Ok(()) | Err(Errno::NOENT | Errno::NOTEMPTY | Errno::EXIST) => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn file_tag(stat: &Stat) -> String {
    format!("{:x}-{:x}", stat.st_dev, stat.st_ino)
}

fn capture_tag(name: &OsStr) -> Option<&str> {
    let name = name.to_str()?.strip_suffix(".log")?;
    let (nonce, suffix) = name.split_at_checked(36)?;
    let nonce_id = Uuid::parse_str(nonce).ok()?;
    let tag = suffix.strip_prefix('-')?;
    let (device, inode) = tag.split_once('-')?;
    (nonce_id.get_version_num() == 4
        && nonce_id.to_string() == nonce
        && !device.is_empty()
        && !inode.is_empty()
        && device
            .bytes()
            .chain(inode.bytes())
            .all(|b| b.is_ascii_hexdigit()))
    .then_some(tag)
}

fn check_name(name: &OsStr) -> io::Result<()> {
    if capture_tag(name).is_some() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid shell capture filename",
        ))
    }
}

fn reserve_name(final_name: &str) -> String {
    format!(".codex-shell-reserve-{}", &final_name[..36])
}

fn verified_inode(parent: &File, name: &OsStr, tag: &str) -> io::Result<Option<Stat>> {
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat)
            if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
                && stat.st_uid == geteuid().as_raw()
                && stat.st_mode & 0o7777 == 0o600
                && file_tag(&stat) == tag =>
        {
            Ok(Some(stat))
        }
        Ok(_) | Err(Errno::NOENT) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn unlink_if_matching(parent: &File, name: &OsStr, original: &Stat) -> io::Result<()> {
    let named = match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(Errno::NOENT) => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    if named.st_dev == original.st_dev && named.st_ino == original.st_ino {
        match unlinkat(parent, name, AtFlags::empty()) {
            Ok(()) | Err(Errno::NOENT) => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}
