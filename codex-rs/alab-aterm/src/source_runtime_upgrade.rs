// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Runtime continuation on the actual SDK constructor's authenticated stream.
//! No endpoint, claim, constructor token or model permission is decoded from DATA.
use super::*;
use crate::conn::Framing;
use crate::runtime_claim::Receipt;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::sync::OnceLock;

struct Host(Arc<Transport>);
struct Continuation {
    transport: Arc<Transport>,
    operation: String,
    primary: String,
    build: u64,
    bytes: u64,
    digest: String,
    activated: OnceLock<Receipt>,
}
pub(super) fn host(transport: Arc<Transport>) -> Arc<dyn codex_alab_core::runtime_upgrade::Host> {
    Arc::new(Host(transport))
}
fn fields<'a>(line: &'a str, prefix: &str) -> io::Result<BTreeMap<&'a str, &'a str>> {
    if line.len() > 4096 {
        return Err(unavailable());
    }
    let mut fields = BTreeMap::new();
    for word in line
        .strip_prefix(prefix)
        .ok_or_else(unavailable)?
        .split_ascii_whitespace()
    {
        let (key, value) = word.split_once('=').ok_or_else(unavailable)?;
        if value.is_empty() || fields.insert(key, value).is_some() {
            return Err(unavailable());
        }
    }
    Ok(fields)
}
fn primary_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, byte)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
        && value
            .bytes()
            .any(|byte| matches!(byte, b'1'..=b'9' | b'a'..=b'f'))
}
fn size(value: &str) -> Option<u64> {
    let number = value.parse::<u64>().ok()?;
    (number > 0 && number.to_string() == value).then_some(number)
}
async fn call(transport: &Transport, request: &str) -> io::Result<String> {
    transport
        .conn
        .request(
            &format!("@{} native-act {request}", transport.sid),
            Framing::Status,
        )
        .await
        .map(|reply| reply.status)
        .map_err(|_| {
            if transport.conn.is_open() {
                unavailable()
            } else {
                io::Error::new(io::ErrorKind::BrokenPipe, "native runtime connection ended")
            }
        })
}
/// Only the host can distinguish an acknowledged settlement from an original
/// serving thread still holding custody. This independent socket only observes.
async fn observe_settlement(transport: &Transport, receipt: &Receipt) -> io::Result<()> {
    let ended = || {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "native settlement was not confirmed after SDK EOF",
        )
    };
    tokio::time::timeout(crate::client::CLAIM_TIMEOUT, async {
        let peer = transport.original_server_pid().map_err(|_| ended())?;
        let opened = conn::open(&transport.endpoint.sock, CALL_TIMEOUT)
            .await
            .map_err(|_| ended())?;
        if opened.server_pid != Some(peer) || opened.auth_identity != transport.auth_identity {
            return Err(ended());
        }
        let observer = Conn::spawn(opened, CALL_TIMEOUT);
        let request = format!(
            "@{} native-act settled source-exec operation={}",
            transport.sid, receipt.operation
        );
        loop {
            let reply = observer
                .request_status_bounded_within(&request, 4096, CALL_TIMEOUT)
                .await
                .map_err(|_| ended())?;
            match receipt.state(&reply.status) {
                Some("settled") => return Ok(()),
                Some("waiting") => tokio::time::sleep(Duration::from_millis(50)).await,
                _ => return Err(ended()),
            }
        }
    })
    .await
    .map_err(|_| ended())?
}
impl codex_alab_core::runtime_upgrade::Host for Host {
    fn load<'a>(
        &'a self,
        primary: &'a str,
        state: &'a [u8],
        build: u64,
    ) -> codex_alab_core::runtime_upgrade::LoadFuture<'a> {
        Box::pin(async move {
            if build == 0
                || state.is_empty()
                || state.len() > 256 * 1024 * 1024
                || !primary_id(primary)
            {
                return Err(unavailable());
            }
            let digest = format!("{:x}", Sha256::digest(state));
            let reply = call(&self.0, "runtime-data discover").await?;
            let metadata = fields(&reply, "OK native runtime-data ")?;
            let operation = *metadata.get("operation").ok_or_else(unavailable)?;
            if metadata.len() != 4
                || metadata.get("state") != Some(&"retained")
                || !valid_hex(operation, 32)
                || metadata.get("bytes").and_then(|value| size(value)) != Some(state.len() as u64)
                || metadata.get("digest").copied() != Some(digest.as_str())
            {
                return Err(unavailable());
            }
            let continuation = Arc::new(Continuation {
                transport: self.0.clone(),
                operation: operation.into(),
                primary: primary.into(),
                build,
                bytes: state.len() as u64,
                digest,
                activated: OnceLock::new(),
            });
            codex_alab_core::runtime_upgrade::Continuation::check(continuation.as_ref()).await?;
            Ok(continuation as Arc<dyn codex_alab_core::runtime_upgrade::Continuation>)
        })
    }
}
impl codex_alab_core::runtime_upgrade::Continuation for Continuation {
    fn check(&self) -> core::UnitFuture<'_> {
        Box::pin(async move {
            let reply = call(
                &self.transport,
                &format!("load source-exec operation={}", self.operation),
            )
            .await?;
            let metadata = fields(&reply, "OK native source-exec ")?;
            if metadata.len() != 5
                || metadata.get("state") != Some(&"loaded")
                || metadata.get("operation").copied() != Some(self.operation.as_str())
                || metadata.get("primary").copied() != Some(self.primary.as_str())
                || metadata.get("bytes").and_then(|value| size(value)) != Some(self.bytes)
                || metadata.get("digest").copied() != Some(self.digest.as_str())
            {
                return Err(unavailable());
            }
            Ok(())
        })
    }
    fn activate(&self) -> core::UnitFuture<'_> {
        Box::pin(async move {
            let reply = call(
                &self.transport,
                &format!("activate source-exec operation={}", self.operation),
            )
            .await?;
            let metadata = fields(&reply, "OK native source-exec ")?;
            if metadata.len() != 4
                || metadata.get("state") != Some(&"activated")
                || metadata.get("build").and_then(|value| size(value)) != Some(self.build)
                || metadata.get("operation").copied() != Some(self.operation.as_str())
                || metadata.get("primary").copied() != Some(self.primary.as_str())
            {
                return Err(unavailable());
            }
            self.activated.get_or_init(|| Receipt {
                operation: self.operation.clone(),
                primary: self.primary.clone(),
                build: self.build,
            });
            Ok(())
        })
    }

    fn settle(&self) -> core::UnitFuture<'_> {
        Box::pin(async move {
            let receipt = self.activated.get().ok_or_else(unavailable)?;
            if !self.transport.conn.is_open() {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "native settlement SDK is closed",
                ));
            }
            let request = format!(
                "@{} native-act settle source-exec operation={}",
                self.transport.sid, self.operation
            );
            match self.transport.conn.request(&request, Framing::Status).await {
                Ok(reply) if receipt.state(&reply.status) == Some("settled") => {}
                Ok(_) => return Err(unavailable()),
                Err(crate::Error::Io(_) | crate::Error::Unanswered | crate::Error::Closed) => {
                    self.transport.conn.close();
                    observe_settlement(&self.transport, receipt).await?;
                }
                Err(error) => return Err(transport_error(error)),
            }
            crate::runtime_claim::publish(&self.transport.endpoint, receipt.clone());
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_core::runtime_upgrade::Host as _;

    const PRIMARY: &str = "00000000-0000-4000-8000-000000000001";
    const OPERATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    // Each fixture has its own listener; the production receipt is process-wide.
    static SETTLEMENT_TEST: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

    async fn supervisor(fake: &crate::fake::FakeAterm) -> crate::Client {
        tokio::time::timeout(
            Duration::from_secs(5),
            crate::runtime_claim::settled_client(fake.endpoint(), crate::Options::default()),
        )
        .await
        .unwrap()
        .unwrap()
    }

    #[tokio::test]
    async fn retained_bytes_require_native_load_and_activation_on_the_original_stream() {
        let _settlement = SETTLEMENT_TEST.acquire().await.unwrap();
        let directory = tempfile::tempdir().unwrap();
        let fake = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        // Authenticated transport fixture only. These scripted replies are not
        // a signed SDK constructor or a native model-admission qualification.
        let transport = Arc::new(
            connect_transport(&fake.endpoint(), "fixture-birth")
                .await
                .unwrap(),
        );
        let state = b"retained original runtime";
        let digest = format!("{:x}", Sha256::digest(state));
        let command = |tail: &str| format!("@{} native-act {tail}", fake.sid());
        fake.reply_once(&command("runtime-data discover"), &format!(
            "OK native runtime-data state=retained operation={OPERATION} bytes={} digest={digest}\n", state.len()));
        fake.reply_once(&command(&format!("load source-exec operation={OPERATION}")), &format!(
            "OK native source-exec state=loaded operation={OPERATION} primary={PRIMARY} bytes={} digest={digest}\n", state.len()));
        let connection = transport.conn.clone();
        let loaded = Host(transport)
            .load(PRIMARY, state, ADMITTED_BUILD)
            .await
            .unwrap();
        assert!(fake.claim().is_none());
        fake.reply_once(&command(&format!("activate source-exec operation={OPERATION}")), &format!(
            "OK native source-exec state=activated operation={OPERATION} primary={PRIMARY} build=2026111101\n"));
        loaded.activate().await.unwrap();
        assert!(
            fake.requests()
                .iter()
                .all(|line| !line.contains("settle source-exec")),
            "activation must retain native custody until actual UI attachment"
        );
        fake.reply_once(&command(&format!("settle source-exec operation={OPERATION}")), &format!(
            "OK native source-exec state=settled operation={OPERATION} primary={PRIMARY} build=2026111101\n"));
        loaded.settle().await.unwrap();
        let client = supervisor(&fake).await;
        assert!(
            !client.holds_claim(),
            "settlement alone is not local adoption"
        );
        assert!(matches!(
            client.harness_policy().await.unwrap(),
            crate::PolicyAnswer::Supported(_)
        ));
        for fields in [
            format!("state=adopted operation={OPERATION} primary={PRIMARY} build=2026111102"),
            format!(
                "state=adopted operation={OPERATION} operation={OPERATION} primary={PRIMARY} build=2026111101"
            ),
            format!("state=activated operation={OPERATION} primary={PRIMARY} build=2026111101"),
        ] {
            fake.reply_once(
                &command(&format!("adopt source-exec operation={OPERATION}")),
                &format!("OK native source-exec {fields}\n"),
            );
            assert!(matches!(
                client.claim().await.unwrap(),
                crate::ClaimOutcome::Unproven
            ));
            assert!(
                !client.holds_claim(),
                "changed or ambiguous native receipts cannot renew authority"
            );
        }
        fake.reply_sequence(&command(&format!("adopt source-exec operation={OPERATION}")),
            ["waiting", "waiting", "adopted"].into_iter().map(|state| format!(
                "OK native source-exec state={state} operation={OPERATION} primary={PRIMARY} build=2026111101\n"
            )).collect());
        assert!(matches!(
            client.claim().await.unwrap(),
            crate::ClaimOutcome::Held { .. }
        ));
        assert!(client.holds_claim());
        assert_eq!(
            fake.requests()
                .iter()
                .filter(|line| line.contains("native-act adopt source-exec"))
                .count(),
            6,
            "waiting receipts never complete adoption"
        );
        client.close();
        assert!(
            connection.is_open(),
            "supervisor lifetime must not close its SDK constructor socket"
        );
        connection.close();
        let count = fake.requests().len();
        assert_eq!(
            loaded.settle().await.unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            fake.requests().len(),
            count,
            "closed-stream cleanup cannot fabricate another connection"
        );
        assert!(
            fake.claim().is_none(),
            "the transport never invents a claim from DATA"
        );
        assert!(
            fake.requests()
                .iter()
                .all(|line| !line.contains("meta set supervisor"))
        );
    }

    fn native(fake: &crate::fake::FakeAterm, action: &str) -> String {
        format!(
            "@{} native-act {action} source-exec operation={OPERATION}",
            fake.sid()
        )
    }

    const ADMITTED_BUILD: u64 = 2026111101;

    fn receipt(state: &str) -> String {
        format!(
            "OK native source-exec state={state} operation={OPERATION} primary={PRIMARY} build=2026111101\n"
        )
    }

    async fn loaded(
        fake: &crate::fake::FakeAterm,
    ) -> Arc<dyn codex_alab_core::runtime_upgrade::Continuation> {
        let transport = connect_transport(&fake.endpoint(), "fixture-birth")
            .await
            .unwrap();
        let data = b"retained";
        let digest = format!("{:x}", Sha256::digest(data));
        let length = data.len();
        let discover = format!("@{} native-act runtime-data discover", fake.sid());
        let retained = format!(
            "OK native runtime-data state=retained operation={OPERATION} bytes={length} digest={digest}\n"
        );
        let loaded = format!(
            "OK native source-exec state=loaded operation={OPERATION} primary={PRIMARY} bytes={length} digest={digest}\n"
        );
        fake.reply_once(&discover, &retained);
        fake.reply_once(&native(fake, "load"), &loaded);
        Host(Arc::new(transport))
            .load(PRIMARY, data, ADMITTED_BUILD)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_lost_settlement_ack_waits_for_native_eof_without_replaying_settle_or_adopting() {
        let _settlement = SETTLEMENT_TEST.acquire().await.unwrap();
        let directory = tempfile::tempdir().unwrap();
        let fake = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let target = loaded(&fake).await;
        assert!(target.settle().await.is_err());
        fake.reply_once(&native(&fake, "activate"), &receipt("activated"));
        target.activate().await.unwrap();
        fake.reply_once(&native(&fake, "settle"), &receipt("settled"));
        fake.drop_next_reply("native-act settle source-exec");
        fake.reply_sequence(
            &native(&fake, "settled"),
            vec![receipt("waiting"), receipt("waiting"), receipt("settled")],
        );
        target.settle().await.unwrap();
        assert_eq!(
            target.settle().await.unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        let sent = fake.requests();
        assert_eq!(
            sent.iter()
                .filter(|line| **line == native(&fake, "settle"))
                .count(),
            1
        );
        assert_eq!(
            sent.iter()
                .filter(|line| **line == native(&fake, "settled"))
                .count(),
            3
        );
        assert!(!sent.contains(&native(&fake, "adopt")));
        let client = supervisor(&fake).await;
        assert!(!client.holds_claim());
        assert!(fake.claim().is_none());
        fake.reply_once(&native(&fake, "adopt"), &receipt("adopted"));
        client.harness_policy().await.unwrap();
        assert!(matches!(
            client.claim().await.unwrap(),
            crate::ClaimOutcome::Held { .. }
        ));
        assert!(client.holds_claim());
        assert_eq!(
            fake.requests()
                .iter()
                .filter(|line| **line == native(&fake, "adopt"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn observation_refusals_or_mismatched_receipts_cannot_recover_a_lost_ack() {
        let good = receipt("settled");
        for reply in [
            "ERR native source-exec unavailable\n".into(),
            receipt("adopted"),
            receipt("activated"),
            good.replace("2026111101", "2026111102"),
            good.replace(PRIMARY, "00000000-0000-4000-8000-000000000002"),
            good.replace(OPERATION, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            good.replace("build=", "build=2026111101 build="),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let fake = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
                .await
                .unwrap();
            let target = loaded(&fake).await;
            fake.reply_once(&native(&fake, "activate"), &receipt("activated"));
            target.activate().await.unwrap();
            fake.reply_once(&native(&fake, "settle"), &good);
            fake.drop_next_reply("native-act settle source-exec");
            fake.reply_once(&native(&fake, "settled"), &reply);
            assert_eq!(
                target.settle().await.unwrap_err().kind(),
                io::ErrorKind::BrokenPipe,
                "{reply}"
            );
            assert_eq!(
                fake.requests()
                    .iter()
                    .filter(|line| **line == native(&fake, "settled"))
                    .count(),
                1
            );
            assert!(!fake.requests().contains(&native(&fake, "adopt")));
        }
    }

    #[tokio::test]
    async fn first_activation_must_match_the_admitted_package_before_settlement() {
        let directory = tempfile::tempdir().unwrap();
        let fake = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let target = loaded(&fake).await;
        fake.reply_once(
            &native(&fake, "activate"),
            &receipt("activated").replace("2026111101", "2026111102"),
        );
        assert!(target.activate().await.is_err());
        assert!(target.settle().await.is_err());
        assert!(!fake.requests().contains(&native(&fake, "settle")));
        assert!(!fake.requests().contains(&native(&fake, "adopt")));

        fake.reply_once(&native(&fake, "activate"), &receipt("activated"));
        target.activate().await.unwrap();
    }

    #[tokio::test]
    async fn settlement_requires_the_activated_build_and_original_listener() {
        let directory = tempfile::tempdir().unwrap();
        let fake = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let target = loaded(&fake).await;
        fake.reply_once(&native(&fake, "activate"), &receipt("activated"));
        target.activate().await.unwrap();
        fake.reply_once(
            &native(&fake, "activate"),
            &receipt("activated").replace("2026111101", "2026111102"),
        );
        assert!(target.activate().await.is_err());
        for reply in [
            receipt("settled").replace("2026111101", "2026111102"),
            "ERR native source-exec unavailable\n".into(),
        ] {
            fake.reply_once(&native(&fake, "settle"), &reply);
            assert!(target.settle().await.is_err());
        }
        fake.replace_listener_identity().unwrap();
        fake.reply_once(&native(&fake, "settle"), &receipt("settled"));
        fake.drop_next_reply("native-act settle source-exec");
        assert_eq!(
            target.settle().await.unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert!(!fake.requests().contains(&native(&fake, "settled")));
        assert!(!fake.requests().contains(&native(&fake, "adopt")));
    }

    #[tokio::test]
    async fn changed_or_ambiguous_host_copy_never_reaches_load_or_activation() {
        for metadata in [
            "state=retained operation=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bytes=3 digest=bad"
                .to_string(),
            "state=retained operation=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bytes=03 digest=bad"
                .to_string(),
            "state=retained operation=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bytes=3 bytes=3 digest=bad"
                .to_string(),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let fake = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
                .await
                .unwrap();
            let transport = Arc::new(
                connect_transport(&fake.endpoint(), "fixture-birth")
                    .await
                    .unwrap(),
            );
            fake.reply_once(
                &format!("@{} native-act runtime-data discover", fake.sid()),
                &format!("OK native runtime-data {metadata}\n"),
            );
            assert!(
                Host(transport)
                    .load(PRIMARY, b"abc", ADMITTED_BUILD)
                    .await
                    .is_err()
            );
            assert!(
                fake.requests()
                    .iter()
                    .all(|line| !line.contains("source-exec"))
            );
        }
    }

    #[test]
    fn malformed_native_frames_never_supply_an_activation() {
        for line in [
            "ERR native source-exec unavailable",
            "OK native source-exec state=loaded state=activated",
            "OK native source-exec state=activated operation=",
        ] {
            assert!(fields(line, "OK native source-exec ").is_err());
        }
        assert!(!primary_id("00000000-0000-0000-0000-000000000000"));
    }
}
