// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
#[cfg(target_os = "linux")]
use std::ffi::OsString;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let parent =
            std::fs::canonicalize(std::env::temp_dir()).expect("physical temporary parent");
        for _ in 0..64 {
            let path = parent.join(format!("ad-{:016x}", rand::random::<u64>()));
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("scratch directory: {error}"),
            }
        }
        panic!("exclusive scratch unavailable");
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).expect("owned test directory");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn export(leaf: &Path, roots: &[(&str, &Path)], limits: Limits) -> PathBuf {
    export_to(
        leaf,
        roots,
        DiagnosticProducerState::ShutdownReturned,
        limits,
        ReadObserver::default(),
    )
    .expect("bounded export")
}

fn manifest(path: &Path) -> serde_json::Value {
    serde_json::from_slice(
        &std::fs::read(path.join(DIAGNOSTIC_MANIFEST_FILE)).expect("manifest file"),
    )
    .expect("manifest JSON")
}

#[test]
fn exports_regular_payloads_and_valid_unicode_names() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("home/nested");
    let root = source.parent().expect("home");
    std::fs::write(source.join("request.json"), b"actual fixture payload").unwrap();
    let name = "snowman-☃.log";
    std::fs::write(root.join(name), b"unicode name").unwrap();
    let output = export(&leaf, &[("home", root)], LIMITS);
    assert_eq!(
        std::fs::read(output.join("home/nested/request.json")).unwrap(),
        b"actual fixture payload"
    );
    assert_eq!(
        std::fs::read(output.join("home").join(name)).unwrap(),
        b"unicode name"
    );
    let record = manifest(&output);
    assert_eq!(record["producer_state"], "shutdown_returned");
    assert_eq!(record["copied_files"], 2);
    assert_eq!(record["copied_bytes"], 34);
    assert!(
        record["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path_hex"] == "736e6f776d616e2de298832e6c6f67")
    );
}

#[test]
fn raw_unix_path_bytes_are_lossless_in_manifest_encoding() {
    let raw = b"nested/\xff.log";
    let entry = Entry {
        root: "home".into(),
        path_hex: hex(raw),
        result: "copied",
        reason: None,
        errno: None,
        bytes: 0,
        truncated: false,
    };
    let encoded = serde_json::to_vec(&entry).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    let path = value["path_hex"].as_str().unwrap();
    assert_eq!(path, "6e65737465642fff2e6c6f67");
    let bytes: Vec<_> = path
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(bytes, raw);
}

// The maintained macOS protected lane rejected creation of this non-UTF8 name
// with EPERM before export. Do not claim native macOS coverage from a skipped
// filesystem error; its byte encoding is tested independently above.
#[cfg(target_os = "linux")]
#[test]
fn exports_a_native_linux_non_utf8_filename() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("source");
    let raw = OsString::from_vec(vec![b'l', 0xff, b'g']);
    std::fs::write(source.join(&raw), b"raw name").expect("Linux raw-name filesystem fixture");
    let output = export(&leaf, &[("home", &source)], LIMITS);
    assert_eq!(
        std::fs::read(output.join("home").join(raw)).unwrap(),
        b"raw name"
    );
    let record = manifest(&output);
    assert_eq!(record["copied_files"], 1);
    assert_eq!(record["copied_bytes"], 8);
    assert_eq!(record["entries"][0]["path_hex"], "6cff67");
}

#[test]
fn links_fifo_and_socket_are_omitted_without_reading_outside() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("source");
    let outside = scratch.dir("outside");
    std::fs::write(outside.join("secret"), b"outside must stay outside").unwrap();
    symlink(outside.join("secret"), source.join("file-link")).unwrap();
    symlink(&outside, source.join("directory-link")).unwrap();
    let fifo = c_name(source.join("pipe").as_os_str().as_bytes()).unwrap();
    // SAFETY: the owned test path is terminated; this creates only its fixture FIFO.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let _socket = UnixListener::bind(source.join("socket")).unwrap();
    let output = export(&leaf, &[("home", &source)], LIMITS);
    assert_eq!(std::fs::read_dir(output.join("home")).unwrap().count(), 0);
    let record = manifest(&output);
    assert_eq!(record["copied_files"], 0);
    assert_eq!(record["omitted"], 4);
    assert_eq!(
        std::fs::read(outside.join("secret")).unwrap(),
        b"outside must stay outside"
    );
}

