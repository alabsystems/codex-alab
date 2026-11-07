// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The mount table and the local resolution ([`crate::mounts`]), as aterm main 240d2cb43c
//! tests them (`aterm-sysprobe/src/tests.rs`, `harness/disk_idle_tests.rs`), unix rule only;
//! and the automounted folder a link leads onto (macOS's `/home`), refused untouched.

use std::path::Path;
use std::path::PathBuf;

use crate::mounts::*;

fn mount(on: &str, fstype: &str, local: bool) -> Mount {
    Mount {
        on: PathBuf::from(on),
        fstype: fstype.to_string(),
        local,
        automounted: fstype == "autofs",
    }
}

fn mac_table() -> Table {
    Table {
        mounts: vec![
            mount("/", "apfs", true),
            mount("/System/Volumes/Data", "apfs", true),
            mount("/System/Volumes/Data/home", "autofs", false),
            mount("/Volumes/Recovery", "apfs", true),
        ],
        complete: true,
        firmlinks: parse_firmlinks("/homes\tUsers\n/private\tprivate\n/Volumes\tVolumes\n"),
    }
}

/// A firmlinked path is read in its data-volume spelling: `~` is on the data volume, not on
/// the sealed root, and a path under `/System/Volumes/Data` stays there. The longest prefix
/// wins, component by component (`/Volumes/Recoveryx` is not on `/Volumes/Recovery`).
/// NEGATIVE CONTROL: with no firmlinks the same `~` reads as the root's.
#[test]
fn a_path_finds_its_mount_through_the_firmlinks() {
    let t = mac_table();
    let on = |p: &str| mount_of(Path::new(p), &t).map(|m| m.on.clone());
    let data = Some(PathBuf::from("/System/Volumes/Data"));
    assert_eq!(on("/homes/jsmith/aterm"), data);
    assert_eq!(on("/private/tmp/x"), data);
    assert_eq!(on("/System/Volumes/Data/homes/x"), data);
    assert_eq!(on("/home/x"), on("/"), "/home is not a firmlink here");
    assert_eq!(
        on("/System/Volumes/Data/home/x"),
        Some(PathBuf::from("/System/Volumes/Data/home"))
    );
    assert_eq!(
        on("/Volumes/Recovery/x"),
        Some(PathBuf::from("/Volumes/Recovery"))
    );
    assert_eq!(on("/Volumes/Recoveryx"), data, "a component, not a prefix");
    assert_eq!(on("relative/path"), None);
    assert_eq!(on("/homes/../etc"), None, "`..` is never placed");
    let bare = Table {
        firmlinks: Vec::new(),
        ..mac_table()
    };
    assert_eq!(
        mount_of(Path::new("/homes/jsmith"), &bare).map(|m| m.on.clone()),
        Some(PathBuf::from("/"))
    );
}

/// The mountinfo parser: `\040` decoded, the optional fields skipped up to `-`, an autofs
/// mount automounted and a network one not local, and a torn last line dropped, which, like a
/// read cut at the cap, makes the table incomplete. NEGATIVE CONTROL: the same text whole is
/// complete.
#[test]
fn the_mountinfo_parser_reads_escapes_and_marks_a_cut_table() {
    let text = "\
22 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
23 22 0:21 / /mnt/my\\040disk ro,nosuid shared:7 master:2 - xfs /dev/sdb1 ro
24 22 0:44 / /net rw - autofs systemd-1 rw
25 22 0:50 / /srv/nfs rw - nfs4 host:/x rw
";
    let t = parse_mountinfo(text, false);
    assert!(t.complete);
    let ons: Vec<&Path> = t.mounts.iter().map(|m| m.on.as_path()).collect();
    assert_eq!(
        ons,
        [
            Path::new("/"),
            Path::new("/mnt/my disk"),
            Path::new("/net"),
            Path::new("/srv/nfs")
        ]
    );
    assert!(t.mounts[0].local);
    assert!(t.mounts[1].local && t.mounts[1].fstype == "xfs");
    assert!(t.mounts[2].automounted && !t.mounts[2].local);
    assert!(!t.mounts[3].local, "nfs is remote");
    let torn = &text[..text.len() - 10];
    let t = parse_mountinfo(torn, false);
    assert!(!t.complete, "a torn last line");
    assert_eq!(t.mounts.len(), 3);
    assert!(!parse_mountinfo(text, true).complete, "cut at the cap");
    let many: String = (0..=MAX_MOUNTS)
        .map(|i| format!("{i} 1 0:1 / /m{i} rw - ext4 /dev/x rw\n"))
        .collect();
    let t = parse_mountinfo(&many, false);
    assert!(!t.complete && t.mounts.len() == MAX_MOUNTS);
}

#[test]
fn the_firmlinks_file_is_read_line_by_line() {
    let f = parse_firmlinks("/homes\tUsers\nbad line\nrel\tx\n/opt\topt\n");
    assert_eq!(
        f,
        [
            (
                PathBuf::from("/homes"),
                PathBuf::from("/System/Volumes/Data/Users")
            ),
            (
                PathBuf::from("/opt"),
                PathBuf::from("/System/Volumes/Data/opt")
            ),
        ]
    );
}

