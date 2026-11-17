// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One real HTTP request with bounded phases and a gated semantic SSE failure.

use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const WAIT: Duration = Duration::from_secs(5);
// Longer than the test's selected-outcome deadline: an expired gate must not
// turn an incorrectly deferred immediate error into an apparently valid EOF.
const EOF_RELEASE_WAIT: Duration = Duration::from_secs(15);
const MAX_REQUEST_PART: usize = 64 * 1024;

#[derive(Debug)]
pub(crate) enum FixtureError {
    Deadline(&'static str),
    Io(&'static str, std::io::Error),
    Malformed(&'static str),
    ReleaseCancelled,
    Task(tokio::task::JoinError),
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deadline(phase) => write!(formatter, "SSE fixture {phase} deadline"),
            Self::Io(phase, error) => write!(formatter, "SSE fixture {phase}: {error}"),
            Self::Malformed(reason) => write!(formatter, "SSE fixture malformed request: {reason}"),
            Self::ReleaseCancelled => formatter.write_str("SSE fixture release was cancelled"),
            Self::Task(error) => write!(formatter, "SSE fixture task: {error}"),
        }
    }
}

impl std::error::Error for FixtureError {}

pub(crate) struct SseFixture {
    address: SocketAddr,
    uri: String,
    body_started: oneshot::Receiver<()>,
    release: Option<oneshot::Sender<()>>,
    event_written: oneshot::Receiver<()>,
    eof_release: Option<oneshot::Sender<()>>,
    consumer_read_pending: Option<oneshot::Receiver<()>>,
    task: Option<JoinHandle<Result<(), FixtureError>>>,
}

impl SseFixture {
    pub(crate) async fn start() -> Self {
        Self::start_with_body_deadline(WAIT).await
    }

    async fn start_with_body_deadline(body_deadline: Duration) -> Self {
        Self::start_response(
            body_deadline,
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\nx-request-id: invalid identifier\r\nx-oai-request-id: req_context-1\r\ncf-ray: ray_context-1\r\nauthorization: Bearer never-log-header\r\n\r\n".to_vec(),
            b"data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"invalid_access_program\",\"message\":\"never-log-provider-content\"}}}\n\n".to_vec(),
            false,
            false,
        )
        .await
    }

    pub(crate) async fn start_scripted_response(headers: Vec<u8>, event: Vec<u8>) -> Self {
        Self::start_response(WAIT, headers, event, true, false).await
    }

    pub(crate) async fn start_consumer_close_response(headers: Vec<u8>, event: Vec<u8>) -> Self {
        Self::start_response(WAIT, headers, event, true, true).await
    }

