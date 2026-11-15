// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use pretty_assertions::assert_eq;

use super::*;

fn entry(name: &str, pointer: &str, purpose: &str) -> VaultEntry {
    VaultEntry {
        name: name.to_owned(),
        pointer: Some(pointer.to_owned()),
        purpose: purpose.to_owned(),
    }
}

fn lines(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| (*l).to_owned()).collect()
}

/// The release machine's registry on 2026-11-12, as the host resolves the incident's wants.
fn held_notary() -> CredentialRead {
    CredentialRead::Resolved(vec![(
        "notary".to_owned(),
        Resolution::Held(vec![entry(
            "release-notary",
            "~/.secrets/release-notary",
            "the notary credential",
        )]),
    )])
}

/// THE INCIDENT'S LINES ARE CREDENTIAL LINES, each with the want codex-alab resolves (aterm's
/// table, verbatim).
#[test]
fn credential_lines_name_their_wants() {
    let table: &[(&str, &[&str])] = &[
        ("the release credential", &[RELEASE_WANT]),
        (
            "the owner's release credential to publish 0.200.0",
            &[RELEASE_WANT],
        ),
        ("the notary credential", &["notary"]),
        ("an app-specific password for notarytool", &["notary"]),
        ("the Developer ID signing certificate", &[RELEASE_WANT]),
        ("a GitHub token with repo scope", &["gh:dev"]),
        ("the release GitHub token", &["gh:release"]),
        ("the site deployer's key", &["site-deployer-key"]),
        ("the Anthropic API key", &[]),
        ("the SSH key", &[]),
        ("the private key", &[]),
        ("the signing key", &[RELEASE_WANT]),
        ("the codesign key", &[RELEASE_WANT]),
        ("a GitHub key with repo scope", &["gh:dev"]),
        ("the service account key", &["site-deployer-key"]),
        ("the deployment keys", &["site-deployer-key"]),
        ("the mailbox password for ops", &[]),
    ];
    for (line, wants) in table {
        assert_eq!(wants_of(line).as_deref(), Some(*wants), "{line}");
    }
}

/// NEGATIVE CONTROLS: a person's decision, a grant, hardware, a release decision and a vendor
/// sign-in at a window are no credential line, so they stay a person's need.
#[test]
fn a_persons_need_is_no_credential_line() {
    for line in [
        "approve the 0.200.0 release",
        "decide whether to publish 1.5.0 to the public tier",
        "decide whether to cut the 1.5.0 release",
        "a decision on the signing policy",
        "the Full Disk Access grant",
        "plug in the YubiKey",
        "press the Caps Lock key to verify the mapping",
        "hold the Control key",
        "press the key to release the modifier",
        "touch the security key to sign in",
        "touch the security key while an API request waits",
        "confirm the shortcut key for deployment",
        "choose the primary key for this database",
        "a person's sign-in to Claude Code in a window",
        "approve the codex sign-in for ops: run `aterm harness accounts login ops`",
    ] {
        assert_eq!(wants_of(line), None, "{line}");
        assert_eq!(
            crate::needs::persons_needs(&format!("NEEDS YOU: {line}")),
            vec![line.to_owned()],
            "a real person's need stays visible: {line}"
        );
    }
}

/// A CREDENTIAL LINE IS NO PERSON'S NEED: the grammar still reads it ([`crate::needs::needs_you`]),
/// the person's list leaves it out, and it is the credential path's.
#[test]
fn a_credential_line_leaves_the_persons_list() {
    let report = "NEEDS YOU:\n- the notary credential\n- approve the 0.200.0 release";
    assert_eq!(
        crate::needs::needs_you(report),
        ["the notary credential", "approve the 0.200.0 release"]
    );
    assert_eq!(
        crate::needs::persons_needs(report),
        ["approve the 0.200.0 release"]
    );
    assert_eq!(
        crate::needs::credential_needs(report),
        ["the notary credential"]
    );
}

/// FOUND: the line names the entry with its pointer and purpose, and the release machine for a
/// release need; nothing is filed for a want the vault holds.
#[test]
fn a_held_want_is_named_with_its_pointer() {
    let named = lines(&["the notary credential to cut the release"]);
    let (text, keys) = told_line(&named, &held_notary(), &BTreeSet::new()).expect("told");
    assert!(text.starts_with(CREDENTIAL_HEAD), "{text}");
    assert!(
        text.contains("`release-notary` at `~/.secrets/release-notary` (the notary credential)"),
        "{text}"
    );
    assert!(text.contains(RELEASE_MACHINE), "{text}");
    assert!(!text.contains("filed"), "{text}");
    assert!(text.ends_with(CREDENTIAL_TAIL), "{text}");
    assert_eq!(keys, ["held:release-notary", RELEASE_WANT]);
}

/// MISSING: a want the vault does not hold was filed, and the line says so; one codex-alab could
/// not file says why; one not resolved, or the vocabulary cannot name, is the worker's to file.
#[test]
fn a_missing_want_is_filed_never_asked_of_a_person() {
    let read = CredentialRead::Resolved(vec![
        ("gh:dev".to_owned(), Resolution::Filed),
        (
            "site-deployer-key".to_owned(),
            Resolution::Failed("ledger locked".into()),
        ),
    ]);
    let named = lines(&["a GitHub token", "the site deployer's key", "the npm token"]);
    let (text, _) = told_line(&named, &read, &BTreeSet::new()).expect("told");
    assert!(
        text.contains("codex-alab filed `aterm secrets need gh:dev`."),
        "{text}"
    );
    assert!(
        text.contains("could not file `aterm secrets need site-deployer-key` (ledger locked)"),
        "{text}"
    );
    assert!(
        text.contains("File it: `aterm secrets need <want>`."),
        "{text}"
    );
    assert!(!text.contains("NEEDS YOU:"), "{text}");
    let (text, _) = told_line(
        &lines(&["the notary credential"]),
        &CredentialRead::NotAsked,
        &BTreeSet::new(),
    )
    .expect("told");
    assert!(text.contains("`aterm secrets need notary`"), "{text}");
}

