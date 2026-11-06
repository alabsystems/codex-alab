// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE MOUNT TABLE, AND A PATH RESOLVED ONLY OVER LOCAL MOUNTS: how the repository probe
//! ([`crate::repo`]) enters a folder, as aterm's does.
//!
//! * [`table`] lists every mount without asking any file system: `getfsstat(MNT_NOWAIT)` and
//!   `/usr/share/firmlinks` on macOS, one bounded read of `/proc/self/mountinfo` on Linux, an
//!   empty incomplete table elsewhere. A table cut at [`MAX_MOUNTS`] (or [`MOUNTINFO_CAP`]
//!   bytes) says so ([`Table::complete`]), and proves nothing.
//! * [`mount_of`] is pure: the mount whose point is the longest one the path lies under, both
//!   read in their data-volume spelling where a macOS firmlink names them (a user directory lies on
//!   `/System/Volumes/Data`).
//! * [`resolve_local`] resolves every link on a path ONE COMPONENT AT A TIME, each component
//!   looked at (`lstat`, `readlink`) only once the table places the path so far on a local,
//!   non-automounted mount. A stalled network share or an automount trigger is never
//!   touched, so this cannot block where `std::fs::canonicalize` can, and what it answers lies
//!   on such a mount too. A folder on a share, on an automounted volume (macOS's `/home`, a
//!   Linux autofs), or reached by a link that leads onto one, is refused.
//!
//! PROVENANCE: ported from aterm main 240d2cb43c: `aterm_sysprobe::volumes` (`table`,
//! `mount_of`, `parse_firmlinks`, `parse_mountinfo`, `unescape_mount`, `LOCAL_FSTYPES`) and
//! `aterm_agent::harness::disk_idle::resolve_local`, by their unix rule. codex-alab ships no
//! Windows build, so the Windows arms (mount-manager table, 8.3 spellings, drive-relative
//! links) are not ported.

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

/// The most mounts one [`table`] lists; a longer table is cut there and marked incomplete.
pub const MAX_MOUNTS: usize = 256;

/// The most bytes of `/proc/self/mountinfo` one [`table`] reads (Linux); a table cut here is
/// incomplete.
pub const MOUNTINFO_CAP: usize = 64 * 1024;

/// macOS's data volume, where every firmlinked root lives.
pub const DATA_VOLUME: &str = "/System/Volumes/Data";

/// The file systems the Linux arm calls LOCAL (an allowlist: a network or FUSE type is not).
pub const LOCAL_FSTYPES: &[&str] = &[
    "ext2", "ext3", "ext4", "xfs", "btrfs", "zfs", "f2fs", "tmpfs", "overlay", "vfat", "exfat",
    "ntfs3",
];

/// The most links [`resolve_local`] follows for one path.
const MAX_HOPS: usize = 40;

/// One mount, as the table lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    /// Where it is mounted.
    pub on: PathBuf,
    /// Its file system type (`apfs`, `autofs`, `ext4`, `smbfs`).
    pub fstype: String,
    /// On a local device (macOS `MNT_LOCAL`; Linux [`LOCAL_FSTYPES`]).
    pub local: bool,
    /// Mounted by the automounter (macOS `MNT_AUTOMOUNTED`, or `autofs`).
    pub automounted: bool,
}

/// The mount table ([`table`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    /// Every mount read, at most [`MAX_MOUNTS`].
    pub mounts: Vec<Mount>,
    /// Every mount was read: nothing was cut and the read did not fail. `false` proves
    /// nothing, and nothing resolves over it.
    pub complete: bool,
    /// macOS's firmlinks: each root (`/Users`) and its data-volume spelling
    /// (`/System/Volumes/Data/Users`). Empty elsewhere.
    pub firmlinks: Vec<(PathBuf, PathBuf)>,
}

impl Table {
    /// The data-volume spelling of `path` when it lies under a firmlinked root (a user directory →
    /// its matching path under `/System/Volumes/Data`), else `None`. Lexical.
    #[must_use]
    pub fn on_data(&self, path: &Path) -> Option<PathBuf> {
        self.firmlinks
            .iter()
            .filter(|(root, _)| path.starts_with(root))
            .max_by_key(|(root, _)| root.components().count())
            .and_then(|(root, data)| path.strip_prefix(root).ok().map(|rest| data.join(rest)))
    }