    async fn start_response(
        body_deadline: Duration,
        headers: Vec<u8>,
        event: Vec<u8>,
        hold_eof: bool,
        observe_consumer_close: bool,
    ) -> Self {
        let listener = tokio::time::timeout(WAIT, TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("SSE listener bind deadline")
            .expect("loopback SSE listener");
        let address = listener.local_addr().unwrap();
        let (release, gate) = oneshot::channel();
        let (body_started, body_phase) = oneshot::channel();
        let (event_written, event_phase) = oneshot::channel();
        let (eof_release, eof_gate) = oneshot::channel();
        let (mut pending_notice, consumer_read_pending) = if observe_consumer_close {
            let (notice, pending) = oneshot::channel();
            (Some(notice), Some(pending))
        } else {
            (None, None)
        };
        let task = tokio::spawn(async move {
            let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
                .await
                .map_err(|_| FixtureError::Deadline("accept"))?
                .map_err(|error| FixtureError::Io("accept", error))?;
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            let header_end = tokio::time::timeout(WAIT, async {
                loop {
                    let read = socket
                        .read(&mut buffer)
                        .await
                        .map_err(|error| FixtureError::Io("headers", error))?;
                    if read == 0 {
                        return Err(FixtureError::Malformed("EOF before request headers"));
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        if end + 4 >= MAX_REQUEST_PART {
                            return Err(FixtureError::Malformed("headers exceed bound"));
                        }
                        return Ok(end + 4);
                    }
                    if request.len() >= MAX_REQUEST_PART {
                        return Err(FixtureError::Malformed("headers exceed bound"));
                    }
                }
            })
            .await
            .map_err(|_| FixtureError::Deadline("headers"))??;
            let request_headers = std::str::from_utf8(&request[..header_end])
                .map_err(|_| FixtureError::Malformed("non-UTF8 headers"))?;
            let content_length = request_headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .ok_or(FixtureError::Malformed("missing or invalid Content-Length"))?;
            if content_length >= MAX_REQUEST_PART {
                return Err(FixtureError::Malformed("body exceeds bound"));
            }
            let _ = body_started.send(());
            tokio::time::timeout(body_deadline, async {
                while request.len() < header_end + content_length {
                    let read = socket
                        .read(&mut buffer)
                        .await
                        .map_err(|error| FixtureError::Io("body", error))?;
                    if read == 0 {
                        return Err(FixtureError::Malformed("EOF before complete request body"));
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                Ok(())
            })
            .await
            .map_err(|_| FixtureError::Deadline("body"))??;
            tokio::time::timeout(WAIT, socket.write_all(&headers))
                .await
                .map_err(|_| FixtureError::Deadline("opening headers"))?
                .map_err(|error| FixtureError::Io("opening headers", error))?;
            tokio::time::timeout(WAIT, gate)
                .await
                .map_err(|_| FixtureError::Deadline("release"))?
                .map_err(|_| FixtureError::ReleaseCancelled)?;
            tokio::time::timeout(WAIT, socket.write_all(&event))
                .await
                .map_err(|_| FixtureError::Deadline("event send"))?
                .map_err(|error| FixtureError::Io("event send", error))?;
            let _ = event_written.send(());
            if observe_consumer_close {
                let mut unexpected = [0_u8; 1];
                let mut read = Box::pin(socket.read(&mut unexpected));
                let closed = tokio::time::timeout(
                    WAIT,
                    std::future::poll_fn(|context| {
                        let result = read.as_mut().poll(context);
                        if result.is_pending()
                            && let Some(notice) = pending_notice.take()
                        {
                            let _ = notice.send(());
                        }
                        result
                    }),
                )
                .await
                .map_err(|_| FixtureError::Deadline("consumer close"))?;
                // Only this read may establish consumer retirement. Do not
                // release EOF or close the server socket to create evidence.
                return match closed {
                    Ok(0) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => Ok(()),
                    Ok(_) => Err(FixtureError::Malformed("data after complete request")),
                    Err(error) => Err(FixtureError::Io("consumer close", error)),
                };
            }
            if hold_eof {
                tokio::time::timeout(EOF_RELEASE_WAIT, eof_gate)
                    .await
                    .map_err(|_| FixtureError::Deadline("EOF release"))?
                    .map_err(|_| FixtureError::ReleaseCancelled)?;
            }
            let closed = tokio::time::timeout(WAIT, socket.shutdown())
                .await
                .map_err(|_| FixtureError::Deadline("close"))?;
            match closed {
                Ok(()) => {}
                Err(error)
                    if hold_eof
                        && matches!(
                            error.kind(),
                            std::io::ErrorKind::BrokenPipe
                                | std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::ConnectionAborted
                                | std::io::ErrorKind::NotConnected
                        ) => {}
                Err(error) => return Err(FixtureError::Io("close", error)),
            }
            Ok(())
        });
        Self {
            address,
            uri: format!("http://{address}"),
            body_started: body_phase,
            release: Some(release),
            event_written: event_phase,
            eof_release: hold_eof.then_some(eof_release),
            consumer_read_pending,
            task: Some(task),
        }
    }

    pub(crate) fn uri(&self) -> &str {
        &self.uri
    }

    pub(crate) fn release_failure(&mut self) {
        self.release
            .take()
            .expect("one failure release")
            .send(())
            .expect("server awaits caller context exit");
    }

    pub(crate) async fn wait_for_event_written(&mut self) {
        tokio::time::timeout(WAIT, &mut self.event_written)
            .await
            .expect("event write observation deadline")
            .expect("server wrote the complete event");
    }

    pub(crate) async fn wait_for_consumer_read_pending(&mut self) {
        tokio::time::timeout(
            WAIT,
            self.consumer_read_pending
                .take()
                .expect("consumer-close fixture has one pending-read witness"),
        )
        .await
        .expect("peer consumer-read pending deadline")
        .expect("peer read was actually polled pending before consumer drop");
    }

    pub(crate) fn eof_is_held(&self) -> bool {
        self.eof_release
            .as_ref()
            .is_some_and(|release| !release.is_closed())
    }

    pub(crate) fn release_eof(&mut self) {
        self.eof_release
            .take()
            .expect("one scripted EOF release")
            .send(())
            .expect("server is still holding EOF");
    }

    async fn wait_for_body_phase(&mut self) {
        tokio::time::timeout(WAIT, &mut self.body_started)
            .await
            .expect("body phase observation deadline")
            .expect("server reached body phase");
    }

    pub(crate) async fn finish(mut self) -> Result<(), FixtureError> {
        // Keep ownership during the await: cancellation of finish must also abort
        // the server through Drop, rather than detach its JoinHandle.
        match tokio::time::timeout(WAIT, self.task.as_mut().expect("one server task")).await {
            Ok(result) => {
                drop(self.task.take());
                result.map_err(FixtureError::Task)?
            }
            Err(_) => {
                let task = self.task.take().expect("one server task");
                task.abort();
                let _ = task.await;
                Err(FixtureError::Deadline("join"))
            }
        }
    }
}

impl Drop for SseFixture {
    fn drop(&mut self) {
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;

    async fn incomplete_request(fixture: &mut SseFixture) -> TcpStream {
        let mut peer = tokio::time::timeout(WAIT, TcpStream::connect(fixture.address))
            .await
            .expect("peer connect deadline")
            .unwrap();
        tokio::time::timeout(
            WAIT,
            peer.write_all(
                b"POST /v1/responses HTTP/1.1\r\nhost: localhost\r\ncontent-length: 4\r\n\r\nx",
            ),
        )
        .await
        .expect("partial request send deadline")
        .unwrap();
        fixture.wait_for_body_phase().await;
        peer
    }

    async fn assert_peer_closed(peer: &mut TcpStream) {
        let mut byte = [0_u8; 1];
        match tokio::time::timeout(WAIT, peer.read(&mut byte))
            .await
            .expect("fixture closes the real connection")
        {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ) => {}
            result => panic!("expected EOF or reset without opening response headers: {result:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn incomplete_content_length_body_hits_named_deadline_and_closes_connection() {
        let mut fixture = SseFixture::start_with_body_deadline(Duration::from_millis(100)).await;
        let mut peer = incomplete_request(&mut fixture).await;
        let error = fixture
            .finish()
            .await
            .expect_err("an incomplete real body must not hang the fixture");
        assert!(matches!(error, FixtureError::Deadline("body")), "{error}");
        assert_peer_closed(&mut peer).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dropping_fixture_with_incomplete_body_closes_connection() {
        let mut fixture = SseFixture::start().await;
        let mut peer = incomplete_request(&mut fixture).await;
        drop(fixture);
        assert_peer_closed(&mut peer).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelling_finish_with_incomplete_body_closes_connection() {
        // The body timer is irrelevant to this cancellation check. The fixture
        // has observed a real partial request before finish is polled once.
        let mut fixture = SseFixture::start_with_body_deadline(Duration::from_secs(60)).await;
        let mut peer = incomplete_request(&mut fixture).await;
        let mut finish = Box::pin(fixture.finish());
        assert!(futures::poll!(finish.as_mut()).is_pending());
        drop(finish);
        assert_peer_closed(&mut peer).await;
    }
}
