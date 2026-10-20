// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The vendor daemon's stale packages, against a scratch CODEX_HOME laid out as OpenAI's
//! installer leaves it (`releases/<name>/{codex-package.json, bin/codex, codex -> bin/codex}`,
//! `current` an absolute symlink). Every rule that keeps a release has a twin case where the
//! release goes, so a rule that keeps everything fails here as surely as one that keeps nothing.

use super::*;
use pretty_assertions::assert_eq;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::path::PathBuf;

const TARGET: &str = "aarch64-apple-darwin";
const LOCAL: &str =
    "local-835696c3bd9ed1a50a3cea819686ddb6d55208abf96f9b254a713bc7912688b1-aarch64-apple-darwin";

/// The vendor daemon's pid files, as `app-server-daemon/src/lib.rs` names them (written out
/// here, not taken from the module, so a name dropped there fails here).
const VENDOR_PID_FILES: [&str; 4] = [
    "daemon.pid",
    "daemon-updater.pid",
    "app-server.pid",
    "app-server-updater.pid",
];

/// The machine as a test describes it: what runs, and the mount points (`None`: that table
/// cannot be read).
struct FakeHost {
    executables: Option<Vec<PathBuf>>,
    mount_points: Option<Vec<PathBuf>>,
}

impl FakeHost {
    /// No codex runs, and every mount point is outside the releases.
    fn quiet() -> Self {
        Self {
            executables: Some(vec![PathBuf::from("/usr/bin/true")]),
            mount_points: Some(vec![PathBuf::from("/"), PathBuf::from("/private/tmp")]),
        }
    }

    fn running(exe: PathBuf) -> Self {
        Self {
            executables: Some(vec![PathBuf::from("/usr/bin/true"), exe]),
            ..Self::quiet()
        }
    }
}

impl Host for FakeHost {
    fn executables(&self) -> std::io::Result<Vec<PathBuf>> {
        self.executables
            .clone()
            .ok_or_else(|| std::io::Error::other("no process table"))
    }

    fn mount_points(&self) -> std::io::Result<Vec<PathBuf>> {
        self.mount_points
            .clone()
            .ok_or_else(|| std::io::Error::other("no mount table"))
    }
}

/// The longest TMPDIR [`home`] uses: the gate's `/private/tmp/alab.XXXXXX` is 22 bytes.
const SHORT_TMPDIR: usize = 32;

fn busy(reason: &str) -> Option<KeptAll> {
    Some(KeptAll::Busy(reason.to_string()))
}

fn doubt(reason: &str) -> Option<KeptAll> {
    Some(KeptAll::Doubt(reason.to_string()))
}

/// A scratch CODEX_HOME with the daemon's state directory and package root. A control socket
/// path must stay under 104 bytes, so it goes under TMPDIR only when that is short (the contained
/// gate, `alab/lib.sh` `alab_contained`, gives a short one and lets a test write nowhere else),
/// else under /tmp (macOS's per-user TMPDIR is too long).
fn home() -> tempfile::TempDir {
    let tmp = std::env::temp_dir();
    let root = if tmp.as_os_str().len() <= SHORT_TMPDIR {
        tmp
    } else {
        PathBuf::from("/tmp")
    };
    let home = tempfile::Builder::new()
        .prefix("alab-dp")
        .tempdir_in(&root)
        .unwrap_or_else(|err| panic!("scratch CODEX_HOME under {}: {err}", root.display()));
    std::fs::create_dir_all(home.path().join("app-server-daemon")).unwrap();
    std::fs::create_dir_all(package_root(home.path()).join("releases")).unwrap();
    std::fs::write(package_root(home.path()).join("install.lock"), "").unwrap();
    home
}

fn releases(home: &Path) -> PathBuf {
    package_root(home).join("releases")
}