#[test]
fn every_source_and_destination_ancestor_refuses_links() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let real = scratch.dir("outside/deep");
    std::fs::write(real.join("secret"), b"not exported").unwrap();
    let link = scratch.0.join("alias");
    symlink(real.parent().unwrap(), &link).unwrap();
    let good = scratch.dir("good");
    std::fs::write(good.join("kept"), b"kept").unwrap();
    let bad = link.join("deep");
    let output = export(&leaf, &[("bad", &bad), ("good", &good)], LIMITS);
    assert!(!output.join("bad").exists());
    assert_eq!(std::fs::read(output.join("good/kept")).unwrap(), b"kept");
    assert_eq!(manifest(&output)["errors"], 1);
    assert!(
        export_to(
            &bad,
            &[("good", &good)],
            DiagnosticProducerState::Incomplete,
            LIMITS,
            ReadObserver::default()
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(&real).unwrap().count(), 1);
}

#[test]
fn adversarial_aliases_and_parent_paths_never_create_an_export() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("source");
    for alias in [
        "",
        ".",
        "..",
        "../escape",
        "/escape",
        "with/slash",
        "with\0nul",
        "manifest.json",
    ] {
        assert!(
            export_to(
                &leaf,
                &[(alias, &source)],
                DiagnosticProducerState::Incomplete,
                LIMITS,
                ReadObserver::default()
            )
            .is_err(),
            "alias {alias:?}"
        );
    }
    assert!(
        export_to(
            &leaf,
            &[("same", &source), ("same", &source)],
            DiagnosticProducerState::Incomplete,
            LIMITS,
            ReadObserver::default()
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(&leaf).unwrap().count(), 0);
    let parent = source.join("../source");
    let output = export(&leaf, &[("bad", &parent), ("good", &source)], LIMITS);
    assert_eq!(manifest(&output)["errors"], 1);
    assert!(output.join("good").is_dir());
}

#[test]
fn destination_overlap_is_omitted_while_independent_roots_survive() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let nested = scratch.dir("leaf/source");
    let good = scratch.dir("good");
    std::fs::write(good.join("kept"), b"kept").unwrap();
    let output = export(
        &leaf,
        &[
            ("ancestor", &scratch.0),
            ("descendant", &nested),
            ("good", &good),
        ],
        LIMITS,
    );
    assert!(!output.join("ancestor").exists());
    assert!(!output.join("descendant").exists());
    assert_eq!(std::fs::read(output.join("good/kept")).unwrap(), b"kept");
    assert_eq!(manifest(&output)["omitted"], 2);
}

#[test]
fn live_growth_is_read_capped_and_marked_incomplete() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("source");
    let path = source.join("growing.log");
    std::fs::write(&path, b"original").unwrap();
    let limits = Limits {
        total_bytes: 8256,
        manifest_bytes: 8192,
        ..LIMITS
    };
    let hook = Box::new(move || {
        File::options()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[b'x'; 128])
            .unwrap();
    });
    let output = export_to(
        &leaf,
        &[("home", &source)],
        DiagnosticProducerState::ShutdownReturned,
        limits,
        ReadObserver {
            after_read: Some(hook),
        },
    )
    .unwrap();
    let record = manifest(&output);
    assert_eq!(record["read_bytes"], 64);
    assert_eq!(record["copied_bytes"], 64);
    assert_eq!(record["producer_state"], "incomplete");
    assert_eq!(record["stopped"], "byte_limit");
    assert_eq!(
        std::fs::metadata(output.join("home/growing.log"))
            .unwrap()
            .len(),
        64
    );
    let manifest_size = std::fs::metadata(output.join(DIAGNOSTIC_MANIFEST_FILE))
        .unwrap()
        .len();
    assert!(64 + manifest_size <= limits.total_bytes);
}

#[test]
fn file_entry_and_depth_caps_are_per_export() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("source");
    for name in ["one", "two", "three"] {
        std::fs::write(source.join(name), b"payload").unwrap();
    }
    let file_output = export(&leaf, &[("home", &source)], Limits { files: 1, ..LIMITS });
    assert_eq!(manifest(&file_output)["copied_files"], 1);
    assert_eq!(manifest(&file_output)["stopped"], "file_limit");
    let entry_output = export(
        &leaf,
        &[("home", &source)],
        Limits {
            entries: 3,
            ..LIMITS
        },
    );
    let record = manifest(&entry_output);
    assert_eq!(record["visited_entries"], 3);
    assert_eq!(record["stopped"], "entry_limit");
    let deep = scratch.dir("source/first/second");
    std::fs::write(deep.join("hidden"), b"too deep").unwrap();
    let depth_output = export(&leaf, &[("home", &source)], Limits { depth: 1, ..LIMITS });
    assert!(!depth_output.join("home/first/second").exists());
    assert!(
        manifest(&depth_output)["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["reason"] == "depth_limit")
    );
}

