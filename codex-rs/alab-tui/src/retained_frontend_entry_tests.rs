// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Selector DATA only. No EnteredRetainedFrontend/Controller/Native receipt is
//! fabricated, and parsing a marker is never called a successful Native entry.
use super::*;
use std::os::unix::ffi::OsStringExt;

#[test]
fn normal_invocation_does_not_select_private_frontend_entry() -> io::Result<()> {
    assert!(select(vec!["codex".into(), "resume".into()])?.is_none());
    Ok(())
}
#[test]
fn selected_tail_preserves_original_cli_and_non_utf8_bytes_exactly() -> io::Result<()> {
    let tail = vec![
        OsString::from("resume"),
        OsString::from("--cd"),
        OsString::from_vec(vec![b'/', 0xff]),
    ];
    let raw = [
        vec![
            "/real/codex".into(),
            MARKER.into(),
            "a".repeat(32).into(),
            "b".repeat(32).into(),
        ],
        tail.clone(),
    ]
    .concat();
    let selected = select(raw)?.ok_or_else(unavailable)?;
    assert_eq!(
        selected.original,
        [vec![OsString::from("/real/codex")], tail].concat()
    );
    assert_eq!(selected.go, "a".repeat(32));
    assert_eq!(selected.service, "b".repeat(32));
    Ok(())
}
#[test]
fn malformed_private_prefix_cannot_choose_ordinary_source_seed_fallback() {
    for bad in [
        vec!["codex".into(), MARKER.into()],
        vec!["codex".into(), MARKER.into(), "a".repeat(32).into()],
        vec![
            "codex".into(),
            MARKER.into(),
            "A".repeat(32).into(),
            "b".repeat(32).into(),
        ],
        vec![
            "codex".into(),
            MARKER.into(),
            "a".repeat(32).into(),
            "b".repeat(31).into(),
        ],
    ] {
        assert!(select(bad).is_err());
    }
}