/// The machine's own table is read whole, and the scratch folder lies on a local mount of it
/// (macOS and Linux; elsewhere nothing is read, and nothing resolves).
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn the_live_table_is_whole_and_places_a_scratch_folder_locally() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let t = table();
    assert!(t.complete, "{t:?}");
    let base = std::fs::canonicalize(tmp.path()).unwrap_or_else(|err| panic!("canon: {err}"));
    assert!(
        mount_of(&base, &t).is_some_and(|m| m.local && !m.automounted),
        "{t:?}"
    );
    assert_eq!(resolve_local(tmp.path(), &t), Some(base));
}

/// Links are resolved ONE COMPONENT AT A TIME, and only on local, non-automounted mounts of
/// a table read whole: a link chain resolves; the same chain where the table calls its
/// volume remote (a stalled share) or automounted is never followed (`None`), nor is anything
/// on a table read short.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn links_are_resolved_only_on_local_mounts() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let base = std::fs::canonicalize(tmp.path()).unwrap_or_else(|err| panic!("canon: {err}"));
    mkdir(&base.join("real/bin"));
    std::fs::write(base.join("real/bin/tool"), b"x").unwrap_or_else(|err| panic!("tool: {err}"));
    link("real", &base.join("via"));
    mkdir(&base.join("links"));
    link(base.join("via/bin/tool"), &base.join("links/tool"));
    let table = table();
    assert_eq!(
        resolve_local(&base.join("links/tool"), &table),
        Some(base.join("real/bin/tool"))
    );
    let on = mount_of(&base, &table)
        .unwrap_or_else(|| panic!("a mount holds it"))
        .on
        .clone();
    for remote in [true, false] {
        let mut t = table.clone();
        for m in &mut t.mounts {
            if m.on == on {
                if remote {
                    m.local = false;
                } else {
                    m.automounted = true;
                }
            }
        }
        assert_eq!(
            resolve_local(&base.join("links/tool"), &t),
            None,
            "remote={remote}"
        );
    }
    let cut = Table {
        complete: false,
        ..table.clone()
    };
    assert_eq!(resolve_local(&base.join("links/tool"), &cut), None);
    assert_eq!(resolve_local(Path::new("relative"), &table), None);
    // A loop of links ends at the hop limit.
    link("loop-b", &base.join("loop-a"));
    link("loop-a", &base.join("loop-b"));
    assert_eq!(resolve_local(&base.join("loop-a"), &table), None);
}

/// A LINK ONTO AN AUTOMOUNTED FOLDER IS REFUSED UNTOUCHED (parity review P2; macOS's `/home`
/// is a link to `/System/Volumes/Data/home`, an autofs mount): the walk places each step
/// before it reads it, so the step onto the automounted folder is refused before anything
/// there is looked at, and a folder reached through such a link never resolves. NEGATIVE
/// CONTROL: the same folder on a local mount resolves to where the link leads.
#[cfg(unix)]
#[test]
fn a_link_onto_an_automounted_folder_is_refused_untouched() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let base = std::fs::canonicalize(tmp.path()).unwrap_or_else(|err| panic!("canon: {err}"));
    mkdir(&base.join("auto/me/repo"));
    link(base.join("auto"), &base.join("home"));
    let auto = base.join("auto").display().to_string();
    let t = |automounted: bool| Table {
        mounts: vec![
            mount("/", "apfs", true),
            Mount {
                automounted,
                ..mount(&auto, if automounted { "autofs" } else { "apfs" }, true)
            },
        ],
        complete: true,
        firmlinks: Vec::new(),
    };
    assert_eq!(resolve_local(&base.join("home/me/repo"), &t(true)), None);
    assert_eq!(resolve_local(&base.join("auto/me/repo"), &t(true)), None);
    // NEGATIVE CONTROL: local.
    assert_eq!(
        resolve_local(&base.join("home/me/repo"), &t(false)),
        Some(base.join("auto/me/repo"))
    );
}

/// A walk ends only on a local folder ([`resolve_local`]): `/` where the table calls its
/// mount remote holds no component for a step to place, and resolves to nothing. NEGATIVE
/// CONTROL: a local root resolves to itself. From the table alone: nothing is opened.
#[test]
fn a_walk_that_ends_on_a_remote_root_resolves_to_nothing() {
    let root = |local: bool| Table {
        mounts: vec![mount("/", if local { "ext4" } else { "nfs" }, local)],
        complete: true,
        firmlinks: Vec::new(),
    };
    assert_eq!(resolve_local(Path::new("/"), &root(false)), None);
    assert_eq!(
        resolve_local(Path::new("/"), &root(true)),
        Some(PathBuf::from("/"))
    );
}

fn mkdir(path: &Path) {
    std::fs::create_dir_all(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
}

#[cfg(unix)]
fn link(to: impl AsRef<Path>, at: &Path) {
    std::os::unix::fs::symlink(to, at).unwrap_or_else(|err| panic!("{}: {err}", at.display()));
}