#[test]
fn source_path_replacement_does_not_redirect_open_directory_handles() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("source");
    std::fs::write(source.join("one"), b"original one").unwrap();
    std::fs::write(source.join("two"), b"original two").unwrap();
    let outside = scratch.dir("outside");
    std::fs::write(outside.join("secret"), b"never follow the replacement").unwrap();
    let moved = scratch.0.join("original-source");
    let source_path = source.clone();
    let mut replaced = false;
    let hook = Box::new(move || {
        if !replaced {
            std::fs::rename(&source_path, &moved).unwrap();
            symlink(&outside, &source_path).unwrap();
            replaced = true;
        }
    });
    let output = export_to(
        &leaf,
        &[("home", &source)],
        DiagnosticProducerState::Incomplete,
        LIMITS,
        ReadObserver {
            after_read: Some(hook),
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read(output.join("home/one")).unwrap(),
        b"original one"
    );
    assert_eq!(
        std::fs::read(output.join("home/two")).unwrap(),
        b"original two"
    );
    assert!(!output.join("home/secret").exists());
}

#[test]
fn each_export_is_exclusive_and_cannot_overwrite_an_earlier_one() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("source");
    std::fs::write(source.join("payload"), b"first").unwrap();
    let first = export(&leaf, &[("home", &source)], LIMITS);
    std::fs::write(source.join("payload"), b"second").unwrap();
    let second = export(&leaf, &[("home", &source)], LIMITS);
    assert_ne!(first, second);
    assert_eq!(std::fs::read(first.join("home/payload")).unwrap(), b"first");
    assert_eq!(
        std::fs::read(second.join("home/payload")).unwrap(),
        b"second"
    );
}

#[test]
fn eight_registered_roots_are_bounded_and_the_ninth_is_refused() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("source");
    std::fs::write(source.join("payload"), b"payload").unwrap();
    let aliases: Vec<_> = (0..9).map(|index| format!("root{index}")).collect();
    let roots: Vec<_> = aliases
        .iter()
        .map(|alias| (alias.as_str(), source.as_path()))
        .collect();
    let output = export(&leaf, &roots[..8], LIMITS);
    assert_eq!(manifest(&output)["copied_files"], 8);
    assert!(
        output.join("root3/payload").is_file(),
        "the optional fourth fixture root is retained"
    );
    assert!(
        export_to(
            &leaf,
            &roots,
            DiagnosticProducerState::Incomplete,
            LIMITS,
            ReadObserver::default()
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(&leaf).unwrap().count(), 1);
}

#[test]
fn metadata_exhaustion_stops_before_an_unrecorded_payload_is_created() {
    let scratch = Scratch::new();
    let leaf = scratch.dir("leaf");
    let source = scratch.dir("source");
    for index in 0..50 {
        std::fs::write(
            source.join(format!("entry-{index:03}-{}", "x".repeat(80))),
            b"payload",
        )
        .unwrap();
    }
    let limits = Limits {
        total_bytes: 16384,
        manifest_bytes: 8192,
        ..LIMITS
    };
    let output = export(&leaf, &[("home", &source)], limits);
    let record = manifest(&output);
    assert_eq!(record["stopped"], "manifest_limit");
    let actual_files = std::fs::read_dir(output.join("home")).unwrap().count();
    let details = record["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["result"] == "copied")
        .count();
    assert_eq!(actual_files, details);
    assert!(actual_files < 50);
    assert!(
        std::fs::metadata(output.join(DIAGNOSTIC_MANIFEST_FILE))
            .unwrap()
            .len()
            <= limits.manifest_bytes as u64
    );
}

#[test]
fn an_unassigned_runner_creates_no_diagnostic_destination() {
    const CHILD: &str = "ALAB_DIAGNOSTIC_ABSENCE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        assert!(
            export_fixture_diagnostics(&[], DiagnosticProducerState::Incomplete)
                .unwrap()
                .is_none()
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["test_support::fixture_diagnostics::tests::an_unassigned_runner_creates_no_diagnostic_destination", "--exact", "--nocapture"])
        .env(CHILD, "1")
        .env_remove("ALAB_UNIT_DIAGNOSTICS_DIR")
        .output().unwrap();
    assert!(
        output.status.success(),
        "isolated absence probe: {output:?}"
    );
}
