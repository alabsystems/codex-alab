// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::Client;
use crate::Error;
use crate::Options;
use std::os::unix::fs::PermissionsExt;

/// The longest socket name FakeAterm can assign with its u32 instance counter.
const LONGEST_SOCKET_NAME: &str = "aterm-4294967295.sock";

/// Use the caller's admitted scratch exactly, even when TMPDIR is nested later.
/// Without an explicit root, preserve the ordinary short /tmp fixture location.
fn short_socket_directory() -> std::io::Result<tempfile::TempDir> {
    let scratch = std::env::var_os("ALAB_TEST_SCRATCH").map(std::path::PathBuf::from);
    short_socket_directory_in(scratch.as_deref())
}

fn short_socket_directory_in(
    scratch: Option<&std::path::Path>,
) -> std::io::Result<tempfile::TempDir> {
    let parent = scratch.unwrap_or(std::path::Path::new("/tmp"));
    if !parent.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "configured fake handshake root must be absolute",
        ));
    }
    let directory = tempfile::Builder::new()
        .prefix("ah.")
        .rand_bytes(6)
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)?;
    check_socket_directory(directory.path())?;
    Ok(directory)
}

fn check_socket_directory(directory: &std::path::Path) -> std::io::Result<()> {
    let resolved = directory.canonicalize()?;
    if [directory, resolved.as_path()].into_iter().any(|root| {
        root.join(LONGEST_SOCKET_NAME)
            .as_os_str()
            .as_encoded_bytes()
            .len()
            > 103
    }) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "fake handshake socket exceeds its 103-byte path budget",
        ));
    }
    Ok(())
}

