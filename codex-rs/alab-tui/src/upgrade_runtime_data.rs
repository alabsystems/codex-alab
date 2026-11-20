// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source-side opaque recovery data on the authenticated native host. This
//! channel preserves bytes; it supplies no SDK, account or execution admission.
use crate::upgrade::wait_until;
use crate::upgrade_host::Host;
use codex_alab_aterm::Client;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsFd;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

/// The native host must explicitly admit the complete source-exec protocol.
/// Data transport and target-image compatibility alone cannot grant a fresh SDK
/// constructor. The host currently keeps this capability unavailable until its
/// source/destination integration and recovery qualification are complete.
pub async fn check_execution(client: &Client, deadline: Instant) -> Result<(), String> {
    let command = format!("@{} native-act info source-exec", client.endpoint().sid);
    let reply = match wait_until(deadline, client.request(&command)).await {
        Some(Ok(reply)) => reply,
        None | Some(Err(codex_alab_aterm::Error::Unanswered)) => {
            return Err(
                "timed out waiting for aterm to report live session handoff support".into(),
            );
        }
        Some(Err(_)) => return Err("native runtime capability transport failed".into()),
    };
    match reply.status.as_str() {
        "OK native source-exec preparation=1 execution=available input-fence=1" => Ok(()),
        "OK native source-exec preparation=1 execution=unavailable input-fence=1"
        | "OK native source-exec preparation=0 execution=unavailable input-fence=1" => {
            Err("aterm reports that live session handoff is unavailable".into())
        }
        _ => Err("aterm has not confirmed support for live session handoff".into()),
    }
}

/// Reserve before committing source producers. A dropped preparation releases
/// its empty slot; once transfer starts, failures retain recovery custody.
pub struct Reservation {
    client: Client,
    host: Host,
    operation: String,
    transferring: bool,
    build: u64,
    primary: String,
}
impl Reservation {
    pub async fn reserve(
        client: Client,
        source_build: u64,
        build: u64,
        primary: String,
        deadline: Instant,
    ) -> Result<Self, String> {
        if source_build == 0 || build == 0 || source_build == build || !primary_id(&primary) {
            return Err("invalid native source replacement recipe".into());
        }
        // Keep ownership in the worker while an outstanding prepare reply settles,
        // but do not let an abandoned caller later seal input for a live session.
        let (caller, abandoned) = tokio::sync::oneshot::channel::<()>();
        let result = tokio::spawn(async move {
            let host = Host::capture(&client)?;
            check_execution(&client, deadline).await?;
            let info = call(&client, "info", deadline).await?;
            let info = fields(&info)?;
            if info.len() != 3
                || info.get("protocol") != Some(&"1")
                || info.get("authority") != Some(&"data-only")
                || info.get("max-bytes") != Some(&"268435456")
            {
                return Err("native runtime data protocol is unavailable".into());
            }
            let mut nonce = [0; 16];
            File::open("/dev/urandom")
                .and_then(|mut file| file.read_exact(&mut nonce))
                .map_err(|error| error.to_string())?;
            let operation = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
            let reservation = Self {
                client,
                host,
                operation,
                transferring: false,
                build,
                primary,
            };
            // The source-exec owner reserves and pins its runtime-data slot.
            // Never create an unrelated data-only operation as a substitute.
            let reply = source_call(&reservation.client, &format!(
                "prepare source-exec operation={} source-build={source_build} build={build} primary={}",
                reservation.operation, reservation.primary,
            ), deadline).await?;
            let status = source_fields(&reply)?;
            reservation.status(&status, &["preparing", "prepared"], "open")?;
            if status.get("runtime") != Some(&"reserved") {
                return Err("native source replacement has no reserved data custody".into());
            }
            while_requested(abandoned, reservation.prepare_input(reply, deadline)).await?;
            Ok(reservation)
        })
        .await
        .map_err(|error| error.to_string())?;
        drop(caller);
        result
    }

