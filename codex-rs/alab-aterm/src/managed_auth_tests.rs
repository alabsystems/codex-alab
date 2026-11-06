// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::fake::FakeAterm;
use crate::fake::Mode;

fn request() -> core::Request {
    core::Request {
        slot: "736c6f742f6f6e65".into(),
        operation: "c".repeat(32),
        expected_digest: "a".repeat(64),
    }
}

fn reply(request: &core::Request, state: &str) -> String {
    format!(
        "OK codex-auth state={state} slot={} operation={} expected_digest={} generation=2 digest={}",
        request.slot,
        request.operation,
        request.expected_digest,
        "b".repeat(64)
    )
}

fn pending(request: &core::Request) -> String {
    format!(
        "OK codex-auth refresh-pending slot={} operation={} expected_digest={}",
        request.slot, request.operation, request.expected_digest
    )
}

#[test]
fn pending_without_known_revision_is_strictly_bound_to_the_refresh() {
    let request = request();
    let pending = pending(&request);
    assert!(matches!(parse(&pending, &request), Ok(Outcome::Waiting)));
    for invalid in [
        pending.replace(&request.slot, "736c6f742f74776f"),
        pending.replace(&request.operation, &"d".repeat(32)),
        pending.replace(&request.expected_digest, &"d".repeat(64)),
        format!("{pending} slot={}", request.slot),
        format!("{pending} generation=1"),
        pending.replace("refresh-pending ", ""),
    ] {
        assert!(parse(&invalid, &request).is_err());
    }
}

#[test]
fn reply_binds_every_field_and_rejects_unknown_duplicate_or_secret_errors() {
    let request = request();
    let ready = reply(&request, "ready");
    assert!(matches!(parse(&ready, &request), Ok(Outcome::Ready(_))));
    assert!(matches!(
        parse(&reply(&request, "waiting"), &request),
        Ok(Outcome::Waiting)
    ));
    for invalid in [
        format!("{ready} extra=1"),
        format!("{ready} state=ready"),
        ready.replace("generation=2", "generation=02"),
        ready.replace("generation=2", "generation=0"),
        ready.replace(&request.operation, &"d".repeat(32)),
        ready.replace(&request.expected_digest, &"d".repeat(64)),
        ready.replace(&request.slot, "736c6f742f74776f"),
        reply(&request, "unknown"),
        "ERR secret-fixture-token".into(),
    ] {
        let error = parse(&invalid, &request)
            .err()
            .expect("malformed reply rejected")
            .to_string();
        assert!(!error.contains("secret-fixture-token"));
    }
}

#[tokio::test]
async fn authenticated_refresh_retries_waiting_without_claiming_autopilot() -> io::Result<()> {
    let dir = tempfile::tempdir()?;
    let fake = FakeAterm::start(dir.path(), Mode::Current).await?;
    fake.set_effective(false, "interface off");
    let request = request();
    let line = format!(
        "@{} codex-auth refresh slot={} operation={} expected_digest={}",
        fake.sid(),
        request.slot,
        request.operation,
        request.expected_digest
    );
    fake.reply_once(&line, &format!("{}\n", pending(&request)));
    let publish = async {
        while !fake.requests().contains(&line) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        fake.reply_once(&line, &format!("{}\n", reply(&request, "ready")));
    };
    let endpoint = fake.endpoint();
    let (revision, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(refresh_at(&endpoint, &request), publish)
    })
    .await
    .expect("bounded refresh");
    assert_eq!(revision?.generation, 2);
    assert_eq!(
        fake.requests()
            .iter()
            .filter(|command| **command == line)
            .count(),
        2
    );
    assert!(
        !fake
            .requests()
            .iter()
            .any(|command| command.contains("supervisor") || command.contains("harness policy"))
    );
    assert!(fake.claim().is_none());
    Ok(())
}

#[test]
fn endpoint_and_install_remain_available_when_the_interface_is_off() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let graph = root.path().join("aterm/graph");
    std::fs::create_dir_all(&graph)?;
    std::fs::write(
        graph.join("s-cafe"),
        format!("sock {}\n", root.path().join("fixture.sock").display()),
    )?;
    let local = module_path!().split_once("::").unwrap().1;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            &format!("{local}::interface_off_child"),
            "--nocapture",
        ])
        .env_clear()
        .env("ALAB_MANAGED_AUTH_OFF_CHILD", "1")
        .env("XDG_RUNTIME_DIR", root.path())
        .env(crate::locate::SELF_SID_ENV, "s-cafe")
        .env("PATH", "/usr/bin:/bin")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("MANAGED_AUTH_OFF_COMPLETE"));
    Ok(())
}

#[test]
#[expect(
    clippy::print_stdout,
    reason = "exact completion proof in an isolated child"
)]
fn interface_off_child() {
    if std::env::var_os("ALAB_MANAGED_AUTH_OFF_CHILD").is_none() {
        return;
    }
    codex_alab_core::activate();
    codex_alab_core::set_enhanced(false);
    assert!(!codex_alab_core::active());
    assert!(Endpoint::from_env().is_none());
    assert_eq!(Endpoint::managed_auth_from_env().unwrap().sid, "s-cafe");
    install();
    println!("MANAGED_AUTH_OFF_COMPLETE");
}