    /// `path` as the file system places it: its data-volume spelling where a firmlink names
    /// it, else itself. A mount point is listed as it was mounted, so both sides of a
    /// comparison are read this way.
    fn placed(&self, path: &Path) -> PathBuf {
        self.on_data(path).unwrap_or_else(|| path.to_path_buf())
    }
}

/// The mount `path` lies on in `t`: the one whose point is the longest `path` lies under, each
/// side read in its data-volume spelling ([`Table::on_data`]). PURE: no path is touched, so a
/// stalled mount cannot block it. `None` for a relative path, one with `.` or `..`, or a table
/// holding no mount of it.
#[must_use]
pub fn mount_of<'a>(path: &Path, t: &'a Table) -> Option<&'a Mount> {
    if !plain_absolute(path) {
        return None;
    }
    let path = t.placed(path);
    t.mounts
        .iter()
        .map(|m| (m, t.placed(&m.on)))
        .filter(|(_, on)| path.starts_with(on))
        .max_by_key(|(_, on)| on.components().count())
        .map(|(m, _)| m)
}

fn plain_absolute(p: &Path) -> bool {
    p.is_absolute()
        && !p
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}

/// `path` with every link on it resolved, one component at a time, each component looked at
/// (`lstat`, `readlink`) only once `table`, read whole, places the path so far on a local,
/// non-automounted mount (the module's rule). What it answers lies on such a mount too (a
/// link onto a share's root is no local folder). `None`: a step lands on another mount, the
/// table was not read whole, the path is relative, a component is missing or could not be
/// read, or more than [`MAX_HOPS`] links. The answer keeps the spelling it was given (and each
/// link's): it is not the file system's canonical spelling.
#[must_use]
pub fn resolve_local(path: &Path, table: &Table) -> Option<PathBuf> {
    resolve_local_or_missing(path, table).ok().flatten()
}

/// [`resolve_local`], telling a path that is not there from one it refuses: `Ok(Some(_))` its
/// answer, `Ok(None)` where a component is missing (`lstat` said no such file) on a local,
/// non-automounted mount, `Err(())` for every other `None` of [`resolve_local`]'s (another
/// mount, a table not read whole, a relative path, a component that could not be read, too many
/// links).
pub(crate) fn resolve_local_or_missing(path: &Path, table: &Table) -> Result<Option<PathBuf>, ()> {
    use std::collections::VecDeque;
    if !table.complete || !path.is_absolute() {
        return Err(());
    }
    let mut todo: VecDeque<std::ffi::OsString> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_os_string()),
            Component::ParentDir => Some("..".into()),
            _ => None,
        })
        .collect();
    let mut at = PathBuf::from("/");
    let mut hops = 0usize;
    while let Some(part) = todo.pop_front() {
        if part == ".." {
            at.pop();
            continue;
        }
        let next = at.join(&part);
        if !local_at(&next, table).ok_or(())? {
            return Err(());
        }
        let meta = match std::fs::symlink_metadata(&next) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(()),
        };
        if !meta.file_type().is_symlink() {
            at = next;
            continue;
        }
        hops += 1;
        if hops > MAX_HOPS {
            return Err(());
        }
        let to = std::fs::read_link(&next).map_err(|_| ())?;
        if to.is_absolute() {
            at = PathBuf::from("/");
        }
        for c in to.components().rev() {
            match c {
                Component::Normal(n) => todo.push_front(n.to_os_string()),
                Component::ParentDir => todo.push_front("..".into()),
                _ => {}
            }
        }
    }
    // A link onto a root, or a `..` back up onto one, ends the walk on a folder no step
    // placed.
    if local_at(&at, table).ok_or(())? {
        Ok(Some(at))
    } else {
        Err(())
    }
}

/// Does `table` place `path` on a local, non-automounted mount? `None` when it places it on
/// none.
fn local_at(path: &Path, table: &Table) -> Option<bool> {
    mount_of(path, table).map(|m| m.local && !m.automounted)
}