    async fn prepare_input(&self, reply: String, deadline: Instant) -> Result<(), String> {
        // Input predecessors (not SDK readers) drain before the host can
        // reserve its pending constructor. Require that readiness while
        // source Core/goal admission is still revocable. Go separately
        // requires the exact old SDK actor to have retired.
        let mut reply = reply;
        loop {
            self.host.validate(&self.client)?;
            let status = source_fields(&reply)?;
            self.status(&status, &["preparing", "prepared"], "open")?;
            if status.get("state") == Some(&"prepared") {
                break;
            }
            pause(deadline).await?;
            reply = source_call(
                &self.client,
                &format!("query source-exec operation={}", self.operation),
                deadline,
            )
            .await?;
        }
        let reply = source_call(
            &self.client,
            &format!("seal-input source-exec operation={}", self.operation),
            deadline,
        )
        .await?;
        self.status(&source_fields(&reply)?, &["prepared"], "sealed")?;
        self.host.validate(&self.client)?;
        Ok(())
    }

    fn status(
        &self,
        fields: &BTreeMap<&str, &str>,
        states: &[&str],
        input: &str,
    ) -> Result<(), String> {
        if fields.len() != 6
            || !fields
                .get("state")
                .is_some_and(|state| states.contains(state))
            || fields.get("operation").copied() != Some(self.operation.as_str())
            || fields.get("build").copied() != Some(self.build.to_string().as_str())
            || !matches!(
                fields.get("runtime").copied(),
                Some("reserved" | "retained")
            )
            || fields.get("execution") != Some(&"unavailable")
            || fields.get("input").copied() != Some(input)
        {
            return Err("native source replacement status differs from its preparation".into());
        }
        Ok(())
    }

    /// Reconcile the exact journaled source-exec operation after the real SDK
    /// readers retired and the host retained the final frame. A lost Go reply
    /// never creates another constructor or changes the source operation.
    pub async fn admit_execution(
        &self,
        state: &[u8],
        deadline: Instant,
    ) -> Result<Execution, String> {
        if !self.transferring
            || crate::upgrade_state::data(state)
                .map_err(|_| "invalid final runtime frame")?
                .primary
                != self.primary
        {
            return Err("native source replacement has no matching committed frame".into());
        }
        let digest = format!("{:x}", Sha256::digest(state));
        let reply = call(
            &self.client,
            &format!("status operation={}", self.operation),
            deadline,
        )
        .await?;
        retained(&fields(&reply)?, &self.operation, state.len(), &digest)?;
        let mut requested = false;
        let mut ready = false;
        loop {
            self.host.validate(&self.client)?;
            let verb = if ready && !requested {
                "execute"
            } else {
                "query"
            };
            requested |= verb == "execute";
            let reply = source_call(
                &self.client,
                &format!("{verb} source-exec operation={}", self.operation),
                deadline,
            )
            .await?;
            let status = source_fields(&reply)?;
            if status.get("state") == Some(&"go") {
                if status.len() != 7
                    || status.get("operation").copied() != Some(self.operation.as_str())
                    || status.get("build").copied() != Some(self.build.to_string().as_str())
                    || status.get("primary").copied() != Some(self.primary.as_str())
                    || status.get("bytes").copied() != Some(state.len().to_string().as_str())
                    || status.get("digest").copied() != Some(digest.as_str())
                {
                    return Err("native execution receipt differs from the captured source".into());
                }
                let command = status
                    .get("command")
                    .ok_or("native execution receipt has no command")?
                    .to_string();
                self.host.validate(&self.client)?;
                return Ok(Execution {
                    command,
                    build: self.build,
                    digest,
                    bytes: state.len(),
                });
            }
            self.status(&status, &["prepared", "possible-execution"], "sealed")?;
            // Reconcile this already prepared operation before requesting
            // Execute once. A possible attempt retains the same journal/ctor.
            ready = matches!(
                status.get("state").copied(),
                Some("prepared" | "possible-execution")
            );
            pause(deadline).await?;
        }
    }

