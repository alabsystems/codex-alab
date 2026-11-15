// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Codex-alab's kept refusals ([`super::Refusals`]) in a scratch `CODEX_HOME`: fake token
//! strings only, never a real credential.

use std::time::Duration;
use std::time::SystemTime;

use codex_protocol::auth::RefreshTokenFailedReason;
use pretty_assertions::assert_eq;

use super::KEPT;
use super::Refusals;

/// A fake refresh token, unlike any real one.
const TOKEN: &str = "fixture-refresh-token-not-a-credential";

/// Every file in the vetoes' folder, by name.
fn files(refusals: &Refusals) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(refusals.dir())
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Set `name`'s modification time in the vetoes' folder to `ago` before now.
fn age(refusals: &Refusals, name: &str, ago: Duration) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(refusals.dir().join(name))
        .unwrap_or_else(|err| panic!("{err}"));
    file.set_modified(SystemTime::now() - ago)
        .unwrap_or_else(|err| panic!("{err}"));
}

/// A KEPT REFUSAL OUTLIVES ITS PROCESS: a veto written by one store is read by a fresh one over
/// the same home (a restart), with its reason. NEGATIVE CONTROL: before the write, none.
#[test]
fn a_kept_refusal_is_read_again_after_a_restart() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let before = Refusals::under(home.path());
    assert_eq!(before.refused(TOKEN), None);
    for (token, reason) in [
        ("token-revoked", RefreshTokenFailedReason::Revoked),
        ("token-reused", RefreshTokenFailedReason::Exhausted),
        ("token-other", RefreshTokenFailedReason::Other),
    ] {
        assert!(
            before
                .keep(token, reason)
                .unwrap_or_else(|err| panic!("{err}"))
        );
        let after = Refusals::under(home.path());
        assert_eq!(after.refused(token), Some(reason), "{token}");
    }
    assert_eq!(
        Refusals::under(home.path()).dir(),
        home.path().join("alab").join("login-refusals")
    );
}

/// A VETO NAMES ONE TOKEN: another refresh token (the login renewed, or another login) reads
/// none, and an empty token has none and keeps none.
#[test]
fn another_refresh_token_reads_no_veto() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let refusals = Refusals::under(home.path());
    assert!(
        refusals
            .keep(TOKEN, RefreshTokenFailedReason::Revoked)
            .unwrap_or_else(|err| panic!("{err}"))
    );
    assert_eq!(refusals.refused("another-fixture-token"), None);
    assert_eq!(refusals.refused(""), None);
    assert!(
        !refusals
            .keep("", RefreshTokenFailedReason::Revoked)
            .unwrap_or_else(|err| panic!("{err}"))
    );
    assert_eq!(files(&refusals).len(), 1);
}

/// `EXPIRED` IS NEVER KEPT: it is the one ordinary end, which aterm's record decides. No file
/// is written, not even the folder.
#[test]
fn an_expired_refusal_is_never_kept() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let refusals = Refusals::under(home.path());
    assert!(
        !refusals
            .keep(TOKEN, RefreshTokenFailedReason::Expired)
            .unwrap_or_else(|err| panic!("{err}"))
    );
    assert_eq!(refusals.refused(TOKEN), None);
    assert!(!refusals.dir().exists());
}

/// THE FIRST REFUSAL STANDS (create-new): a second write for the same token writes nothing and
/// leaves the first reason and file as they were, however often it is asked.
#[test]
fn keeping_a_refusal_again_changes_nothing() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let refusals = Refusals::under(home.path());
    assert!(
        refusals
            .keep(TOKEN, RefreshTokenFailedReason::Revoked)
            .unwrap_or_else(|err| panic!("{err}"))
    );
    let path = refusals.dir().join(Refusals::name(TOKEN));
    let first = std::fs::read(&path).unwrap_or_else(|err| panic!("{err}"));
    for reason in [
        RefreshTokenFailedReason::Revoked,
        RefreshTokenFailedReason::Exhausted,
        RefreshTokenFailedReason::Other,
        RefreshTokenFailedReason::Expired,
    ] {
        assert!(
            !refusals
                .keep(TOKEN, reason)
                .unwrap_or_else(|err| panic!("{err}"))
        );
    }
    assert_eq!(
        std::fs::read(&path).unwrap_or_else(|err| panic!("{err}")),
        first
    );
    assert_eq!(
        refusals.refused(TOKEN),
        Some(RefreshTokenFailedReason::Revoked)
    );
    assert_eq!(
        files(&refusals),
        vec![Refusals::name(TOKEN)],
        "no temporary is left"
    );
}