/// `/usr/share/firmlinks`' lines (`/Users\tUsers`): each root and its data-volume spelling. A
/// line that is not two fields, or whose root is not absolute, is left out.
#[must_use]
pub fn parse_firmlinks(text: &str) -> Vec<(PathBuf, PathBuf)> {
    text.lines()
        .filter_map(|l| {
            let (root, rel) = l.split_once('\t')?;
            let (root, rel) = (Path::new(root.trim()), rel.trim());
            (plain_absolute(root) && !rel.is_empty() && !rel.starts_with('/'))
                .then(|| (root.to_path_buf(), Path::new(DATA_VOLUME).join(rel)))
        })
        .collect()
}

/// A mountinfo field with its octal escapes (`\040` for a space) decoded.
#[must_use]
pub fn unescape_mount(field: &str) -> String {
    let b = field.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let octal = b
            .get(i + 1..i + 4)
            .filter(|d| b[i] == b'\\' && d.iter().all(|c| (b'0'..=b'7').contains(c)))
            .map(|d| d.iter().fold(0u32, |a, c| a * 8 + u32::from(c - b'0')));
        match octal.and_then(|v| u8::try_from(v).ok()) {
            Some(v) => {
                out.push(v);
                i += 4;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `/proc/self/mountinfo`'s `text` as a [`Table`]. `cut`: the read stopped at
/// [`MOUNTINFO_CAP`]. A torn last line (no newline: the read was cut inside it) is dropped; a
/// cut text, a line that does not parse, or more than [`MAX_MOUNTS`] mounts makes the table
/// incomplete. PURE.
#[must_use]
pub fn parse_mountinfo(text: &str, cut: bool) -> Table {
    let whole = if text.ends_with('\n') {
        text
    } else {
        &text[..text.rfind('\n').map_or(0, |i| i + 1)]
    };
    let mut complete = !cut && whole.len() == text.len();
    let mut mounts = Vec::new();
    for line in whole.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // `<id> <parent> <maj:min> <root> <point> <opts> [optional…] - <type> <source> <super>`
        let fields: Vec<&str> = line.split(' ').collect();
        let Some(dash) = fields.iter().position(|f| *f == "-") else {
            complete = false;
            continue;
        };
        let (Some(point), Some(fstype)) = (fields.get(4), fields.get(dash + 1)) else {
            complete = false;
            continue;
        };
        let on = PathBuf::from(unescape_mount(point));
        if !on.is_absolute() {
            complete = false;
            continue;
        }
        if mounts.len() == MAX_MOUNTS {
            complete = false;
            break;
        }
        let fstype = unescape_mount(fstype);
        let automounted = fstype == "autofs";
        mounts.push(Mount {
            on,
            local: !automounted && LOCAL_FSTYPES.contains(&fstype.as_str()),
            automounted,
            fstype,
        });
    }
    Table {
        mounts,
        complete,
        firmlinks: Vec::new(),
    }
}

/// The machine's mount table now, read without asking any file system (the module's rule).
#[must_use]
pub fn table() -> Table {
    #[cfg(target_os = "macos")]
    {
        arm::table()
    }
    #[cfg(target_os = "linux")]
    {
        match read_capped(Path::new("/proc/self/mountinfo"), MOUNTINFO_CAP) {
            Some((text, cut)) => parse_mountinfo(&text, cut),
            None => Table::default(),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Table::default()
    }
}

/// At most `cap` bytes of `path` as text, and whether there was more.
#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]
fn read_capped(path: &Path, cap: usize) -> Option<(String, bool)> {
    use std::io::Read as _;
    #[cfg(unix)]
    let f = {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .ok()?
    };
    #[cfg(not(unix))]
    let f = std::fs::File::open(path).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut buf = Vec::with_capacity(cap.min(16 * 1024));
    // One byte past the cap says whether there was more.
    let n = f
        .take(u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut buf)
        .ok()?;
    let cut = n > cap;
    buf.truncate(cap);
    Some((String::from_utf8_lossy(&buf).into_owned(), cut))
}

#[cfg(target_os = "macos")]
mod arm {
    use super::Mount;
    use super::Table;
    use super::parse_firmlinks;
    use std::path::Path;
    use std::path::PathBuf;

    // `<sys/mount.h>`: `int getfsstat(struct statfs *buf, int bufsize, int mode)`, which the
    // `libc` crate does not declare. Intel spells the 64-bit-inode `statfs` it fills as
    // `getfsstat$INODE64`, as `libc::statfs` itself is `statfs$INODE64` there.
    unsafe extern "C" {
        #[cfg_attr(target_arch = "x86_64", link_name = "getfsstat$INODE64")]
        fn getfsstat(
            buf: *mut libc::statfs,
            bufsize: libc::c_int,
            mode: libc::c_int,
        ) -> libc::c_int;
    }

    /// The most bytes of `/usr/share/firmlinks` read (the file is about 1 KiB).
    const FIRMLINKS_CAP: usize = 16 * 1024;

    /// `<sys/mount.h>`.
    const MNT_NOWAIT: libc::c_int = 2;
    const MNT_LOCAL: u32 = 0x0000_1000;
    const MNT_AUTOMOUNTED: u32 = 0x0040_0000;

    /// A NUL-terminated `c_char` array as text.
    fn text(chars: &[libc::c_char]) -> String {
        let bytes: Vec<u8> = chars
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c.to_ne_bytes()[0])
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub(super) fn table() -> Table {
        let firmlinks = super::read_capped(Path::new("/usr/share/firmlinks"), FIRMLINKS_CAP)
            .filter(|(_, cut)| !cut)
            .map(|(t, _)| parse_firmlinks(&t))
            .unwrap_or_default();
        // SAFETY: a null buffer asks only for the count; nothing is written.
        let Ok(count) = usize::try_from(unsafe { getfsstat(std::ptr::null_mut(), 0, MNT_NOWAIT) })
        else {
            return Table {
                firmlinks,
                ..Table::default()
            };
        };
        // SAFETY: `statfs` is integers and arrays of them: all-zero is a value.
        let blank: libc::statfs = unsafe { std::mem::zeroed() };
        // Room for a few mounted between the two calls, and one past the cap to tell a table
        // longer than it.
        let room = count.saturating_add(4).min(super::MAX_MOUNTS + 1);
        let mut buf = vec![blank; room];
        let Ok(bytes) = libc::c_int::try_from(std::mem::size_of_val(buf.as_slice())) else {
            return Table {
                firmlinks,
                ..Table::default()
            };
        };
        // SAFETY: `buf` is `bytes` writable bytes of whole `statfs` records, and getfsstat
        // writes at most that many, answering how many it wrote. MNT_NOWAIT asks no file
        // system, so a stalled mount cannot block it.
        let written = unsafe { getfsstat(buf.as_mut_ptr(), bytes, MNT_NOWAIT) };
        let Ok(written) = usize::try_from(written) else {
            return Table {
                firmlinks,
                ..Table::default()
            };
        };
        let mounts: Vec<Mount> = buf
            .iter()
            .take(written.min(super::MAX_MOUNTS))
            .map(|r| Mount {
                on: PathBuf::from(text(&r.f_mntonname)),
                fstype: text(&r.f_fstypename),
                local: r.f_flags & MNT_LOCAL != 0,
                automounted: r.f_flags & MNT_AUTOMOUNTED != 0,
            })
            .filter(|m| m.on.is_absolute())
            .collect();
        Table {
            complete: written <= super::MAX_MOUNTS && written < room && mounts.len() == written,
            mounts,
            firmlinks,
        }
    }
}

#[cfg(test)]
mod file_read_tests {
    use super::*;

    #[test]
    fn capped_reads_reject_non_regular_files_without_losing_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_capped(dir.path(), 4).is_none());
        let path = dir.path().join("table");
        std::fs::write(&path, b"abcdef").unwrap();
        assert_eq!(read_capped(&path, 4), Some(("abcd".into(), true)));
        assert_eq!(read_capped(&path, 8), Some(("abcdef".into(), false)));
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            let fifo_path = dir.path().join("table-fifo");
            let fifo = std::ffi::CString::new(fifo_path.as_os_str().as_bytes()).unwrap();
            // SAFETY: valid NUL-terminated path owned by this private fixture.
            assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
            assert!(read_capped(&fifo_path, 4).is_none());
        }
    }
}
