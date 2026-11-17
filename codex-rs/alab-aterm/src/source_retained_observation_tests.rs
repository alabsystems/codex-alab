// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real authenticated Unix framing/counter/closure controls ONLY. Syntax-only
//! replies never construct an AcceptedOrigin, Ready, Go or SDK permission.
use super::*;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::io::BufReader;

const MEMBER: &str = "11111111111111111111111111111111";
const OPERATION: &str = "22222222222222222222222222222222";
const BIRTH: &str = "retained-wire-syntax-only-birth";
#[derive(Clone, Copy)]
enum Reply {
    Syntax,
    Negative(&'static str),
    WrongCounter,
    Malformed,
    Eof,
}
struct Wire {
    _dir: tempfile::TempDir,
    endpoint: Endpoint,
    requests: Arc<Mutex<Vec<String>>>,
    server: Option<tokio::task::JoinHandle<io::Result<()>>>,
}
impl Wire {
    async fn start(script: Vec<Reply>) -> io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let sock = dir.path().join("aterm-1.sock");
        let token = dir.path().join("aterm-1.token");
        std::fs::write(&token, b"retained-observation-fixture-token\n")?;
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600))?;
        let listener = {
            let _guard = codex_alab_core::native_fd_custody::receiving()?;
            let listener = std::os::unix::net::UnixListener::bind(&sock)?;
            listener.set_nonblocking(true)?;
            tokio::net::UnixListener::from_std(listener)?
        };
        let endpoint = Endpoint {
            sock,
            sid: "t0pe7873g64".into(),
        };
        let sid = endpoint.sid.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let server = tokio::spawn(async move {
            // One physical authenticated channel. No reconnect is accepted.
            let (stream, _) = listener.accept().await?;
            let (read, mut write) = stream.into_split();
            let mut read = BufReader::new(read);
            let mut line = String::new();
            read.read_line(&mut line).await?;
            if line != "AUTH retained-observation-fixture-token\n" {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            line.clear();
            read.read_line(&mut line).await?;
            if line != "version\n" {
                return Err(io::ErrorKind::InvalidData.into());
            }
            write
                .write_all(b"OK version=retained-wire-syntax-only\n")
                .await?;
            let mut previous = None;
            for reply in script {
                line.clear();
                if read.read_line(&mut line).await? == 0 {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                recorded
                    .lock()
                    .map_err(|_| unavailable())?
                    .push(line.trim_end().into());
                let words = line.split_ascii_whitespace().collect::<Vec<_>>();
                let args = words
                    .get(3..)
                    .ok_or_else(unavailable)?
                    .iter()
                    .map(|word| word.split_once('=').ok_or_else(unavailable))
                    .collect::<io::Result<BTreeMap<_, _>>>()?;
                let request = *args.get("request").ok_or_else(unavailable)?;
                let operation = *args.get("operation").ok_or_else(unavailable)?;
                if previous
                    .as_ref()
                    .is_some_and(|old: &String| old >= &request.to_owned())
                {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                previous = Some(request.into());
                match reply {
                    Reply::Negative(status) => {
                        write.write_all(format!("{status}\n").as_bytes()).await?
                    }
                    Reply::Malformed => {
                        write
                            .write_all(b"OK codex-auth controller-retained-origin {\n")
                            .await?
                    }
                    Reply::Eof => return Ok(()),
                    Reply::Syntax | Reply::WrongCounter => {
                        let action = words.get(2).ok_or_else(unavailable)?;
                        let request = if matches!(reply, Reply::WrongCounter) {
                            "00000000000000000000000000000000"
                        } else {
                            request
                        };
                        let frame = serde_json::json!({"schema":1,"request":request,"operation":operation,"member":MEMBER,
                            "session":{"sid":sid,"process_pid":std::process::id(),"process_birth":BIRTH,"epoch":"wire-syntax-only-epoch"},
                            "state":"syntax-only-NOT-current"});
                        write
                            .write_all(format!("OK codex-auth {action} {frame}\n").as_bytes())
                            .await?;
                    }
                }
            }
            line.clear();
            if read.read_line(&mut line).await? != 0 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            Ok(())
        });
        Ok(Self {
            _dir: dir,
            endpoint,
            requests,
            server: Some(server),
        })
    }
    async fn finish(&mut self, abort: bool) -> io::Result<usize> {
        let server = self.server.as_mut().ok_or_else(unavailable)?;
        if abort {
            server.abort();
        }
        // A timeout must leave the SAME task in our custody until abort is
        // observed. Moving the handle into timeout would detach it on expiry.
        let joined = tokio::time::timeout(Duration::from_secs(5), &mut *server).await;
        let result = match joined {
            Ok(result) => result
                .map_err(io::Error::other)
                .and_then(std::convert::identity),
            Err(_) => {
                server.abort();
                let _ = server.await;
                Err(io::ErrorKind::TimedOut.into())
            }
        };
        self.server.take();
        result?;
        Ok(self.requests.lock().map_err(|_| unavailable())?.len())
    }
}
impl Drop for Wire {
    fn drop(&mut self) {
        // Cancellation/panic fallback only. Normal Result paths use finish
        // and observe completion; Drop cannot claim a joined cleanup.
        if let Some(server) = &self.server {
            server.abort();
        }
    }
}
fn check(condition: bool, message: &'static str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}
async fn exercise<F, Fut>(script: Vec<Reply>, expected: usize, run: F) -> io::Result<()>
where
    F: FnOnce(Arc<Transport>) -> Fut,
    Fut: std::future::Future<Output = io::Result<()>>,
{
    let mut wire = Wire::start(script).await?;
    let mut retained = None;
    let result = async {
        let transport = Arc::new(transport(&wire).await?);
        retained = Some(transport.clone());
        run(transport).await
    }
    .await;
    if let Some(transport) = retained {
        transport.release();
    }
    // Always join/abort+join, even when setup or the tested operation failed.
    // Preserve the primary failure after the cleanup attempt completes.
    let cleanup = wire.finish(result.is_err()).await;
    result?;
    check(
        cleanup? == expected,
        "unexpected original-channel request count",
    )
}
async fn transport(wire: &Wire) -> io::Result<Transport> {
    let transport = connect_transport(&wire.endpoint, BIRTH).await?;
    // Establish only common syntax/counter/epoch validation through the real
    // wire. The state cannot decode as public CurrentOrigin; no grant is made.
    let body = transport
        .call(
            "controller-retained-origin",
            OPERATION,
            &format!(" member={MEMBER}"),
            Some(MEMBER),
        )
        .await?;
    if !body.contains("syntax-only-NOT-current") {
        transport.release();
        return Err(unavailable());
    }
    Ok(transport)
}

#[tokio::test]
async fn confirmed_denied_preserves_the_same_channel_for_later_correlated_syntax() -> io::Result<()>
{
    exercise(
        vec![
            Reply::Syntax,
            Reply::Negative("ERR codex-auth denied"),
            Reply::Syntax,
        ],
        3,
        |transport| async move {
            let result = transport
                .observe_retained(
                    RetainedQuery::Origin,
                    OPERATION,
                    &format!(" member={MEMBER}"),
                    MEMBER,
                )
                .await;
            check(
                matches!(result, Err(ObservationError::Refused(_))),
                "expected confirmed denial",
            )?;
            check(
                transport.conn.is_open(),
                "confirmed denial closed original channel",
            )?;
            let result = transport
                .observe_retained(
                    RetainedQuery::Origin,
                    OPERATION,
                    &format!(" member={MEMBER}"),
                    MEMBER,
                )
                .await
                .map_err(ObservationError::into_io)?;
            check(
                result.contains("syntax-only-NOT-current"),
                "missing syntax-only observation",
            )
        },
    )
    .await
}

#[tokio::test]
async fn exhausted_stale_is_a_negative_then_original_channel_can_still_observe() -> io::Result<()> {
    let mut script = vec![Reply::Syntax];
    for _ in 0..8 {
        script.push(Reply::Negative("ERR codex-auth stale"));
    }
    script.push(Reply::Syntax);
    exercise(script, 10, |transport| async move {
        let result = transport
            .observe_retained(
                RetainedQuery::Origin,
                OPERATION,
                &format!(" member={MEMBER}"),
                MEMBER,
            )
            .await;
        check(
            matches!(result, Err(ObservationError::Refused(_))),
            "expected exhausted stale denial",
        )?;
        check(
            transport.conn.is_open(),
            "stale denial closed original channel",
        )?;
        transport
            .observe_retained(
                RetainedQuery::Origin,
                OPERATION,
                &format!(" member={MEMBER}"),
                MEMBER,
            )
            .await
            .map_err(ObservationError::into_io)?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn malformed_counter_and_io_fail_closed_without_reconnect() -> io::Result<()> {
    for fault in [Reply::Malformed, Reply::WrongCounter, Reply::Eof] {
        exercise(vec![Reply::Syntax, fault], 2, |transport| async move {
            let result = transport
                .observe_retained(
                    RetainedQuery::Origin,
                    OPERATION,
                    &format!(" member={MEMBER}"),
                    MEMBER,
                )
                .await;
            check(
                matches!(result, Err(ObservationError::Uncertain(_))),
                "expected uncertainty",
            )?;
            check(
                !transport.conn.is_open(),
                "uncertainty left original channel open",
            )
        })
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn busy_then_wrong_counter_cannot_carry_a_confirmed_negative_flag() -> io::Result<()> {
    exercise(
        vec![
            Reply::Syntax,
            Reply::Negative("ERR codex-auth busy"),
            Reply::WrongCounter,
        ],
        3,
        |transport| async move {
            let result = transport
                .observe_retained(
                    RetainedQuery::Origin,
                    OPERATION,
                    &format!(" member={MEMBER}"),
                    MEMBER,
                )
                .await;
            check(
                matches!(result, Err(ObservationError::Uncertain(_))),
                "protocol failure after Busy became a confirmed denial",
            )?;
            check(
                !transport.conn.is_open(),
                "protocol failure after Busy left original channel open",
            )
        },
    )
    .await
}

#[tokio::test]
async fn generic_call_still_closes_on_a_confirmed_native_denial() -> io::Result<()> {
    exercise(
        vec![Reply::Syntax, Reply::Negative("ERR codex-auth denied")],
        2,
        |transport| async move {
            check(
                transport
                    .call(
                        "controller-retained-origin",
                        OPERATION,
                        &format!(" member={MEMBER}"),
                        Some(MEMBER),
                    )
                    .await
                    .is_err(),
                "generic call accepted denial",
            )?;
            check(
                !transport.conn.is_open(),
                "generic call denial left original channel open",
            )
        },
    )
    .await
}

#[tokio::test]
async fn invalid_request_is_protocol_uncertainty_and_closes_the_original_channel() -> io::Result<()>
{
    exercise(
        vec![
            Reply::Syntax,
            Reply::Negative("ERR codex-auth invalid request"),
        ],
        2,
        |transport| async move {
            let result = transport
                .observe_retained(
                    RetainedQuery::Origin,
                    OPERATION,
                    &format!(" member={MEMBER}"),
                    MEMBER,
                )
                .await;
            check(
                matches!(result, Err(ObservationError::Uncertain(_))),
                "invalid request became a confirmed denial",
            )?;
            check(
                !transport.conn.is_open(),
                "invalid request left original channel open",
            )
        },
    )
    .await
}
