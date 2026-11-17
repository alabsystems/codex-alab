// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise framing through the real client handshake without timing guesses.

use super::*;
use futures::StreamExt;
use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tokio::io::AsyncReadExt;
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const BODY: &[u8] = b"{\"detail\":\"Bad Request\"}";

enum End {
    Eof,
    Pending,
    Error,
    Policy,
}

struct ScriptedIo {
    chunks: VecDeque<Vec<u8>>,
    at: usize,
    end: End,
    gate_after_first: Option<oneshot::Receiver<()>>,
    first_observed: Option<oneshot::Sender<()>>,
    chunks_read: usize,
}

impl ScriptedIo {
    fn new(chunks: Vec<Vec<u8>>, end: End) -> Self {
        Self {
            chunks: chunks.into(),
            at: 0,
            end,
            gate_after_first: None,
            first_observed: None,
            chunks_read: 0,
        }
    }
}

impl AsyncRead for ScriptedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.chunks_read > 0
            && let Some(gate) = self.gate_after_first.as_mut()
        {
            match Pin::new(gate).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => {
                    result.expect("release scripted response fragment");
                    self.gate_after_first = None;
                }
            }
        }
        if let Some(chunk) = self.chunks.front() {
            let chunk_len = chunk.len();
            let take = (chunk_len - self.at).min(output.remaining());
            output.put_slice(&chunk[self.at..self.at + take]);
            self.at += take;
            if self.at == chunk_len {
                self.chunks.pop_front();
                self.at = 0;
                self.chunks_read += 1;
                if let Some(observed) = self.first_observed.take() {
                    let _ = observed.send(());
                }
            }
            return Poll::Ready(Ok(()));
        }
        match self.end {
            End::Eof => Poll::Ready(Ok(())),
            End::Pending => Poll::Pending,
            End::Error => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "scripted read failure",
            ))),
            End::Policy => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                codex_http_client::NetworkPolicyDenied::Revoked,
            ))),
        }
    }
}

impl AsyncWrite for ScriptedIo {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[derive(Default)]
struct IoOwnership {
    polls: AtomicUsize,
    pending_reads: AtomicUsize,
    drops: AtomicUsize,
}

struct DropProbe {
    inner: ScriptedIo,
    ownership: Arc<IoOwnership>,
}

impl AsyncRead for DropProbe {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.ownership.polls.fetch_add(1, Ordering::SeqCst);
        let result = Pin::new(&mut this.inner).poll_read(cx, output);
        if result.is_pending() {
            this.ownership.pending_reads.fetch_add(1, Ordering::SeqCst);
        }
        result
    }
}

impl AsyncWrite for DropProbe {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.ownership.polls.fetch_add(1, Ordering::SeqCst);
        Pin::new(&mut this.inner).poll_write(cx, bytes)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.ownership.polls.fetch_add(1, Ordering::SeqCst);
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.ownership.polls.fetch_add(1, Ordering::SeqCst);
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.ownership.drops.fetch_add(1, Ordering::SeqCst);
    }
}

fn request() -> Request {
    "ws://127.0.0.1/v1/responses".into_client_request().unwrap()
}

fn head(status: u16, fields: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status} Rejected\r\nx-request-id: req_framing-1\r\nRetry-After: 10\r\n{fields}\r\n").into_bytes()
}

async fn rejected(stream: ScriptedIo) -> Response {
    match tokio::time::timeout(
        Duration::from_secs(20),
        client_async_tls_with_config(request(), stream, None, None),
    )
    .await
    .expect("bounded client handshake")
    {
        Err(WebSocketError::Http(response)) => *response,
        _ => panic!("expected original HTTP rejection"),
    }
}

async fn next_message(websocket: &mut WebSocketStream<GuardedStream<ScriptedIo>>) -> Message {
    tokio::time::timeout(Duration::from_secs(1), websocket.next())
        .await
        .expect("bounded response frame")
        .expect("response frame")
        .expect("valid response frame")
}

