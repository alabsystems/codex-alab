// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use pretty_assertions::assert_eq;

use super::*;

/// A stand-in `aterm` that runs `body` as a shell script.
fn program(dir: &tempfile::TempDir, body: &str) -> PathBuf {
    let path = dir.path().join("aterm");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("mode");
    path
}

/// The listing as `aterm secrets ls --json` prints it: names, pointers and descriptions only.
const LISTING: &str = r#"{"entries":[
  {"description":"the notary credential","name":"release-notary","pointer":"~/.secrets/release-notary"},
  {"description":"metadata for that token (no secret)","name":"aterm-claude-token-ac-1-meta","pointer":null}
]}"#;

#[test]
fn the_listing_reads_names_pointers_and_purposes() {
    let entries = parse_listing(LISTING.as_bytes()).expect("listing");
    assert_eq!(
        entries,
        vec![
            Entry {
                name: "release-notary".into(),
                pointer: Some("~/.secrets/release-notary".into()),
                purpose: "the notary credential".into(),
            },
            Entry {
                name: "aterm-claude-token-ac-1-meta".into(),
                pointer: None,
                purpose: "metadata for that token (no secret)".into(),
            },
        ]
    );
    assert!(parse_listing(b"release-notary\n").is_err());
}

#[tokio::test]
async fn the_registry_is_read_through_the_cli() {
    let dir = tempfile::tempdir().expect("dir");
    let args = dir.path().join("args");
    let aterm = program(
        &dir,
        &format!(
            "echo \"$@\" >> '{}'\nprintf '%s\\n' '{LISTING}'",
            args.display()
        ),
    );
    let entries = registry(&aterm).await.expect("registry");
    assert_eq!(entries.len(), 2);
    assert_eq!(
        std::fs::read_to_string(&args).expect("args"),
        "secrets ls --json\n"
    );
}

#[tokio::test]
async fn a_need_is_filed_by_hand_and_a_failure_says_why() {
    let dir = tempfile::tempdir().expect("dir");
    let args = dir.path().join("args");
    let aterm = program(&dir, &format!("echo \"$@\" >> '{}'", args.display()));
    need(&aterm, "gh:dev").await.expect("filed");
    assert_eq!(
        std::fs::read_to_string(&args).expect("args"),
        "secrets need gh:dev\n"
    );

    let refused = program(&dir, "echo 'need ledger locked' >&2\nexit 3");
    assert_eq!(
        need(&refused, "notary").await,
        Err("`aterm secrets need notary`: exit 3: need ledger locked".to_string())
    );
    let missing = dir.path().join("no-such-aterm");
    assert!(
        need(&missing, "notary")
            .await
            .is_err_and(|why| why.contains("did not start"))
    );
}