/// A release as the installer writes it, named `name`, its manifest saying `version`.
fn release_with(home: &Path, name: &str, version: &str, entrypoint: &str) -> PathBuf {
    let dir = releases(home).join(name);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::create_dir_all(dir.join("codex-path")).unwrap();
    std::fs::write(dir.join("bin/codex"), vec![7u8; 4096]).unwrap();
    std::fs::write(dir.join("codex-path/rg"), vec![7u8; 1024]).unwrap();
    std::os::unix::fs::symlink("bin/codex", dir.join("codex")).unwrap();
    std::fs::write(
        dir.join("codex-package.json"),
        manifest(version, entrypoint),
    )
    .unwrap();
    dir
}

fn manifest(version: &str, entrypoint: &str) -> String {
    format!(
        r#"{{"layoutVersion":1,"version":"{version}","target":"{TARGET}","variant":"codex","entrypoint":"{entrypoint}","resourcesDir":"codex-resources","pathDir":"codex-path"}}"#
    )
}

/// The bytes one release made by [`release_with`] holds: its two files, its manifest and its
/// `codex -> bin/codex` link.
fn release_bytes(version: &str) -> u64 {
    (4096 + 1024 + "bin/codex".len() + manifest(version, "bin/codex").len()) as u64
}

fn release(home: &Path, version: &str) -> PathBuf {
    release_with(home, &format!("{version}-{TARGET}"), version, "bin/codex")
}

/// The owner's layout: four vendor releases and the local one `current` names.
fn vendor_layout(home: &Path) {
    for version in ["0.158.0", "0.159.0", "0.159.1", "0.159.2"] {
        release(home, version);
    }
    let local = release_with(home, LOCAL, "0.159.2", "bin/codex");
    select(home, &local);
}

fn select(home: &Path, target: &Path) {
    let current = package_root(home).join("current");
    let _ = std::fs::remove_file(&current);
    std::os::unix::fs::symlink(target, current).unwrap();
}