fn evidence(response: &Response, status: u16, body: Option<&[u8]>, outcome: RejectionBodyOutcome) {
    assert_eq!(response.status().as_u16(), status);
    assert_eq!(response.headers()["x-request-id"], "req_framing-1");
    assert_eq!(response.headers()["retry-after"], "10");
    assert_eq!(response.body().as_deref(), body);
    let metadata = response
        .extensions()
        .get::<RejectionMetadata>()
        .expect("captured header-time evidence");
    assert_eq!(metadata.outcome, outcome);
    assert!(metadata.retry_after.is_some());
    assert_eq!(metadata.decoded_body_bytes, body.map(<[u8]>::len));
}

fn progress_evidence(
    response: &Response,
    body: Option<&[u8]>,
    outcome: RejectionBodyOutcome,
    accepted: Option<usize>,
) {
    evidence(response, 403, body, outcome);
    let metadata = response.extensions().get::<RejectionMetadata>().unwrap();
    assert!(
        metadata
            .entity_bytes_accepted
            .is_none_or(|bytes| bytes <= MAX_BODY)
    );
    assert_eq!(metadata.entity_bytes_accepted, accepted);
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_pending_rejection_retires_io_without_capture_or_later_polls() {
    // This is the real plain-stream adapter and Tungstenite handshake boundary,
    // before the outer TLS wrapper attaches captured metadata to an HTTP error.
    let capture = Arc::new(Mutex::new(None));
    let ownership = Arc::new(IoOwnership::default());
    let guarded = RejectionStream::new(
        MaybeTlsStream::Plain(DropProbe {
            inner: ScriptedIo::new(
                vec![
                    head(403, &format!("Content-Length: {}\r\n", BODY.len())),
                    BODY.to_vec(),
                ],
                End::Pending,
            ),
            ownership: ownership.clone(),
        }),
        capture.clone(),
    );
    let error = match tokio::time::timeout(
        Duration::from_secs(20),
        tokio_tungstenite::client_async_with_config(request(), guarded, None),
    )
    .await
    .expect("bounded healthy capture control")
    {
        Err(error) => error,
        Ok(_) => panic!("the healthy capture control must retain the HTTP rejection"),
    };
    let WebSocketError::Http(response) = error else {
        panic!("the healthy capture control must preserve its original HTTP type");
    };
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(response.headers()["x-request-id"], "req_framing-1");
    assert_eq!(response.headers()["retry-after"], "10");
    let captured = capture
        .lock()
        .unwrap()
        .take()
        .expect("healthy completed capture");
    assert_eq!(captured.body.as_deref(), Some(BODY));
    assert_eq!(captured.metadata.outcome, RejectionBodyOutcome::Complete);
    assert_eq!(captured.metadata.decoded_body_bytes, Some(BODY.len()));
    assert_eq!(captured.metadata.entity_bytes_accepted, Some(BODY.len()));
    assert!(captured.metadata.retry_after.is_some());
    assert_eq!(ownership.pending_reads.load(Ordering::SeqCst), 0);
    assert_eq!(ownership.drops.load(Ordering::SeqCst), 1);

    for prefix in [false, true] {
        for deadline_ready in [false, true] {
            let capture = Arc::new(Mutex::new(None));
            let ownership = Arc::new(IoOwnership::default());
            let mut fragments = vec![head(
                403,
                &format!("Content-Length: {}\r\n", BODY.len() + 1),
            )];
            if prefix {
                fragments.push(BODY.to_vec());
            }
            let guarded = RejectionStream::new(
                MaybeTlsStream::Plain(DropProbe {
                    inner: ScriptedIo::new(fragments, End::Pending),
                    ownership: ownership.clone(),
                }),
                capture.clone(),
            );
            let started = Instant::now();
            let mut handshake = Box::pin(tokio_tungstenite::client_async_with_config(
                request(),
                guarded,
                None,
            ));
            assert!(futures::poll!(&mut handshake).is_pending());
            // The pending underlying read follows all ready header/prefix reads,
            // so the adapter has already processed those bytes at this point.
            let pending_reads = ownership.pending_reads.load(Ordering::SeqCst);
            assert!(pending_reads > 0);
            assert_eq!(Instant::now(), started);
            assert_eq!(ownership.drops.load(Ordering::SeqCst), 0);
            assert!(capture.lock().unwrap().is_none());
            let polls = ownership.polls.load(Ordering::SeqCst);
            if deadline_ready {
                tokio::time::advance(BODY_TIMEOUT).await;
                assert_eq!(Instant::now() - started, BODY_TIMEOUT);
                // Waking a timer does not poll this locally owned future.
                assert_eq!(ownership.polls.load(Ordering::SeqCst), polls);
                assert!(capture.lock().unwrap().is_none());
            }
            drop(handshake);
            assert_eq!(ownership.drops.load(Ordering::SeqCst), 1);
            assert!(capture.lock().unwrap().is_none());
            tokio::time::advance(BODY_TIMEOUT + Duration::from_secs(1)).await;
            tokio::task::yield_now().await;
            assert_eq!(ownership.polls.load(Ordering::SeqCst), polls);
            assert_eq!(
                ownership.pending_reads.load(Ordering::SeqCst),
                pending_reads
            );
            assert_eq!(ownership.drops.load(Ordering::SeqCst), 1);
            assert!(capture.lock().unwrap().is_none());
        }
    }
}

#[tokio::test]
async fn accepted_entity_progress_distinguishes_absent_decoder_from_measured_zero() {
    for (fields, outcome) in [
        (
            format!("Content-Length: {}\r\n", MAX_BODY + 1),
            RejectionBodyOutcome::TooLarge,
        ),
        (
            "Content-Length: 1\r\nContent-Length: 2\r\n".to_owned(),
            RejectionBodyOutcome::Malformed,
        ),
        (
            "Transfer-Encoding: gzip, chunked\r\n".to_owned(),
            RejectionBodyOutcome::Unsupported,
        ),
        (
            "Content-Encoding: gzip\r\nContent-Length: 1\r\n".to_owned(),
            RejectionBodyOutcome::Unsupported,
        ),
    ] {
        let response = rejected(ScriptedIo::new(
            vec![head(403, &fields), BODY.to_vec()],
            End::Eof,
        ))
        .await;
        progress_evidence(&response, None, outcome, None);
    }

    for (fields, bytes, end, outcome, body) in [
        (
            "Content-Length: 0\r\n",
            Vec::new(),
            End::Pending,
            RejectionBodyOutcome::Complete,
            Some(b"".as_slice()),
        ),
        (
            "Content-Length: 1\r\n",
            Vec::new(),
            End::Eof,
            RejectionBodyOutcome::Truncated,
            None,
        ),
        (
            "Transfer-Encoding: chunked\r\n",
            format!("{:x}\r\n", MAX_BODY + 1).into_bytes(),
            End::Pending,
            RejectionBodyOutcome::TooLarge,
            None,
        ),
    ] {
        let response = rejected(ScriptedIo::new(vec![head(403, fields), bytes], end)).await;
        progress_evidence(&response, body, outcome, Some(0));
    }
}

#[tokio::test]
async fn accepted_entity_progress_preserves_partial_and_complete_decoder_counts() {
    for (end, outcome) in [
        (End::Eof, RejectionBodyOutcome::Truncated),
        (End::Error, RejectionBodyOutcome::ReadError),
    ] {
        let response = rejected(ScriptedIo::new(
            vec![
                head(403, &format!("Content-Length: {}\r\n", BODY.len() + 1)),
                BODY.to_vec(),
            ],
            end,
        ))
        .await;
        progress_evidence(&response, None, outcome, Some(BODY.len()));
    }

    for (tail, outcome, body) in [
        (
            b"\r\n0\r\nmalformed-trailer\r\n\r\n".to_vec(),
            RejectionBodyOutcome::Malformed,
            None,
        ),
        (
            format!("\r\n{:x}\r\n", MAX_BODY - BODY.len() + 1).into_bytes(),
            RejectionBodyOutcome::TooLarge,
            None,
        ),
        (
            b"\r\n0\r\nx-finished: yes\r\n\r\n".to_vec(),
            RejectionBodyOutcome::Complete,
            Some(BODY),
        ),
    ] {
        let wire = [
            format!("{:x};part=body\r\n", BODY.len()).into_bytes(),
            BODY.to_vec(),
            tail,
        ]
        .concat();
        assert!(
            wire.len() > BODY.len(),
            "framing must not count as entity data"
        );
        let response = rejected(ScriptedIo::new(
            vec![head(403, "Transfer-Encoding: chunked\r\n"), wire],
            End::Pending,
        ))
        .await;
        progress_evidence(&response, body, outcome, Some(BODY.len()));
    }

    let response = rejected(ScriptedIo::new(
        vec![head(403, ""), BODY.to_vec()],
        End::Eof,
    ))
    .await;
    progress_evidence(
        &response,
        Some(BODY),
        RejectionBodyOutcome::Complete,
        Some(BODY.len()),
    );

    let body = vec![b'x'; MAX_BODY];
    let response = rejected(ScriptedIo::new(
        vec![
            head(403, &format!("Content-Length: {MAX_BODY}\r\n")),
            body.clone(),
        ],
        End::Pending,
    ))
    .await;
    progress_evidence(
        &response,
        Some(&body),
        RejectionBodyOutcome::Complete,
        Some(MAX_BODY),
    );
}

#[tokio::test(start_paused = true)]
async fn accepted_entity_progress_survives_empty_and_partial_body_timeouts() {
    for prefix in [Vec::new(), BODY.to_vec()] {
        let accepted = prefix.len();
        let started = Instant::now();
        let mut fragments = vec![head(
            403,
            &format!("Content-Length: {}\r\n", BODY.len() + 1),
        )];
        if !prefix.is_empty() {
            fragments.push(prefix);
        }
        let mut handshake = Box::pin(rejected(ScriptedIo::new(fragments, End::Pending)));
        // Ready scripted bytes pass through the decoder before the pending read.
        assert!(futures::poll!(&mut handshake).is_pending());
        assert_eq!(Instant::now(), started);
        let response = handshake.await;
        assert_eq!(Instant::now() - started, BODY_TIMEOUT);
        assert_eq!(
            response
                .extensions()
                .get::<RejectionMetadata>()
                .unwrap()
                .capture_elapsed,
            BODY_TIMEOUT
        );
        progress_evidence(
            &response,
            None,
            RejectionBodyOutcome::TimedOut,
            Some(accepted),
        );
    }
}

#[tokio::test]
async fn declared_rejection_body_survives_every_body_byte_fragment() {
    for status in [400, 403] {
        let headers = head(status, &format!("Content-Length: {}\r\n", BODY.len()));
        let mut fragments = vec![headers];
        fragments.extend(BODY.iter().map(|byte| vec![*byte]));
        let response = rejected(ScriptedIo::new(fragments, End::Pending)).await;
        evidence(
            &response,
            status,
            Some(BODY),
            RejectionBodyOutcome::Complete,
        );
    }
}

#[tokio::test]
async fn fragmented_headers_are_parsed_without_discarding_the_body() {
    let headers = head(403, &format!("Content-Length: {}\r\n", BODY.len()));
    let mut fragments = headers.chunks(23).map(<[u8]>::to_vec).collect::<Vec<_>>();
    fragments.extend(BODY.chunks(3).map(<[u8]>::to_vec));
    let response = rejected(ScriptedIo::new(fragments, End::Pending)).await;
    evidence(&response, 403, Some(BODY), RejectionBodyOutcome::Complete);
}

fn near_limit_headers(mut headers: Vec<u8>) -> Vec<u8> {
    headers.truncate(headers.len() - 2);
    headers.extend_from_slice(b"x-padding: ");
    headers.resize(MAX_HEADERS - 9, b'p');
    headers.extend_from_slice(b"\r\n\r\n");
    assert_eq!(headers.len(), MAX_HEADERS - 5);
    headers
}

fn header_and_tail_chunks(headers: Vec<u8>, tail: &[u8]) -> Vec<Vec<u8>> {
    let split = headers.len() - 17;
    vec![
        headers[..split].to_vec(),
        [&headers[split..], tail].concat(),
    ]
}

#[tokio::test]
async fn header_budget_does_not_count_the_same_read_rejection_body() {
    let headers = near_limit_headers(head(400, &format!("Content-Length: {}\r\n", BODY.len())));
    let response = rejected(ScriptedIo::new(
        header_and_tail_chunks(headers, BODY),
        End::Pending,
    ))
    .await;
    evidence(&response, 400, Some(BODY), RejectionBodyOutcome::Complete);
}

#[tokio::test]
async fn chunked_body_is_decoded_after_its_trailer_terminator() {
    let split = BODY.len() / 2;
    let mut wire = format!("{split:x};name=token;quoted=\"a\\\"b\"\r\n").into_bytes();
    wire.extend_from_slice(&BODY[..split]);
    wire.extend_from_slice(format!("\r\n{:x}\r\n", BODY.len() - split).as_bytes());
    wire.extend_from_slice(&BODY[split..]);
    wire.extend_from_slice(b"\r\n0\r\nx-finished: yes\r\n\r\n");
    let mut fragments = vec![head(400, "Transfer-Encoding: chunked\r\n")];
    fragments.extend(wire.into_iter().map(|byte| vec![byte]));
    let response = rejected(ScriptedIo::new(fragments, End::Pending)).await;
    evidence(&response, 400, Some(BODY), RejectionBodyOutcome::Complete);
}

#[tokio::test]
async fn eof_delimited_rejection_requires_actual_eof() {
    let response = rejected(ScriptedIo::new(
        vec![head(400, ""), BODY.to_vec()],
        End::Eof,
    ))
    .await;
    evidence(&response, 400, Some(BODY), RejectionBodyOutcome::Complete);
}

#[tokio::test]
async fn incomplete_and_ambiguous_framing_never_exposes_a_complete_json_prefix() {
    let cases = [
        (
            format!("Content-Length: {}\r\n", BODY.len() + 1),
            BODY.to_vec(),
            RejectionBodyOutcome::Truncated,
        ),
        (
            "Transfer-Encoding: chunked\r\n".into(),
            [
                format!("{:x}\r\n", BODY.len()).into_bytes(),
                BODY.to_vec(),
                b"\r\n0\r\n".to_vec(),
            ]
            .concat(),
            RejectionBodyOutcome::Truncated,
        ),
        (
            "Transfer-Encoding: chunked\r\n".into(),
            [
                format!("{:x}\r\n", BODY.len()).into_bytes(),
                BODY.to_vec(),
                b"X\n0\r\n\r\n".to_vec(),
            ]
            .concat(),
            RejectionBodyOutcome::Malformed,
        ),
        (
            format!(
                "Content-Length: {}\r\nContent-Length: {}\r\n",
                BODY.len(),
                BODY.len() + 1
            ),
            BODY.to_vec(),
            RejectionBodyOutcome::Malformed,
        ),
        (
            format!(
                "Transfer-Encoding: chunked\r\nContent-Length: {}\r\n",
                BODY.len()
            ),
            BODY.to_vec(),
            RejectionBodyOutcome::Malformed,
        ),
        (
            "Transfer-Encoding: gzip, chunked\r\n".into(),
            BODY.to_vec(),
            RejectionBodyOutcome::Unsupported,
        ),
        (
            format!(
                "Content-Encoding: gzip\r\nContent-Length: {}\r\n",
                BODY.len()
            ),
            BODY.to_vec(),
            RejectionBodyOutcome::Unsupported,
        ),
    ];
    for (fields, bytes, outcome) in cases {
        let response = rejected(ScriptedIo::new(vec![head(403, &fields), bytes], End::Eof)).await;
        evidence(&response, 403, None, outcome);
    }
}

#[tokio::test]
async fn equal_content_lengths_are_unambiguous_and_zero_is_measured() {
    let response = rejected(ScriptedIo::new(
        vec![
            head(
                400,
                &format!(
                    "Content-Length: {}, {}\r\nContent-Length: {}\r\n",
                    BODY.len(),
                    BODY.len(),
                    BODY.len()
                ),
            ),
            BODY.to_vec(),
        ],
        End::Pending,
    ))
    .await;
    evidence(&response, 400, Some(BODY), RejectionBodyOutcome::Complete);
    let response = rejected(ScriptedIo::new(
        vec![head(400, "Content-Length: 0\r\n")],
        End::Pending,
    ))
    .await;
    evidence(&response, 400, Some(b""), RejectionBodyOutcome::Complete);
}

#[tokio::test]
async fn body_and_chunk_metadata_limits_preserve_http_refusal_without_partial_body() {
    let fields = format!("Content-Length: {}\r\n", MAX_BODY + 1);
    let response = rejected(ScriptedIo::new(
        vec![head(403, &fields), BODY.to_vec()],
        End::Pending,
    ))
    .await;
    evidence(&response, 403, None, RejectionBodyOutcome::TooLarge);

    let trailers = "x-metadata: ".to_owned() + &"x".repeat(200) + "\r\n";
    let response = rejected(ScriptedIo::new(
        vec![
            head(403, "Transfer-Encoding: chunked\r\n"),
            [
                format!("{:x}\r\n", BODY.len()).into_bytes(),
                BODY.to_vec(),
                b"\r\n0\r\n".to_vec(),
                trailers.repeat(100).into_bytes(),
                b"\r\n".to_vec(),
            ]
            .concat(),
        ],
        End::Pending,
    ))
    .await;
    evidence(&response, 403, None, RejectionBodyOutcome::TooLarge);

    let response = rejected(ScriptedIo::new(
        vec![head(400, ""), vec![b'x'; MAX_BODY + 1]],
        End::Eof,
    ))
    .await;
    evidence(&response, 400, None, RejectionBodyOutcome::TooLarge);

    let oversized_line = format!("1;{}\r\nx\r\n0\r\n\r\n", "a".repeat(MAX_LINE));
    let response = rejected(ScriptedIo::new(
        vec![
            head(403, "Transfer-Encoding: chunked\r\n"),
            oversized_line.into_bytes(),
        ],
        End::Pending,
    ))
    .await;
    evidence(&response, 403, None, RejectionBodyOutcome::TooLarge);

    let body = vec![b'x'; MAX_BODY];
    let response = rejected(ScriptedIo::new(
        vec![
            head(400, &format!("Content-Length: {MAX_BODY}\r\n")),
            body.clone(),
        ],
        End::Pending,
    ))
    .await;
    evidence(&response, 400, Some(&body), RejectionBodyOutcome::Complete);
}

#[tokio::test(start_paused = true)]
async fn body_deadline_is_absolute_and_never_accepts_a_json_prefix() {
    for fields in [
        "".to_owned(),
        format!("Content-Length: {}\r\n", BODY.len() + 1),
    ] {
        let started = Instant::now();
        let response = rejected(ScriptedIo::new(
            vec![head(403, &fields), BODY.to_vec()],
            End::Pending,
        ))
        .await;
        evidence(&response, 403, None, RejectionBodyOutcome::TimedOut);
        assert_eq!(Instant::now() - started, BODY_TIMEOUT);
        assert_eq!(
            response
                .extensions()
                .get::<RejectionMetadata>()
                .unwrap()
                .capture_elapsed,
            BODY_TIMEOUT
        );
    }
}

#[tokio::test(start_paused = true)]
async fn delayed_body_keeps_retry_advice_anchored_to_header_receipt() {
    let (observed, ready) = oneshot::channel();
    let (release, gate) = oneshot::channel();
    let mut stream = ScriptedIo::new(
        vec![
            head(400, &format!("Content-Length: {}\r\n", BODY.len())),
            BODY.to_vec(),
        ],
        End::Pending,
    );
    stream.first_observed = Some(observed);
    stream.gate_after_first = Some(gate);
    let started = Instant::now();
    let (response, ()) = tokio::join!(rejected(stream), async {
        ready.await.unwrap();
        tokio::time::advance(Duration::from_secs(3)).await;
        release.send(()).unwrap();
    });
    evidence(&response, 400, Some(BODY), RejectionBodyOutcome::Complete);
    let advice = response
        .extensions()
        .get::<RejectionMetadata>()
        .unwrap()
        .retry_after
        .unwrap();
    assert_eq!(advice.deadline(), started + Duration::from_secs(10));
    assert_eq!(advice.remaining_delay(), Duration::from_secs(7));
    assert_eq!(
        response
            .extensions()
            .get::<RejectionMetadata>()
            .unwrap()
            .capture_elapsed,
        Duration::from_secs(3)
    );
}

#[tokio::test(start_paused = true)]
async fn later_body_fragments_do_not_restart_the_absolute_deadline() {
    let (observed, ready) = oneshot::channel();
    let (release, gate) = oneshot::channel();
    let mut stream = ScriptedIo::new(
        vec![
            head(403, &format!("Content-Length: {}\r\n", BODY.len() + 1)),
            BODY.to_vec(),
        ],
        End::Pending,
    );
    stream.first_observed = Some(observed);
    stream.gate_after_first = Some(gate);
    let started = Instant::now();
    let (response, ()) = tokio::join!(rejected(stream), async {
        ready.await.unwrap();
        tokio::time::advance(Duration::from_secs(3)).await;
        release.send(()).unwrap();
    });
    evidence(&response, 403, None, RejectionBodyOutcome::TimedOut);
    assert_eq!(Instant::now() - started, BODY_TIMEOUT);
}

#[tokio::test(start_paused = true)]
async fn header_read_budget_bounds_the_prebuffer() {
    let mut chunks = vec![b"HTTP/1.1 400 Rejected\r\nx-incomplete: ".to_vec()];
    chunks.extend((0..64).map(|_| b"x".to_vec()));
    let error = match client_async_tls_with_config(
        request(),
        ScriptedIo::new(chunks, End::Pending),
        None,
        None,
    )
    .await
    {
        Err(error) => error,
        Ok(_) => panic!("tiny header reads must retain the original attack guard"),
    };
    assert!(
        matches!(error, WebSocketError::Io(ref error) if error.kind() == io::ErrorKind::InvalidData)
    );
}

#[tokio::test(start_paused = true)]
async fn successful_upgrade_uses_the_callers_budget_for_delayed_headers() {
    let (observed, ready) = oneshot::channel();
    let (release, gate) = oneshot::channel();
    let mut request = request();
    request.headers_mut().insert(
        "sec-websocket-key",
        "dGhlIHNhbXBsZSBub25jZQ==".parse().unwrap(),
    );
    let mut tail = b"Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n".to_vec();
    tail.extend_from_slice(&[0x81, 2, b'o', b'k']);
    let mut stream = ScriptedIo::new(
        vec![b"HTTP/1.1 101 Switching Protocols\r\n".to_vec(), tail],
        End::Pending,
    );
    stream.first_observed = Some(observed);
    stream.gate_after_first = Some(gate);
    let started = Instant::now();
    let (result, ()) = tokio::join!(
        tokio::time::timeout(
            Duration::from_secs(60),
            client_async_tls_with_config(request, stream, None, None),
        ),
        async {
            ready.await.unwrap();
            tokio::time::advance(Duration::from_secs(20)).await;
            release.send(()).unwrap();
        }
    );
    let (mut websocket, response) = result.expect("caller connection budget").unwrap();
    assert_eq!(Instant::now() - started, Duration::from_secs(20));
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert!(response.extensions().get::<RejectionMetadata>().is_none());
    assert_eq!(next_message(&mut websocket).await, Message::text("ok"));
}

#[tokio::test(start_paused = true)]
async fn delayed_rejection_headers_start_a_full_new_entity_budget() {
    let (observed, ready) = oneshot::channel();
    let (release, gate) = oneshot::channel();
    let headers = head(403, &format!("Content-Length: {}\r\n", BODY.len() + 1));
    let split = b"HTTP/1.1 403 Rejected\r\n".len();
    let mut stream = ScriptedIo::new(
        vec![
            headers[..split].to_vec(),
            [&headers[split..], BODY].concat(),
        ],
        End::Pending,
    );
    stream.first_observed = Some(observed);
    stream.gate_after_first = Some(gate);
    let started = Instant::now();
    let (result, ()) = tokio::join!(
        tokio::time::timeout(
            Duration::from_secs(60),
            client_async_tls_with_config(request(), stream, None, None),
        ),
        async {
            ready.await.unwrap();
            tokio::time::advance(Duration::from_secs(20)).await;
            release.send(()).unwrap();
        }
    );
    let response = match result.expect("caller connection budget") {
        Err(WebSocketError::Http(response)) => response,
        _ => panic!("expected original HTTP rejection"),
    };
    evidence(&response, 403, None, RejectionBodyOutcome::TimedOut);
    assert_eq!(
        Instant::now() - started,
        Duration::from_secs(20) + BODY_TIMEOUT
    );
    let metadata = response.extensions().get::<RejectionMetadata>().unwrap();
    assert_eq!(metadata.capture_elapsed, BODY_TIMEOUT);
    assert_eq!(
        metadata.retry_after.unwrap().deadline(),
        started + Duration::from_secs(30)
    );
}

#[tokio::test]
async fn body_read_failure_does_not_erase_a_received_http_status() {
    let response = rejected(ScriptedIo::new(
        vec![
            head(403, &format!("Content-Length: {}\r\n", BODY.len() + 1)),
            BODY.to_vec(),
        ],
        End::Error,
    ))
    .await;
    evidence(&response, 403, None, RejectionBodyOutcome::ReadError);
}

#[tokio::test]
async fn body_policy_refusal_keeps_its_original_typed_io_error() {
    let error = match client_async_tls_with_config(
        request(),
        ScriptedIo::new(
            vec![
                head(403, &format!("Content-Length: {}\r\n", BODY.len() + 1)),
                BODY.to_vec(),
            ],
            End::Policy,
        ),
        None,
        None,
    )
    .await
    {
        Err(error) => error,
        Ok(_) => panic!("policy denial must prevent the upgrade"),
    };
    let WebSocketError::Io(error) = error else {
        panic!("policy denial must keep its original error kind");
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(
        error
            .get_ref()
            .and_then(|source| source.downcast_ref::<codex_http_client::NetworkPolicyDenied>()),
        Some(&codex_http_client::NetworkPolicyDenied::Revoked)
    );
}

#[tokio::test]
async fn successful_upgrade_keeps_the_original_verifier_and_buffered_frames() {
    let mut request = request();
    request.headers_mut().insert(
        "sec-websocket-key",
        "dGhlIHNhbXBsZSBub25jZQ==".parse().unwrap(),
    );
    let mut wire = b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n".to_vec();
    wire.extend_from_slice(&[0x81, 2, b'o', b'k']);
    let (mut websocket, response) = client_async_tls_with_config(
        request,
        ScriptedIo::new(vec![wire, vec![0x81, 3, b'e', b'n', b'd']], End::Pending),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert!(response.extensions().get::<RejectionMetadata>().is_none());
    assert_eq!(next_message(&mut websocket).await, Message::text("ok"));
    assert_eq!(next_message(&mut websocket).await, Message::text("end"));
}

#[tokio::test]
async fn header_budget_does_not_count_same_read_successful_frame_bytes() {
    let mut request = request();
    request.headers_mut().insert(
        "sec-websocket-key",
        "dGhlIHNhbXBsZSBub25jZQ==".parse().unwrap(),
    );
    let headers = near_limit_headers(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n".to_vec());
    let payload = "x".repeat(32);
    let frame = [&[0x81, 32], payload.as_bytes()].concat();
    let (mut websocket, response) = client_async_tls_with_config(
        request,
        ScriptedIo::new(header_and_tail_chunks(headers, &frame), End::Pending),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert!(response.extensions().get::<RejectionMetadata>().is_none());
    assert_eq!(next_message(&mut websocket).await, Message::text(payload));
}

#[tokio::test]
async fn successful_status_still_requires_the_original_accept_verification() {
    let wire = b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: invalid\r\n\r\n".to_vec();
    let error = match client_async_tls_with_config(
        request(),
        ScriptedIo::new(vec![wire], End::Eof),
        None,
        None,
    )
    .await
    {
        Err(error) => error,
        Ok(_) => panic!("original verifier must reject an invalid WebSocket accept value"),
    };
    assert!(matches!(error, WebSocketError::Protocol(_)));
}

#[tokio::test]
async fn success_adapter_replays_original_header_and_tail_bytes_exactly() {
    let wire = b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\nframe-tail".to_vec();
    let capture = Arc::new(Mutex::new(None));
    let mut stream = RejectionStream::new(
        ScriptedIo::new(vec![wire.clone(), b"next".to_vec()], End::Eof),
        capture.clone(),
    );
    let mut replay = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut replay))
        .await
        .expect("bounded replay")
        .unwrap();
    assert_eq!(replay, [wire, b"next".to_vec()].concat());
    assert!(capture.lock().unwrap().is_none());
}