    /// Called only with the final producer-stopped frame. Return success only
    /// after the host reports an independent copy with the exact size/digest.
    /// Keep this owner on failure: retry reconciles the SAME operation.
    pub async fn retain(&mut self, bytes: Arc<[u8]>, deadline: Instant) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > crate::upgrade_exec_custody::MAX_STATE_BYTES {
            return Err("invalid runtime recovery data size".into());
        }
        self.host.validate(&self.client)?;
        self.transferring = true;
        let size = bytes.len();
        let (file, digest) = tokio::task::spawn_blocking(move || snapshot(&bytes))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        // A previous waiter may have disappeared after its retain request was
        // accepted. Reconcile that copy before offering a new transfer socket.
        loop {
            self.host.validate(&self.client)?;
            let reply = call(
                &self.client,
                &format!("status operation={}", self.operation),
                deadline,
            )
            .await?;
            let status = fields(&reply)?;
            match status.get("state").copied() {
                Some("retained") => return retained(&status, &self.operation, size, &digest),
                Some("reserved" | "failed") if status.len() == 1 => break,
                Some("receiving") if status.len() == 1 => pause(deadline).await?,
                _ => return Err("native runtime recovery operation is unavailable".into()),
            }
        }
        let directory = tempfile::Builder::new()
            .prefix("cx-runtime-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        let socket = directory.path().join("s");
        let listener = {
            let _fd_custody = codex_alab_core::native_fd_custody::receiving()
                .map_err(|error| error.to_string())?;
            tokio::net::UnixListener::bind(&socket).map_err(|error| error.to_string())?
        };
        let socket = socket.to_str().ok_or("runtime socket path is not UTF-8")?;
        let command = format!(
            "retain operation={} socket={}",
            self.operation,
            codex_alab_aterm::wire::pct_encode(socket)
        );
        let reply = call(&self.client, &command, deadline).await?;
        let status = fields(&reply)?;
        match status.get("state").copied() {
            Some("receiving") if status.len() == 1 => {
                let (stream, _) = wait_until(
                    deadline,
                    std::future::poll_fn(|context| {
                        let _fd_custody = match codex_alab_core::native_fd_custody::receiving() {
                            Ok(guard) => guard,
                            Err(error) => return std::task::Poll::Ready(Err(error)),
                        };
                        listener.poll_accept(context)
                    }),
                )
                .await
                .ok_or("native runtime connection deadline")?
                .map_err(|error| error.to_string())?;
                let peer = stream
                    .peer_cred()
                    .map_err(|error| error.to_string())?
                    .pid()
                    .and_then(|pid| u32::try_from(pid).ok());
                if peer != self.client.server_pid() {
                    return Err("runtime data recipient differs from the authenticated host".into());
                }
                self.host.validate(&self.client)?;
                send(&stream, &file, deadline)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Some("retained") => return retained(&status, &self.operation, size, &digest),
            _ => return Err("native runtime data transfer was refused".into()),
        }
        loop {
            self.host.validate(&self.client)?;
            let reply = call(
                &self.client,
                &format!("status operation={}", self.operation),
                deadline,
            )
            .await?;
            let status = fields(&reply)?;
            if status.get("state") != Some(&"receiving") || status.len() != 1 {
                return retained(&status, &self.operation, size, &digest);
            }
            pause(deadline).await?;
        }
    }
}

async fn while_requested(
    mut abandoned: tokio::sync::oneshot::Receiver<()>,
    readiness: impl std::future::Future<Output = Result<(), String>>,
) -> Result<(), String> {
    tokio::select! {
        biased;
        _ = &mut abandoned => Err("native source preparation was abandoned".into()),
        result = readiness => result,
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.transferring {
            return;
        }
        let client = self.client.clone();
        let operation = self.operation.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let deadline = Instant::now() + Duration::from_secs(5);
                let cancelled = source_call(
                    &client,
                    &format!("cancel-prepare source-exec operation={operation}"),
                    deadline,
                )
                .await;
                if let Err(error) = cancelled {
                    tracing::debug!(%error, "unused native source replacement cancellation failed");
                    return;
                }
                if let Err(error) =
                    call(&client, &format!("release operation={operation}"), deadline).await
                {
                    tracing::debug!(%error, "unused runtime data reservation release failed");
                }
            });
        }
    }
}
/// A Go on the original authenticated native operation, never constructible
/// from runtime bytes or a decoded command supplied by another caller.
pub struct Execution {
    command: String,
    build: u64,
    digest: String,
    bytes: usize,
}
impl Execution {
    pub(crate) fn plan(
        &self,
        state: &[u8],
        expected: &crate::upgrade::ExecPlan,
        admitted: &crate::upgrade_admission::Receipt,
    ) -> Result<crate::upgrade::ExecPlan, String> {
        if self.build != admitted.build
            || self.bytes != state.len()
            || self.digest != format!("{:x}", Sha256::digest(state))
        {
            return Err("native runtime execution differs from final admitted state".into());
        }
        crate::upgrade_native_command::decode(&self.command, expected, admitted)
    }
}
fn primary_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
        && value
            .bytes()
            .any(|byte| matches!(byte, b'1'..=b'9' | b'a'..=b'f'))
}
async fn source_call(client: &Client, tail: &str, deadline: Instant) -> Result<String, String> {
    let command = format!("@{} native-act {tail}", client.endpoint().sid);
    wait_until(deadline, client.request(&command))
        .await
        .ok_or_else(|| "native source replacement deadline".to_owned())?
        .map(|reply| reply.status)
        .map_err(|_| "native source replacement transport failed".to_owned())
}
fn source_fields(line: &str) -> Result<BTreeMap<&str, &str>, String> {
    if line.len() > 132 * 1024 {
        return Err("oversized native source replacement reply".into());
    }
    let body = line
        .strip_prefix("OK native source-exec ")
        .ok_or("native source replacement refused")?;
    let mut result = BTreeMap::new();
    for word in body.split_ascii_whitespace() {
        let (name, value) = word
            .split_once('=')
            .ok_or("invalid native source replacement reply")?;
        if value.is_empty() || result.insert(name, value).is_some() {
            return Err("duplicate or empty native source replacement field".into());
        }
    }
    Ok(result)
}