/// ONCE A FACT: what was told is never told again; a new fact beside it is told alone; a line
/// that names no credential tells nothing.
#[test]
fn each_fact_is_told_once() {
    let named = lines(&["the notary credential"]);
    let (_, keys) = told_line(&named, &held_notary(), &BTreeSet::new()).expect("told");
    let told: BTreeSet<String> = keys.into_iter().collect();
    assert_eq!(told_line(&named, &held_notary(), &told), None);
    let (text, keys) = told_line(
        &lines(&["the notary credential", "the release credential"]),
        &held_notary(),
        &told,
    )
    .expect("the release machine is new");
    assert_eq!(keys, [RELEASE_WANT]);
    assert!(!text.contains("release-notary"), "{text}");
    assert_eq!(
        told_line(
            &lines(&["a decision on the schema"]),
            &held_notary(),
            &BTreeSet::new()
        ),
        None
    );
}

/// codex-alab resolves each want once, never the release machine's.
#[test]
fn the_host_resolves_each_want_once() {
    assert_eq!(
        host_wants(&lines(&[
            "the release credential",
            "the notary credential",
            "an app-specific password for notarytool",
            "a GitHub token",
        ])),
        ["notary", "gh:dev"]
    );
}

/// A stand-in `aterm` whose registry holds the notary credential alone, each `need` it is asked
/// appended to `needs`.
fn fake_aterm(dir: &tempfile::TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let needs = dir.path().join("needs");
    let path = dir.path().join("aterm");
    let listing = r#"{"entries":[{"description":"the notary credential","name":"release-notary","pointer":"~/.secrets/release-notary"},{"description":"the release GitHub token","name":"gh_access_token_alabsystems","pointer":"~/.secrets/gh_access_token_alabsystems"}]}"#;
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\ncase \"$2\" in\n  ls) echo '{listing}' ;;\n  need) echo \"$3\" >> '{}' ;;\n  *) exit 2 ;;\nesac\n",
            needs.display()
        ),
    )
    .expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("mode");
    (path, needs)
}

/// THE ONE PATH: a want the registry holds is named from it and filed nowhere; one it lacks is
/// filed; each is resolved once a session, however many points name it.
#[tokio::test]
async fn each_want_is_looked_up_then_filed_once() {
    let dir = tempfile::tempdir().expect("dir");
    let (aterm, needs) = fake_aterm(&dir);
    let mut known = BTreeMap::new();
    let wants = host_wants(&lines(&[
        "the notary credential",
        "a GitHub token",
        "the release GitHub token",
    ]));
    let read = resolve(&aterm, &wants, &mut known).await;
    assert_eq!(
        read,
        CredentialRead::Resolved(vec![
            (
                "notary".into(),
                Resolution::Held(vec![entry(
                    "release-notary",
                    "~/.secrets/release-notary",
                    "the notary credential"
                )])
            ),
            ("gh:dev".into(), Resolution::Filed),
            (
                "gh:release".into(),
                Resolution::Held(vec![entry(
                    "gh_access_token_alabsystems",
                    "~/.secrets/gh_access_token_alabsystems",
                    "the release GitHub token"
                )])
            ),
        ])
    );
    assert_eq!(std::fs::read_to_string(&needs).expect("needs"), "gh:dev\n");
    assert_eq!(resolve(&aterm, &wants, &mut known).await, read);
    assert_eq!(
        std::fs::read_to_string(&needs).expect("needs"),
        "gh:dev\n",
        "a want resolved is filed no more"
    );
    assert_eq!(
        resolve(&aterm, &[], &mut known).await,
        CredentialRead::NotAsked
    );
}

/// AN UNREAD REGISTRY IS NO EMPTY ONE: the want is filed and the ledger answers; a filing that
/// fails says why, and is tried again at the next point.
#[tokio::test]
async fn an_unread_registry_files_and_a_failure_is_tried_again() {
    let filed = resolve_over(None, "notary", |_| async { Ok(()) }).await;
    assert_eq!(filed, Resolution::Filed);
    let dir = tempfile::tempdir().expect("dir");
    let missing = dir.path().join("no-such-aterm");
    let mut known = BTreeMap::new();
    let wants = vec!["gh:dev".to_string()];
    let CredentialRead::Resolved(read) = resolve(&missing, &wants, &mut known).await else {
        panic!("resolved");
    };
    assert!(
        matches!(&read[0].1, Resolution::Failed(why) if why.contains("did not start")),
        "{read:?}"
    );
    let (aterm, needs) = fake_aterm(&dir);
    assert_eq!(
        resolve(&aterm, &wants, &mut known).await,
        CredentialRead::Resolved(vec![("gh:dev".into(), Resolution::Filed)])
    );
    assert_eq!(std::fs::read_to_string(&needs).expect("needs"), "gh:dev\n");
}