fn names(home: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(releases(home))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn all_vendor_names() -> Vec<String> {
    let mut names: Vec<String> = ["0.158.0", "0.159.0", "0.159.1", "0.159.2"]
        .iter()
        .map(|version| format!("{version}-{TARGET}"))
        .collect();
    names.push(LOCAL.to_string());
    names.sort();
    names
}

/// The pid of a process that has exited (and been reaped).
fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

fn pid_record(pid: u32) -> String {
    format!(r#"{{"pid":{pid},"processStartTime":"x"}}"#)
}

/// A daemon's control socket in `home`, listening; `accept` says whether anything reached it.
fn control_socket(home: &Path) -> std::os::unix::net::UnixListener {
    let socket_dir = home.join("app-server-control");
    std::fs::create_dir_all(&socket_dir).unwrap();
    let listener =
        std::os::unix::net::UnixListener::bind(socket_dir.join("app-server-control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
}

/// Whether anything connected to `listener` since it was last asked.
fn reached(listener: &std::os::unix::net::UnixListener) -> bool {
    match listener.accept() {
        Ok(_) => true,
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(err) => panic!("accept: {err}"),
    }
}

#[test]
fn superseded_releases_go_and_current_stays() {
    let home = home();
    vendor_layout(home.path());

    let report = prune(home.path(), &FakeHost::quiet());

    assert_eq!(report.kept_all, None);
    assert_eq!(
        report.removed,
        ["0.158.0", "0.159.0", "0.159.1", "0.159.2"].map(|v| format!("{v}-{TARGET}"))
    );
    assert_eq!(
        report.freed_bytes,
        ["0.158.0", "0.159.0", "0.159.1", "0.159.2"]
            .map(release_bytes)
            .iter()
            .sum::<u64>()
    );
    assert_eq!(report.failed, Vec::new());
    assert_eq!(names(home.path()), [LOCAL]);
    // A vendor codex still finds its daemon package.
    let current = package_root(home.path()).join("current");
    assert!(
        current.join("bin/codex").is_file(),
        "current still resolves"
    );
    assert_eq!(
        prune(home.path(), &FakeHost::quiet()).kept_all,
        Some(KeptAll::Nothing(NOTHING_SUPERSEDED))
    );
}

#[test]
fn nothing_goes_while_a_daemon_may_run() {
    // A process runs from one of the releases (as the kernel names it: canonical).
    let home = home();
    vendor_layout(home.path());
    let exe =
        std::fs::canonicalize(releases(home.path()).join(format!("0.158.0-{TARGET}/bin/codex")))
            .unwrap();
    let report = prune(home.path(), &FakeHost::running(exe.clone()));
    assert_eq!(report.kept_all, busy(&format!("{} runs", exe.display())));
    assert_eq!(names(home.path()), all_vendor_names());

    // The process table cannot be read.
    let unreadable = FakeHost {
        executables: None,
        ..FakeHost::quiet()
    };
    let report = prune(home.path(), &unreadable);
    assert_eq!(
        report.kept_all,
        doubt("the process table: no process table")
    );
    assert_eq!(names(home.path()), all_vendor_names());

    // Each of the daemon's pid files, naming a live process or no pid at all.
    let me = std::process::id();
    for name in VENDOR_PID_FILES {
        let pid_file = home.path().join("app-server-daemon").join(name);
        std::fs::write(&pid_file, pid_record(me)).unwrap();
        let report = prune(home.path(), &FakeHost::quiet());
        assert_eq!(
            report.kept_all,
            busy(&format!("{name} names live process {me}"))
        );
        std::fs::write(&pid_file, "").unwrap();
        let report = prune(home.path(), &FakeHost::quiet());
        assert_eq!(report.kept_all, doubt(&format!("{name} names no pid")));
        assert_eq!(names(home.path()), all_vendor_names());
        std::fs::remove_file(&pid_file).unwrap();
    }

    // The control socket answers.
    let listener = control_socket(home.path());
    let report = prune(home.path(), &FakeHost::quiet());
    assert_eq!(report.kept_all, busy(SOCKET_ANSWERS));
    assert_eq!(names(home.path()), all_vendor_names());

    // The twins: a socket nobody listens on and pid files of processes that are gone are a
    // daemon that is not running, and the superseded releases go.
    drop(listener);
    for name in VENDOR_PID_FILES {
        let pid_file = home.path().join("app-server-daemon").join(name);
        std::fs::write(&pid_file, pid_record(dead_pid())).unwrap();
    }
    let report = prune(home.path(), &FakeHost::quiet());
    assert_eq!(report.kept_all, None);
    assert_eq!(names(home.path()), [LOCAL]);
}

/// A running vendor daemon is seen by its pid file or its process before its socket is asked,
/// so a start never connects to it; and with nothing superseded (a vendor-daemon user's every
/// start: `current` and its rollback target) no lock is taken and nothing is probed.
#[test]
fn a_running_daemon_is_seen_without_being_reached() {
    let home = home();
    vendor_layout(home.path());
    let listener = control_socket(home.path());

    let pid_file = home.path().join("app-server-daemon/daemon.pid");
    std::fs::write(&pid_file, pid_record(std::process::id())).unwrap();
    let report = prune(home.path(), &FakeHost::quiet());
    assert!(matches!(report.kept_all, Some(KeptAll::Busy(_))));
    assert!(
        !reached(&listener),
        "a live pid file: the socket is not asked"
    );
    std::fs::remove_file(&pid_file).unwrap();

    let exe =
        std::fs::canonicalize(releases(home.path()).join(format!("0.159.2-{TARGET}/bin/codex")))
            .unwrap();
    let report = prune(home.path(), &FakeHost::running(exe));
    assert!(matches!(report.kept_all, Some(KeptAll::Busy(_))));
    assert!(
        !reached(&listener),
        "a process from the package: the socket is not asked"
    );

    // The negative control: with only the socket to tell, the probe does reach it.
    let report = prune(home.path(), &FakeHost::quiet());
    assert_eq!(report.kept_all, busy(SOCKET_ANSWERS));
    assert!(reached(&listener), "the probe reaches a listening socket");
    assert_eq!(names(home.path()), all_vendor_names());

    // Nothing superseded: `current` and the release selection-history names.
    let quiet_home = self::home();
    release(quiet_home.path(), "0.159.2");
    let local = release_with(quiet_home.path(), LOCAL, "0.159.2", "bin/codex");
    select(quiet_home.path(), &local);
    std::fs::write(
        package_root(quiet_home.path()).join("selection-history"),
        format!("{LOCAL}\n0.159.2-{TARGET}\n1 2 3.000000000\n"),
    )
    .unwrap();
    let listener = control_socket(quiet_home.path());
    let report = prune(quiet_home.path(), &FakeHost::quiet());
    assert_eq!(report.kept_all, Some(KeptAll::Nothing(NOTHING_SUPERSEDED)));
    assert!(!reached(&listener), "nothing superseded: no probe");
    assert!(
        !quiet_home
            .path()
            .join("app-server-daemon/daemon.lock")
            .exists(),
        "nothing superseded: no lock taken"
    );
}

/// A CODEX_HOME whose control socket path is longer than `sun_path` (104 bytes on macOS) is
/// read as codex's client reads it: through the path's symlinks. No socket file there is no
/// daemon; one that answers through its symlink is; one that cannot be reached or followed is a
/// doubt.
#[test]
fn a_long_codex_home_is_read_like_codex_reads_it() {
    let base = home();
    let home = base.path().join("x".repeat(80));
    std::fs::create_dir_all(home.join("app-server-daemon")).unwrap();
    std::fs::create_dir_all(releases(&home)).unwrap();
    vendor_layout(&home);
    let socket_dir = home.join("app-server-control");
    std::fs::create_dir_all(&socket_dir).unwrap();
    let socket = socket_dir.join("app-server-control.sock");
    assert!(
        socket.as_os_str().len() > 104,
        "the path is too long for sun_path"
    );

    // Answers through a symlink to a short socket.
    let short = base.path().join("s.sock");
    let listener = std::os::unix::net::UnixListener::bind(&short).unwrap();
    std::os::unix::fs::symlink(&short, &socket).unwrap();
    let report = prune(&home, &FakeHost::quiet());
    assert_eq!(report.kept_all, busy(SOCKET_ANSWERS));
    drop(listener);

    // A file there that cannot be reached at all.
    std::fs::remove_file(&socket).unwrap();
    std::fs::write(&socket, "").unwrap();
    let report = prune(&home, &FakeHost::quiet());
    assert!(
        matches!(&report.kept_all, Some(KeptAll::Doubt(reason)) if reason.starts_with("the daemon's control socket:")),
        "{report:?}"
    );
    assert_eq!(names(&home), all_vendor_names());

    // A path that cannot be followed (a symlink loop) is a doubt too, not "no daemon".
    std::fs::remove_file(&socket).unwrap();
    std::os::unix::fs::symlink(&socket, &socket).unwrap();
    let report = prune(&home, &FakeHost::quiet());
    assert!(
        matches!(&report.kept_all, Some(KeptAll::Doubt(reason)) if reason.starts_with("the daemon's control socket:")),
        "{report:?}"
    );
    assert_eq!(names(&home), all_vendor_names());

    // The twin: no socket file, no daemon; the superseded releases go.
    std::fs::remove_file(&socket).unwrap();
    assert_eq!(prune(&home, &FakeHost::quiet()).kept_all, None);
    assert_eq!(names(&home), [LOCAL]);
}

#[test]
fn nothing_goes_without_a_valid_current() {
    let home = home();
    vendor_layout(home.path());
    let current = package_root(home.path()).join("current");
    let elsewhere = tempfile::tempdir().unwrap();

    std::fs::remove_file(&current).unwrap();
    assert_eq!(
        prune(home.path(), &FakeHost::quiet()).kept_all,
        doubt(NO_CURRENT)
    );

    std::fs::create_dir(&current).unwrap();
    assert_eq!(
        prune(home.path(), &FakeHost::quiet()).kept_all,
        doubt(NO_CURRENT),
        "current a directory"
    );
    std::fs::remove_dir(&current).unwrap();

    std::os::unix::fs::symlink(elsewhere.path(), &current).unwrap();
    assert_eq!(
        prune(home.path(), &FakeHost::quiet()).kept_all,
        doubt(NO_CURRENT),
        "current outside releases"
    );

    select(home.path(), &releases(home.path()).join("0.160.0-gone"));
    assert_eq!(
        prune(home.path(), &FakeHost::quiet()).kept_all,
        doubt(NO_CURRENT),
        "current dangling"
    );
    assert_eq!(names(home.path()), all_vendor_names());

    // The twin: a relative `current` naming a release is valid.
    std::fs::remove_file(&current).unwrap();
    std::os::unix::fs::symlink(format!("releases/{LOCAL}"), &current).unwrap();
    assert_eq!(prune(home.path(), &FakeHost::quiet()).kept_all, None);
    assert_eq!(names(home.path()), [LOCAL]);
}

#[test]
fn only_what_the_installer_made_goes() {
    let home = home();
    vendor_layout(home.path());
    let releases = releases(home.path());
    // Not named for its manifest, a foreign entrypoint, a digest that is not hex, no manifest,
    // a symlink, a plain file, and a hidden directory that is not ours.
    release_with(
        home.path(),
        &format!("0.150.0-{TARGET}"),
        "0.151.0",
        "bin/codex",
    );
    release_with(
        home.path(),
        &format!("0.152.0-{TARGET}"),
        "0.152.0",
        "bin/other",
    );
    release_with(
        home.path(),
        &format!("local-{}-{TARGET}", "z".repeat(64)),
        "0.152.0",
        "bin/codex",
    );
    std::fs::create_dir(releases.join("notes")).unwrap();
    std::fs::write(releases.join("notes/plan.txt"), "mine").unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), releases.join(format!("0.153.0-{TARGET}"))).unwrap();
    std::fs::write(releases.join("README"), "mine").unwrap();
    release_with(home.path(), ".retired.abc", "0.154.0", "bin/codex");

    let report = prune(home.path(), &FakeHost::quiet());

    assert_eq!(report.kept_all, None);
    assert_eq!(
        report.removed.len(),
        4,
        "the four vendor releases go: {report:?}"
    );
    let mut kept: Vec<String> = report.kept.iter().map(|(name, _)| name.clone()).collect();
    kept.sort();
    let mut expected = vec![
        format!("0.150.0-{TARGET}"),
        format!("0.152.0-{TARGET}"),
        format!("0.153.0-{TARGET}"),
        format!("local-{}-{TARGET}", "z".repeat(64)),
        ".retired.abc".to_string(),
        "README".to_string(),
        "notes".to_string(),
    ];
    expected.sort();
    assert_eq!(kept, expected);
    let mut left = expected;
    left.push(LOCAL.to_string());
    left.sort();
    assert_eq!(names(home.path()), left);
    assert!(
        outside.path().is_dir(),
        "a symlinked entry's target is untouched"
    );
}

#[test]
fn the_vendors_previous_selection_stays() {
    let home = home();
    vendor_layout(home.path());
    std::fs::write(
        package_root(home.path()).join("selection-history"),
        format!("{LOCAL}\n0.159.1-{TARGET}\n16777232 1234 1790809518.342987000\n"),
    )
    .unwrap();

    let report = prune(home.path(), &FakeHost::quiet());

    // It stays; the twin releases go.
    assert_eq!(
        report.kept,
        [(
            format!("0.159.1-{TARGET}"),
            "named in selection-history".to_string()
        )]
    );
    assert_eq!(
        report.removed,
        ["0.158.0", "0.159.0", "0.159.2"].map(|v| format!("{v}-{TARGET}"))
    );
    assert_eq!(
        names(home.path()),
        [format!("0.159.1-{TARGET}"), LOCAL.to_string()]
    );
}

#[test]
fn a_held_daemon_lock_keeps_everything() {
    let home = home();
    vendor_layout(home.path());

    // The daemon's lifecycle lock, held by another open of the file.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.path().join("app-server-daemon/daemon.lock"))
        .unwrap();
    // SAFETY: a plain flock(2) on a descriptor the test owns.
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let report = prune(home.path(), &FakeHost::quiet());
    assert_eq!(report.kept_all, busy(OPERATION_LOCK_HELD));
    drop(lock);

    // The installer's lock on `install.lock`, held as the shell installer holds it (`lockf(1)`
    // and `flock(1)` both take a flock(2) lock).
    let install = std::fs::OpenOptions::new()
        .write(true)
        .open(package_root(home.path()).join("install.lock"))
        .unwrap();
    // SAFETY: a plain flock(2) on a descriptor the test owns.
    assert_eq!(
        unsafe { libc::flock(install.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let report = prune(home.path(), &FakeHost::quiet());
    assert_eq!(report.kept_all, busy(INSTALLER_LOCK_HELD));
    assert_eq!(names(home.path()), all_vendor_names());
    drop(install);

    // The shell installer's directory lock.
    let dir_lock = package_root(home.path()).join("install.lock.d");
    std::fs::create_dir(&dir_lock).unwrap();
    let report = prune(home.path(), &FakeHost::quiet());
    assert_eq!(report.kept_all, busy(INSTALLER_DIRECTORY_LOCK_HELD));
    assert_eq!(names(home.path()), all_vendor_names());
    std::fs::remove_dir(&dir_lock).unwrap();

    // A migration's prepared release, not yet `current`.
    let migration = package_root(home.path()).join(".migration-current");
    std::os::unix::fs::symlink(format!("releases/0.159.2-{TARGET}"), &migration).unwrap();
    let report = prune(home.path(), &FakeHost::quiet());
    assert_eq!(report.kept_all, busy(MIGRATING));
    assert_eq!(names(home.path()), all_vendor_names());
    std::fs::remove_file(&migration).unwrap();

    // No daemon state at all is a doubt too.
    std::fs::remove_dir_all(home.path().join("app-server-daemon")).unwrap();
    assert_eq!(
        prune(home.path(), &FakeHost::quiet()).kept_all,
        doubt("no daemon state directory")
    );
    assert_eq!(names(home.path()), all_vendor_names());

    // The twin: with the locks free and no migration, the superseded releases go.
    std::fs::create_dir(home.path().join("app-server-daemon")).unwrap();
    assert_eq!(prune(home.path(), &FakeHost::quiet()).kept_all, None);
    assert_eq!(names(home.path()), [LOCAL]);
}

/// A mount point inside a release keeps that release, whatever its device says (a Linux bind
/// mount keeps its source's); one above the releases does not; a mount table that cannot be
/// read keeps everything.
#[test]
fn a_mount_inside_a_release_keeps_it() {
    let home = home();
    vendor_layout(home.path());
    let canonical = std::fs::canonicalize(releases(home.path())).unwrap();
    let inside = canonical.join(format!("0.158.0-{TARGET}/codex-path/mnt"));
    let mounted = FakeHost {
        mount_points: Some(vec![PathBuf::from("/"), canonical, inside.clone()]),
        ..FakeHost::quiet()
    };

    let report = prune(home.path(), &mounted);

    assert_eq!(report.kept_all, None);
    assert_eq!(
        report.kept,
        [(
            format!("0.158.0-{TARGET}"),
            format!("mount point {}", inside.display())
        )]
    );
    assert_eq!(
        report.removed,
        ["0.159.0", "0.159.1", "0.159.2"].map(|v| format!("{v}-{TARGET}"))
    );

    let no_table = FakeHost {
        mount_points: None,
        ..FakeHost::quiet()
    };
    assert_eq!(
        prune(home.path(), &no_table).kept_all,
        doubt("the mount table: no mount table")
    );
    assert_eq!(
        names(home.path()),
        [format!("0.158.0-{TARGET}"), LOCAL.to_string()]
    );

    // The twin: unmounted, it goes.
    assert_eq!(
        prune(home.path(), &FakeHost::quiet()).removed,
        [format!("0.158.0-{TARGET}")]
    );
    assert_eq!(names(home.path()), [LOCAL]);
}

/// Linux's mount table, as proc(5) writes it: the fifth field, octal-escaped; a line without
/// one is an error (the tidy then keeps everything), never a table without that mount.
#[test]
fn the_mount_table_is_read_as_the_kernel_writes_it() {
    let table = concat!(
        "22 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw\n",
        "36 22 8:1 /src /home/me/.codex/packages/app-server-daemon/releases/a\\040b\\134c\\x rw shared:2 - ext4 /dev/sda1 rw\n",
    );
    assert_eq!(
        mountinfo_points(table.as_bytes()).unwrap(),
        [
            PathBuf::from("/"),
            PathBuf::from("/home/me/.codex/packages/app-server-daemon/releases/a b\\c\\x"),
        ]
    );
    assert!(mountinfo_points(b"22 1 8:1 /\n").is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn the_live_mount_table_has_the_root() {
    assert!(mount_points().unwrap().contains(&PathBuf::from("/")));
}

#[test]
fn an_unfinished_retirement_is_finished() {
    let home = home();
    vendor_layout(home.path());
    let leftover_name = format!("{RETIRED_PREFIX}1.2.0.157.0-{TARGET}");
    let leftover = release_with(home.path(), &leftover_name, "0.157.0", "bin/codex");

    let report = prune(home.path(), &FakeHost::quiet());

    assert!(
        !leftover.exists(),
        "the leftover of an earlier rename is deleted"
    );
    let mut removed = ["0.158.0", "0.159.0", "0.159.1", "0.159.2"]
        .map(|v| format!("{v}-{TARGET}"))
        .to_vec();
    removed.push(leftover_name.clone());
    removed.sort();
    assert_eq!(report.removed, removed);
    assert_eq!(
        report.freed_bytes,
        ["0.157.0", "0.158.0", "0.159.0", "0.159.1", "0.159.2"]
            .map(release_bytes)
            .iter()
            .sum::<u64>()
    );
    assert_eq!(names(home.path()), [LOCAL]);

    // A leftover alone is something to do (and to say), not "nothing superseded".
    release_with(home.path(), &leftover_name, "0.157.0", "bin/codex");
    let report = prune(home.path(), &FakeHost::quiet());
    assert_eq!(report.kept_all, None);
    assert_eq!(report.removed, [leftover_name]);
    assert_eq!(names(home.path()), [LOCAL]);
}

#[test]
fn only_codex_alab_with_the_daemon_off_tidies() {
    assert!(should_tidy(
        /*active*/ true, /*daemon_auto_start*/ false
    ));
    assert!(!should_tidy(
        /*active*/ true, /*daemon_auto_start*/ true
    ));
    assert!(!should_tidy(
        /*active*/ false, /*daemon_auto_start*/ false
    ));
    assert!(!should_tidy(
        /*active*/ false, /*daemon_auto_start*/ true
    ));
}

#[test]
fn the_process_table_lists_this_test() {
    let executables = running_executables().unwrap();
    let me = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    assert!(
        executables
            .iter()
            .any(|exe| std::fs::canonicalize(exe).is_ok_and(|exe| exe == me)),
        "this test's own process is in the table"
    );
}

#[test]
fn tidy_removes_off_thread_in_codex_alab() {
    codex_alab_core::activate();
    let home = home();
    vendor_layout(home.path());

    let thread = tidy(home.path(), /*daemon_auto_start*/ false)
        .unwrap_or_else(|| panic!("codex-alab with the daemon off tidies"));

    assert_eq!(thread.thread().name(), Some("alab-daemon-packages"));
    thread.join().unwrap();
    assert_eq!(names(home.path()), [LOCAL]);
    assert!(
        tidy(home.path(), /*daemon_auto_start*/ true).is_none(),
        "the daemon on: no tidy"
    );
}