fn snapshot(bytes: &[u8]) -> io::Result<(File, String)> {
    use std::os::unix::fs::MetadataExt;
    if bytes.is_empty() || bytes.len() > crate::upgrade_exec_custody::MAX_STATE_BYTES {
        return Err(io::Error::other("invalid runtime recovery data size"));
    }
    let digest = format!("{:x}", Sha256::digest(bytes));
    let mut temporary = tempfile::NamedTempFile::new()?;
    temporary.write_all(bytes)?;
    let original = temporary.as_file().metadata()?;
    let file = File::open(temporary.path())?;
    let held = file.metadata()?;
    if (original.dev(), original.ino()) != (held.dev(), held.ino()) {
        return Err(io::Error::other("runtime recovery data file changed"));
    }
    temporary.close()?;
    Ok((file, digest))
}
async fn pause(deadline: Instant) -> Result<(), String> {
    wait_until(deadline, tokio::time::sleep(Duration::from_millis(20)))
        .await
        .ok_or_else(|| "native runtime retention deadline".into())
}
async fn call(client: &Client, tail: &str, deadline: Instant) -> Result<String, String> {
    let command = format!("@{} native-act runtime-data {tail}", client.endpoint().sid);
    wait_until(deadline, client.request(&command))
        .await
        .ok_or_else(|| "native runtime data deadline".to_owned())?
        .map(|reply| reply.status)
        .map_err(|_| "native runtime data transport failed".to_owned())
}
fn fields(line: &str) -> Result<BTreeMap<&str, &str>, String> {
    // The host deliberately reports a failed copy with an ERR status, while
    // retaining the same reserved operation for a producer retry.
    if line == "ERR native runtime-data state=failed" {
        return Ok(BTreeMap::from([("state", "failed")]));
    }
    let body = line
        .strip_prefix("OK native runtime-data ")
        .ok_or("invalid native runtime data reply")?;
    let mut fields = BTreeMap::new();
    if line.len() > 4096 {
        return Err("oversized native runtime data reply".into());
    }
    for word in body.split_ascii_whitespace() {
        let (key, value) = word
            .split_once('=')
            .ok_or("invalid native runtime data field")?;
        if value.is_empty() || fields.insert(key, value).is_some() {
            return Err("duplicate or empty native runtime data field".into());
        }
    }
    Ok(fields)
}
fn retained(
    fields: &BTreeMap<&str, &str>,
    operation: &str,
    size: usize,
    digest: &str,
) -> Result<(), String> {
    if fields.len() != 4
        || fields.get("state") != Some(&"retained")
        || fields.get("operation").copied() != Some(operation)
        || fields.get("bytes").copied() != Some(size.to_string().as_str())
        || fields.get("digest").copied() != Some(digest)
    {
        return Err("native runtime recovery copy differs from the source frame".into());
    }
    Ok(())
}
async fn send(stream: &tokio::net::UnixStream, file: &File, deadline: Instant) -> io::Result<()> {
    loop {
        wait_until(deadline, stream.writable())
            .await
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "runtime descriptor deadline")
            })??;
        let result = stream.try_io(tokio::io::Interest::WRITABLE, || {
            let rights = [file.as_fd()];
            let mut storage = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
            let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut storage);
            if !ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)) {
                return Err(io::Error::other("runtime descriptor did not fit"));
            }
            rustix::net::sendmsg(
                stream,
                &[io::IoSlice::new(b"S")],
                &mut ancillary,
                rustix::net::SendFlags::empty(),
            )
            .map_err(io::Error::from)
        });
        match result {
            Ok(1) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
            _ => return Err(io::Error::other("incomplete runtime descriptor marker")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_aterm::Options;
    use codex_alab_aterm::fake::FakeAterm;
    use codex_alab_aterm::fake::Mode;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::FileExt;
    use std::os::unix::fs::MetadataExt;

    #[tokio::test]
    async fn data_transport_is_not_a_native_execution_capability() {
        let unconfirmed = Some("aterm has not confirmed support for live session handoff");
        for (status, expected_error) in [
            (
                "OK native source-exec preparation=1 execution=available",
                unconfirmed,
            ),
            (
                "OK native source-exec preparation=1 execution=unavailable",
                unconfirmed,
            ),
            (
                "OK native source-exec preparation=1 execution=unavailable input-fence=1",
                Some("aterm reports that live session handoff is unavailable"),
            ),
            (
                "OK native source-exec preparation=0 execution=unavailable input-fence=1",
                Some("aterm reports that live session handoff is unavailable"),
            ),
            (
                "ERR native source-exec preparation=1 execution=unavailable input-fence=1",
                unconfirmed,
            ),
            (
                "OK native source-exec preparation=1 execution=unavailable input-fence=1 execution=available",
                unconfirmed,
            ),
            (
                "OK native source-exec preparation=0 execution=available",
                unconfirmed,
            ),
            (
                "OK native source-exec preparation=0 execution=available input-fence=1",
                unconfirmed,
            ),
            (
                "OK native source-exec preparation=1 execution=available execution=unavailable",
                unconfirmed,
            ),
            (
                "OK native runtime-data protocol=1 max-bytes=268435456 authority=data-only",
                unconfirmed,
            ),
            (
                "OK native source-exec preparation=1 execution=available input-fence=1",
                None,
            ),
        ] {
            let directory = crate::test_paths::short_tempdir().unwrap();
            let fake = FakeAterm::start(directory.path(), Mode::Current)
                .await
                .unwrap();
            let client = Client::connect(fake.endpoint(), Options::default())
                .await
                .unwrap();
            let command = format!("@{} native-act info source-exec", fake.sid());
            fake.reply_once(&command, &format!("{status}\n"));
            assert_eq!(
                check_execution(&client, Instant::now() + Duration::from_secs(5))
                    .await
                    .err()
                    .as_deref(),
                expected_error,
                "{status}"
            );
            assert!(
                fake.requests()
                    .iter()
                    .all(|line| !line.contains("runtime-data reserve")
                        && !line.contains("execute source-exec"))
            );
        }
    }

    #[tokio::test]
    async fn shifted_task_clock_preserves_native_capability_budget_and_host_refusal() {
        let directory = crate::test_paths::short_tempdir().unwrap();
        let fake = FakeAterm::start(directory.path(), Mode::Current)
            .await
            .unwrap();
        let client = Client::connect(fake.endpoint(), Options::default())
            .await
            .unwrap();
        let command = format!("@{} native-act info source-exec", fake.sid());
        fake.reply_once(
            &command,
            "OK native source-exec preparation=1 execution=unavailable input-fence=1\n",
        );
        let expired = Instant::now();
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(121)).await;
        tokio::time::resume();
        let deadline = Instant::now() + Duration::from_secs(5);
        assert!(tokio::time::Instant::now().into_std() > deadline);
        assert_eq!(
            check_execution(&client, expired).await.unwrap_err(),
            "timed out waiting for aterm to report live session handoff support"
        );
        assert_eq!(
            check_execution(&client, deadline).await.unwrap_err(),
            "aterm reports that live session handoff is unavailable"
        );
        let requests: Vec<_> = fake
            .requests()
            .into_iter()
            .filter(|line| line.contains(" native-act "))
            .collect();
        assert_eq!(requests, [command]);
    }

    #[tokio::test]
    async fn unanswered_capability_query_reports_timeout_before_the_phase_deadline() {
        let directory = crate::test_paths::short_tempdir().unwrap();
        let fake = FakeAterm::start(directory.path(), Mode::Current)
            .await
            .unwrap();
        let options = Options::default();
        let client_timeout = options.timeout;
        let client = Client::connect(fake.endpoint(), options).await.unwrap();
        let query = "native-act info source-exec";
        fake.gate(query);
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut check = std::pin::pin!(check_execution(&client, deadline));
        tokio::select! {
            result = &mut check => panic!("gated query returned {result:?}"),
            seen = fake.wait_for(|host| !host.gated_requests().is_empty()) => assert!(seen),
        }
        assert_eq!(fake.gated_requests(), [format!("@{} {query}", fake.sid())]);
        tokio::time::pause();
        tokio::time::advance(client_timeout + Duration::from_millis(1)).await;
        tokio::time::resume();
        assert_eq!(
            check.await.unwrap_err(),
            "timed out waiting for aterm to report live session handoff support"
        );
        fake.open_gate(query);
        assert!(Instant::now() < deadline);
        assert_eq!(
            check_execution(&client, deadline).await.unwrap_err(),
            "native runtime capability transport failed"
        );
    }

    #[tokio::test]
    async fn abandoned_preparation_never_reaches_a_later_input_seal() {
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::Ordering;

        for simultaneously_ready in [false, true] {
            let (caller, abandoned) = tokio::sync::oneshot::channel();
            let (ready, readiness) = tokio::sync::oneshot::channel();
            let sealed = Arc::new(AtomicBool::new(false));
            let observed = sealed.clone();
            let worker = tokio::spawn(while_requested(abandoned, async move {
                readiness.await.unwrap();
                observed.store(true, Ordering::SeqCst);
                Ok(())
            }));
            tokio::task::yield_now().await;
            drop(caller);
            if simultaneously_ready {
                ready.send(()).unwrap();
            }
            let result = tokio::time::timeout(Duration::from_secs(5), worker)
                .await
                .expect("abandonment must not wait for native readiness")
                .unwrap();
            assert!(result.is_err());
            assert!(!sealed.load(Ordering::SeqCst));
        }

        let (caller, abandoned) = tokio::sync::oneshot::channel();
        assert!(while_requested(abandoned, async { Ok(()) }).await.is_ok());
        drop(caller);
    }

    #[tokio::test]
    async fn recovery_descriptor_is_read_only_unlinked_and_preserves_exact_bytes() {
        let payload = b"codex-runtime-state-v1\n{\"opaque\":true}";
        let (file, digest) = snapshot(payload).unwrap();
        assert_eq!(digest, format!("{:x}", Sha256::digest(payload)));
        let (source, destination) = tokio::net::UnixStream::pair().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        send(&source, &file, deadline).await.unwrap();
        let destination = destination.into_std().unwrap();
        let [right] =
            crate::upgrade_admission::receive_rights(&destination, b"S", Some(deadline)).unwrap();
        let held = File::from(right);
        assert_eq!(held.metadata().unwrap().nlink(), 0);
        // SAFETY: F_GETFL only inspects the live owned descriptor.
        let flags = unsafe { libc::fcntl(held.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
        let mut bytes = vec![0; payload.len()];
        held.read_exact_at(&mut bytes, 0).unwrap();
        assert_eq!(bytes, payload);
        held.read_exact_at(&mut bytes, 0).unwrap();
        assert_eq!(
            bytes, payload,
            "shared offsets do not affect positional recovery reads"
        );
    }

    #[test]
    fn retained_receipt_must_match_the_exact_operation_size_and_digest() {
        let reply = "OK native runtime-data state=retained operation=abc bytes=3 digest=def";
        let valid = fields(reply).unwrap();
        assert_eq!(
            fields("ERR native runtime-data state=failed")
                .unwrap()
                .get("state"),
            Some(&"failed")
        );
        assert!(retained(&valid, "abc", 3, "def").is_ok());
        assert!(retained(&valid, "other", 3, "def").is_err());
        assert!(retained(&valid, "abc", 4, "def").is_err());
        assert!(retained(&valid, "abc", 3, "other").is_err());
        for invalid in [
            "OK native runtime-data state=retained state=retained",
            "OK native runtime-data state=",
            "ERR native runtime data unavailable",
        ] {
            assert!(fields(invalid).is_err());
        }
        for changed in [
            reply.replace("bytes=3", "bytes=03"),
            format!("{reply} authority=execution"),
            reply.replace("state=retained", "state=receiving"),
        ] {
            assert!(retained(&fields(&changed).unwrap(), "abc", 3, "def").is_err());
        }
    }
}