#[test]
fn explicit_socket_root_is_private_unique_and_cleaned() {
    let owner = short_socket_directory().unwrap();
    let parent = owner.path().parent().unwrap();
    let first = short_socket_directory_in(Some(parent)).unwrap();
    let second = short_socket_directory_in(Some(parent)).unwrap();
    assert_ne!(first.path(), second.path());
    for directory in [&first, &second] {
        assert_eq!(directory.path().parent(), Some(parent));
        let leaf = directory.path().file_name().unwrap().as_encoded_bytes();
        assert!(leaf.starts_with(b"ah."));
        assert_eq!(leaf.len(), 9);
        assert_eq!(
            directory.path().metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
        let socket = directory.path().join(LONGEST_SOCKET_NAME);
        assert!(socket.as_os_str().as_encoded_bytes().len() <= 103);
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        drop(listener);
    }
    let paths = [first.path().to_path_buf(), second.path().to_path_buf()];
    drop(first);
    drop(second);
    assert!(paths.iter().all(|path| !path.exists()));
}

#[test]
fn invalid_explicit_socket_roots_do_not_fall_back() {
    for relative in ["", "relative"] {
        let error = short_socket_directory_in(Some(std::path::Path::new(relative))).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
    let owner = short_socket_directory().unwrap();
    let missing = owner.path().join("missing");
    let error = short_socket_directory_in(Some(&missing)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(!missing.exists());
    let file = owner.path().join("file");
    std::fs::write(&file, b"preserved").unwrap();
    assert!(short_socket_directory_in(Some(&file)).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), b"preserved");
    assert_eq!(std::fs::read_dir(owner.path()).unwrap().count(), 1);
}

#[test]
fn lexical_and_canonical_socket_budgets_reject_and_clean_the_candidate() {
    let owner = short_socket_directory().unwrap();
    let long = owner.path().join("p".repeat(104));
    std::fs::create_dir(&long).unwrap();
    let error = short_socket_directory_in(Some(&long)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(std::fs::read_dir(&long).unwrap().count(), 0);

    let alias = short_socket_directory().unwrap();
    assert!(
        alias
            .path()
            .join(LONGEST_SOCKET_NAME)
            .as_os_str()
            .as_encoded_bytes()
            .len()
            <= 103,
        "the alias alone must fit so this exercises the canonical limit"
    );
    std::fs::remove_dir(alias.path()).unwrap();
    std::os::unix::fs::symlink(&long, alias.path()).unwrap();
    let error = check_socket_directory(alias.path()).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    std::fs::remove_file(alias.path()).unwrap();
    assert_eq!(std::fs::read_dir(&long).unwrap().count(), 0);
}

async fn fake() -> (tempfile::TempDir, FakeAterm) {
    let directory = short_socket_directory().expect("private short socket directory");
    let fake = FakeAterm::start(directory.path(), Mode::Current)
        .await
        .expect("fake host");
    (directory, fake)
}

fn stages(fake: &FakeAterm) -> Vec<HandshakeStage> {
    fake.handshake_events()
        .events
        .into_iter()
        .map(|event| event.stage)
        .collect()
}

#[tokio::test]
async fn a_received_version_does_not_claim_its_reply_was_written() {
    let (_directory, fake) = fake().await;
    fake.set_version("private-reply-content");
    fake.gate("version");
    let endpoint = fake.endpoint();
    let connecting = tokio::spawn(Client::connect(endpoint, Options::default()));
    assert!(
        fake.wait_for(|host| host.gated_requests().iter().any(|line| line == "version"))
            .await
    );
    assert_eq!(
        stages(&fake),
        [
            HandshakeStage::Accepted,
            HandshakeStage::Authenticated,
            HandshakeStage::VersionReceived,
        ]
    );
    assert!(fake.requests().is_empty());

    fake.open_gate("version");
    let client = connecting.await.unwrap().expect("authenticated client");
    let reply_bytes = "OK version=private-reply-content\n".len();
    assert_eq!(
        stages(&fake),
        [
            HandshakeStage::Accepted,
            HandshakeStage::Authenticated,
            HandshakeStage::VersionReceived,
            HandshakeStage::VersionWriteStarted { reply_bytes },
            HandshakeStage::VersionWriteFinished {
                reply_bytes,
                error: None,
            },
        ]
    );
    let observed = fake.handshake_events();
    assert_eq!(observed.dropped_events, 0);
    assert!(observed.events.iter().all(|event| event.connection == 1));
    assert!(
        observed
            .events
            .iter()
            .all(|event| event.elapsed_micros <= observed.captured_elapsed_micros)
    );
    assert!(
        observed
            .events
            .windows(2)
            .all(|pair| pair[0].elapsed_micros <= pair[1].elapsed_micros)
    );
    let encoded = serde_json::to_string(&observed).unwrap();
    let token = crate::locate::read_token_beside(&fake.endpoint().sock).unwrap();
    assert!(!encoded.contains(&token));
    assert!(!encoded.contains("private-reply-content"));
    client.close();
}

#[tokio::test]
async fn an_intentionally_dropped_reply_is_distinct_from_a_write_attempt() {
    let (_directory, fake) = fake().await;
    fake.drop_next_reply("version");
    assert!(matches!(
        Client::connect(fake.endpoint(), Options::default()).await,
        Err(Error::Closed)
    ));
    assert_eq!(
        stages(&fake),
        [
            HandshakeStage::Accepted,
            HandshakeStage::Authenticated,
            HandshakeStage::VersionReceived,
            HandshakeStage::VersionReplyDropped,
        ]
    );
    assert_eq!(fake.requests(), ["version"]);
}

#[tokio::test]
async fn a_closed_peer_records_the_failed_version_write_without_error_text() {
    let (_directory, fake) = fake().await;
    fake.gate("version");
    let endpoint = fake.endpoint();
    let mut peer = UnixStream::connect(&endpoint.sock).await.unwrap();
    let token = crate::locate::read_token_beside(&endpoint.sock).unwrap();
    peer.write_all(format!("AUTH {token}\nversion\n").as_bytes())
        .await
        .unwrap();
    assert!(
        fake.wait_for(|host| host.gated_requests().iter().any(|line| line == "version"))
            .await
    );
    drop(peer);
    fake.open_gate("version");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if stages(&fake).iter().any(|stage| {
                matches!(
                    stage,
                    HandshakeStage::VersionWriteFinished { error: Some(_), .. }
                )
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("the actual socket write fails after its peer closes");
    let observed = stages(&fake);
    assert!(matches!(
        observed.as_slice(),
        [
            HandshakeStage::Accepted,
            HandshakeStage::Authenticated,
            HandshakeStage::VersionReceived,
            HandshakeStage::VersionWriteStarted { .. },
            HandshakeStage::VersionWriteFinished { error: Some(_), .. },
        ]
    ));
    let error = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "private-error-content");
    let encoded = serde_json::to_string(&HandshakeIoError::from_error(&error)).unwrap();
    assert!(!encoded.contains("private-error-content"));
}