/// A WRITE PRUNES THE VETOES OLDER THAN 30 DAYS, and only those: a veto 31 days old and a
/// leftover temporary of that age go, one 29 days old stays, and so does a file that is none of
/// this store's.
#[test]
fn a_write_prunes_vetoes_older_than_thirty_days() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let refusals = Refusals::under(home.path());
    let day = Duration::from_secs(24 * 60 * 60);
    assert_eq!(KEPT, 30 * day);
    for token in ["old-token", "recent-token"] {
        assert!(
            refusals
                .keep(token, RefreshTokenFailedReason::Revoked)
                .unwrap_or_else(|err| panic!("{err}"))
        );
    }
    let leftover = format!(".{}.1.2.tmp", Refusals::name("crashed"));
    std::fs::write(refusals.dir().join(&leftover), "revoked\n")
        .unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(refusals.dir().join("README"), "not a veto\n")
        .unwrap_or_else(|err| panic!("{err}"));
    age(&refusals, &Refusals::name("old-token"), 31 * day);
    age(&refusals, &leftover, 31 * day);
    age(&refusals, "README", 31 * day);
    age(&refusals, &Refusals::name("recent-token"), 29 * day);
    assert_eq!(
        refusals.refused("old-token"),
        Some(RefreshTokenFailedReason::Revoked),
        "a read prunes nothing"
    );
    assert!(
        refusals
            .keep(TOKEN, RefreshTokenFailedReason::Other)
            .unwrap_or_else(|err| panic!("{err}"))
    );
    assert_eq!(refusals.refused("old-token"), None);
    assert_eq!(
        refusals.refused("recent-token"),
        Some(RefreshTokenFailedReason::Revoked)
    );
    let mut expected = vec![
        Refusals::name(TOKEN),
        Refusals::name("recent-token"),
        "README".to_owned(),
    ];
    expected.sort();
    assert_eq!(files(&refusals), expected);
}

/// NO TOKEN IS KEPT: the file's name is the domain-separated digest (64 hex digits, not the
/// token's bare SHA-256), and no file's name or content holds the token.
#[test]
fn the_file_is_named_by_the_digest_and_holds_no_token() {
    use sha2::Digest as _;
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let refusals = Refusals::under(home.path());
    assert!(
        refusals
            .keep(TOKEN, RefreshTokenFailedReason::Exhausted)
            .unwrap_or_else(|err| panic!("{err}"))
    );
    let name = Refusals::name(TOKEN);
    assert_eq!(name.len(), 64);
    assert!(name.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(
        name,
        format!("{:x}", sha2::Sha256::digest(TOKEN.as_bytes()))
    );
    let mut domain = b"alab/login-refusal/v1\0".to_vec();
    domain.extend_from_slice(TOKEN.as_bytes());
    assert_eq!(name, format!("{:x}", sha2::Sha256::digest(&domain)));
    assert_eq!(files(&refusals), vec![name.clone()]);
    let content =
        std::fs::read_to_string(refusals.dir().join(&name)).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(content, "exhausted\n");
    assert!(!content.contains(TOKEN) && !name.contains(TOKEN));
}

/// A VETO THIS READER CANNOT READ STILL VETOES: an unknown word is `other`, never `expired`.
#[test]
fn an_unknown_word_is_still_a_veto() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let refusals = Refusals::under(home.path());
    std::fs::create_dir_all(refusals.dir()).unwrap_or_else(|err| panic!("{err}"));
    for word in ["expired\n", "", "something-newer\n"] {
        std::fs::write(refusals.dir().join(Refusals::name(TOKEN)), word)
            .unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(
            refusals.refused(TOKEN),
            Some(RefreshTokenFailedReason::Other),
            "{word:?}"
        );
    }
}
