// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A startup sweep may retire only the dead snapshot it actually judged.

use crate::record;
use pretty_assertions::assert_eq;
use std::cell::Cell;
use std::path::Path;
use std::path::PathBuf;

const PID: u32 = 999_995;
const STARTED: u64 = 10_000;

fn candidate(dir: &Path, temporary: bool) -> (PathBuf, record::Record) {
    super::stale_record(dir, PID, None, STARTED - 1);
    let canonical = record::path_for(dir, PID);
    let source = record::read(&canonical).expect("dead source record");
    let path = if temporary {
        let path = dir.join(format!("{PID}.json.interrupted.tmp"));
        std::fs::rename(canonical, &path).expect("interrupted publication");
        path
    } else {
        canonical
    };
    (path, source)
}

fn replace(dir: &Path, path: &Path, replacement: &record::Record) {
    record::write_atomic(dir, replacement).expect("concurrent successor publication");
    let canonical = record::path_for(dir, replacement.pid);
    if canonical != path {
        std::fs::rename(canonical, path).expect("concurrent temporary publication");
    }
}

#[test]
fn start_sweep_preserves_a_record_replaced_after_the_verdict_snapshot() {
    for temporary in [false, true] {
        for verdict in [
            record::Sweep::Hold,
            record::Sweep::Take,
            record::Sweep::Remove,
        ] {
            let home = tempfile::tempdir().expect("scratch records");
            let (path, source) = candidate(home.path(), temporary);
            let mut replacement = source.clone();
            replacement.updated_at = STARTED + 1;
            replacement.session_id = Some("new-session".into());
            let mut taken = Vec::new();
            let removed = record::sweep_with(
                home.path(),
                std::process::id(),
                STARTED,
                |_, _| false,
                |swept| {
                    assert_eq!(swept, &source);
                    replace(home.path(), &path, &replacement);
                    verdict
                },
                |_, swept| taken.push(swept.clone()),
            );
            assert_eq!(removed, 0, "temporary={temporary}, verdict={verdict:?}");
            assert_eq!(record::read(&path), Some(replacement));
            assert!(taken.is_empty(), "a displaced snapshot grants no carry-on");
        }
    }
}

#[test]
fn start_sweep_rechecks_liveness_before_retiring_a_judged_record() {
    for verdict in [
        record::Sweep::Hold,
        record::Sweep::Take,
        record::Sweep::Remove,
    ] {
        let home = tempfile::tempdir().expect("scratch records");
        let (path, source) = candidate(home.path(), false);
        let live = Cell::new(false);
        let mut taken = Vec::new();
        let removed = record::sweep_with(
            home.path(),
            std::process::id(),
            STARTED,
            |_, _| live.get(),
            |_| {
                live.set(true);
                verdict
            },
            |_, swept| taken.push(swept.clone()),
        );
        assert_eq!(removed, 0, "verdict={verdict:?}");
        assert_eq!(record::read(&path), Some(source));
        assert!(taken.is_empty());
    }
}

#[test]
fn start_sweep_preserves_atomic_republication_of_the_same_known_fields() {
    for verdict in [
        record::Sweep::Hold,
        record::Sweep::Take,
        record::Sweep::Remove,
    ] {
        let home = tempfile::tempdir().expect("scratch records");
        let (path, source) = candidate(home.path(), false);
        let mut taken = Vec::new();
        let removed = record::sweep_with(
            home.path(),
            std::process::id(),
            STARTED,
            |_, _| false,
            |_| {
                // Known-field equality cannot establish that this is the same
                // publication; future readers may own fields this build ignores.
                let mut value = serde_json::to_value(&source).expect("source fields");
                value["futureCustody"] = "new owner".into();
                let replacement = home.path().join("replacement");
                std::fs::write(&replacement, serde_json::to_vec(&value).unwrap())
                    .expect("future writer bytes");
                std::fs::rename(replacement, &path).expect("atomic republication");
                verdict
            },
            |_, swept| taken.push(swept.clone()),
        );
        assert_eq!(removed, 0, "verdict={verdict:?}");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("replacement survives"))
                .expect("future writer fields");
        assert_eq!(value["futureCustody"], "new owner");
        assert!(taken.is_empty());
    }
}

#[test]
fn start_sweep_preserves_a_valid_replacement_of_an_unreadable_candidate() {
    for temporary in [false, true] {
        let home = tempfile::tempdir().expect("scratch records");
        let (path, source) = candidate(home.path(), temporary);
        std::fs::write(&path, "partial record").expect("interrupted record bytes");
        let replaced = Cell::new(false);
        let mut taken = Vec::new();
        let removed = record::sweep_with(
            home.path(),
            std::process::id(),
            codex_alab_core::session::now_ms() + 60_000,
            |_, _| {
                if !replaced.replace(true) {
                    replace(home.path(), &path, &source);
                }
                false
            },
            |_| panic!("malformed snapshot grants no verdict"),
            |_, swept| taken.push(swept.clone()),
        );
        assert!(replaced.get());
        assert_eq!(removed, 0, "temporary={temporary}");
        assert_eq!(record::read(&path), Some(source));
        assert!(taken.is_empty());
    }
}

#[test]
fn start_sweep_hold_keeps_both_publications_when_restore_finds_a_new_canonical_record() {
    let home = tempfile::tempdir().expect("scratch records");
    let (path, source) = candidate(home.path(), false);
    let mut replacement = source.clone();
    replacement.updated_at += 1;
    replacement.session_id = Some("concurrent-successor".into());
    let liveness_reads = Cell::new(0);
    let mut taken = Vec::new();
    let removed = record::sweep_with(
        home.path(),
        std::process::id(),
        STARTED,
        |_, _| {
            liveness_reads.set(liveness_reads.get() + 1);
            if liveness_reads.get() == 2 {
                // The old source has passed metadata/content validation inside
                // quarantine. Its original slot belongs to this newer writer.
                assert!(!path.exists());
                replace(home.path(), &path, &replacement);
            }
            false
        },
        |_| record::Sweep::Hold,
        |_, swept| taken.push(swept.clone()),
    );
    assert_eq!(liveness_reads.get(), 2);
    assert_eq!(removed, 0);
    assert!(taken.is_empty(), "an unrestored Hold supplies no handoff");
    assert_eq!(record::read(&path), Some(replacement));
    let retained = std::fs::read_dir(home.path())
        .expect("custody census")
        .flatten()
        .filter(|entry| record::read(&entry.path()).as_ref() == Some(&source))
        .collect::<Vec<_>>();
    assert_eq!(retained.len(), 1, "captured source stays in one quarantine");
    assert_ne!(retained[0].path(), path);
}
