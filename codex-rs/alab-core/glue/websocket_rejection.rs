// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Complete bounded HTTP rejection bodies before Tungstenite drops its stream.
//! Successful upgrades still use Tungstenite's original verifier and frame state.

use codex_http_client::RetryAfter;
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use std::future::Future;
use std::io;
use std::net::Ipv6Addr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::ReadBuf;
use tokio::net::TcpStream;
use tokio::time::Instant;
use tokio::time::Sleep;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::Connector;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::client_async_with_config;
use tokio_tungstenite::proxy::connect_via_proxy as library_connect_via_proxy;
use tokio_tungstenite::tungstenite::Error as WebSocketError;
use tokio_tungstenite::tungstenite::client::uri_mode;
use tokio_tungstenite::tungstenite::error::TlsError;
use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::handshake::client::Response;
use tokio_tungstenite::tungstenite::handshake::machine::TryParse;
use tokio_tungstenite::tungstenite::http::HeaderMap;
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::proxy::ProxyConfig;
use tokio_tungstenite::tungstenite::proxy::ProxyScheme;
use tokio_tungstenite::tungstenite::stream::Mode;

const MAX_HEADERS: usize = 64 * 1024;
const MAX_HEADER_READS: usize = 512;
const MAX_BODY: usize = 64 * 1024;
const MAX_WIRE_BODY: usize = 128 * 1024;
const MAX_BODY_METADATA: usize = 16 * 1024;
const MAX_LINE: usize = 4096;
const BODY_TIMEOUT: Duration = Duration::from_secs(5);
const READS_PER_POLL: usize = 16;

/// Stable body-capture classifications; none contains provider-controlled text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionBodyOutcome {
    Complete,
    Malformed,
    Truncated,
    TooLarge,
    Unsupported,
    TimedOut,
    ReadError,
}

impl RejectionBodyOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Malformed => "malformed",
            Self::Truncated => "truncated",
            Self::TooLarge => "too_large",
            Self::Unsupported => "unsupported",
            Self::TimedOut => "timed_out",
            Self::ReadError => "read_error",
        }
    }
}

/// Advice belongs to header receipt, even if collecting the entity takes time.
/// Presence with `retry_after=None` must not restart advice by parsing it again.
#[derive(Clone, Copy, Debug)]
pub struct RejectionMetadata {
    pub retry_after: Option<RetryAfter>,
    pub outcome: RejectionBodyOutcome,
    pub capture_elapsed: Duration,
    pub decoded_body_bytes: Option<usize>,
    /// Entity bytes accepted by a created decoder, including incomplete bodies.
    /// Excludes framing and data rejected by the decoder's retention limit.
    pub entity_bytes_accepted: Option<usize>,
}

struct Captured {
    body: Option<Vec<u8>>,
    metadata: RejectionMetadata,
}

type Capture = Arc<Mutex<Option<Captured>>>;

pub(crate) type GuardedStream<S> = RejectionStream<MaybeTlsStream<S>>;

/// The guard is above target TLS and below the existing WebSocket handshake.
/// After a 101 its read/write operations are ordinary transparent delegation.
pub(crate) struct RejectionStream<S> {
    stream: S,
    state: State,
    capture: Capture,
    header_received: Instant,
    timer: Option<Pin<Box<Sleep>>>,
}

enum State {
    Headers {
        bytes: Vec<u8>,
        reads: usize,
    },
    Body {
        headers: Vec<u8>,
        decoder: Decoder,
        retry_after: Option<RetryAfter>,
    },
    Replay {
        bytes: Vec<u8>,
        at: usize,
        upgraded: bool,
    },
    Pass,
    Done,
}

enum Framing {
    Length(usize),
    Chunked(ChunkState),
    Eof,
}

struct Decoder {
    framing: Framing,
    body: Vec<u8>,
    wire_bytes: usize,
    metadata_bytes: usize,
    line: Vec<u8>,
}

#[derive(Clone, Copy)]
enum ChunkState {
    Size,
    Data(usize),
    DataCr,
    DataLf,
    Trailer,
    Complete,
}

impl Decoder {
    fn new(headers: &HeaderMap, status: StatusCode) -> Result<Self, RejectionBodyOutcome> {
        let has_length = headers.contains_key("content-length");
        let has_transfer = headers.contains_key("transfer-encoding");
        if has_length && has_transfer {
            return Err(RejectionBodyOutcome::Malformed);
        }
        for value in headers.get_all("content-encoding") {
            if !value
                .to_str()
                .ok()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("identity"))
            {
                return Err(RejectionBodyOutcome::Unsupported);
            }
        }
        let framing = if status.is_informational()
            || status == StatusCode::NO_CONTENT
            || status == StatusCode::NOT_MODIFIED
        {
            Framing::Length(0)
        } else if has_transfer {
            let mut codings = Vec::new();
            for value in headers.get_all("transfer-encoding") {
                let value = value
                    .to_str()
                    .map_err(|_| RejectionBodyOutcome::Malformed)?;
                codings.extend(value.split(',').map(str::trim));
            }
            if codings.len() != 1 || !codings[0].eq_ignore_ascii_case("chunked") {
                return Err(RejectionBodyOutcome::Unsupported);
            }
            Framing::Chunked(ChunkState::Size)
        } else if has_length {
            let mut length = None;
            for value in headers.get_all("content-length") {
                for value in value
                    .to_str()
                    .map_err(|_| RejectionBodyOutcome::Malformed)?
                    .split(',')
                {
                    let value = value.trim();
                    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                        return Err(RejectionBodyOutcome::Malformed);
                    }
                    let parsed = value
                        .parse::<usize>()
                        .map_err(|_| RejectionBodyOutcome::TooLarge)?;
                    if length.is_some_and(|length| length != parsed) {
                        return Err(RejectionBodyOutcome::Malformed);
                    }
                    length = Some(parsed);
                }
            }
            let length = length.ok_or(RejectionBodyOutcome::Malformed)?;
            if length > MAX_BODY {
                return Err(RejectionBodyOutcome::TooLarge);
            }
            Framing::Length(length)
        } else {
            Framing::Eof
        };
        Ok(Self {
            framing,
            body: Vec::new(),
            wire_bytes: 0,
            metadata_bytes: 0,
            line: Vec::new(),
        })
    }

    fn complete(&self) -> bool {
        matches!(
            self.framing,
            Framing::Length(0) | Framing::Chunked(ChunkState::Complete)
        )
    }

    fn feed(&mut self, mut bytes: &[u8]) -> Result<bool, RejectionBodyOutcome> {
        while !bytes.is_empty() && !self.complete() {
            match &mut self.framing {
                Framing::Length(remaining) => {
                    let take = (*remaining).min(bytes.len());
                    self.body.extend_from_slice(&bytes[..take]);
                    *remaining -= take;
                    bytes = &bytes[take..];
                    self.wire_bytes += take;
                }
                Framing::Eof => {
                    if bytes.len() > MAX_BODY - self.body.len() {
                        return Err(RejectionBodyOutcome::TooLarge);
                    }
                    self.body.extend_from_slice(bytes);
                    self.wire_bytes += bytes.len();
                    bytes = &[];
                }
                Framing::Chunked(state) => match *state {
                    ChunkState::Data(remaining) => {
                        let take = remaining.min(bytes.len());
                        self.body.extend_from_slice(&bytes[..take]);
                        bytes = &bytes[take..];
                        self.wire_bytes += take;
                        *state = if take == remaining {
                            ChunkState::DataCr
                        } else {
                            ChunkState::Data(remaining - take)
                        };
                    }
                    ChunkState::DataCr | ChunkState::DataLf => {
                        let expected = if matches!(*state, ChunkState::DataCr) {
                            b'\r'
                        } else {
                            b'\n'
                        };
                        if bytes[0] != expected {
                            return Err(RejectionBodyOutcome::Malformed);
                        }
                        bytes = &bytes[1..];
                        self.wire_bytes += 1;
                        *state = if expected == b'\r' {
                            ChunkState::DataLf
                        } else {
                            ChunkState::Size
                        };
                    }
                    ChunkState::Size | ChunkState::Trailer => {
                        let byte = bytes[0];
                        bytes = &bytes[1..];
                        self.wire_bytes += 1;
                        self.metadata_bytes += 1;
                        if self.line.len() >= MAX_LINE || self.metadata_bytes > MAX_BODY_METADATA {
                            return Err(RejectionBodyOutcome::TooLarge);
                        }
                        self.line.push(byte);
                        if byte == b'\n' {
                            if !self.line.ends_with(b"\r\n") {
                                return Err(RejectionBodyOutcome::Malformed);
                            }
                            let line = &self.line[..self.line.len() - 2];
                            if matches!(*state, ChunkState::Size) {
                                let size = chunk_size(line)?;
                                if size > MAX_BODY - self.body.len() {
                                    return Err(RejectionBodyOutcome::TooLarge);
                                }
                                *state = if size == 0 {
                                    ChunkState::Trailer
                                } else {
                                    ChunkState::Data(size)
                                };
                            } else if line.is_empty() {
                                *state = ChunkState::Complete;
                            } else if !valid_trailer(line) {
                                return Err(RejectionBodyOutcome::Malformed);
                            }
                            self.line.clear();
                        }
                    }
                    ChunkState::Complete => unreachable!(),
                },
            }
            if self.wire_bytes > MAX_WIRE_BODY {
                return Err(RejectionBodyOutcome::TooLarge);
            }
        }
        Ok(self.complete())
    }

    fn eof(&self) -> RejectionBodyOutcome {
        if matches!(self.framing, Framing::Eof) || self.complete() {
            RejectionBodyOutcome::Complete
        } else {
            RejectionBodyOutcome::Truncated
        }
    }
}

fn token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn skip_ows(bytes: &[u8], at: &mut usize) {
    while bytes
        .get(*at)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        *at += 1;
    }
}

fn chunk_size(line: &[u8]) -> Result<usize, RejectionBodyOutcome> {
    let digits = line
        .iter()
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count();
    if digits == 0 {
        return Err(RejectionBodyOutcome::Malformed);
    }
    let number =
        std::str::from_utf8(&line[..digits]).map_err(|_| RejectionBodyOutcome::Malformed)?;
    let size = usize::from_str_radix(number, 16).map_err(|_| RejectionBodyOutcome::TooLarge)?;
    let mut at = digits;
    while at < line.len() {
        skip_ows(line, &mut at);
        if line.get(at) != Some(&b';') {
            return Err(RejectionBodyOutcome::Malformed);
        }
        at += 1;
        skip_ows(line, &mut at);
        let name = at;
        while line.get(at).is_some_and(|byte| token(*byte)) {
            at += 1;
        }
        if name == at {
            return Err(RejectionBodyOutcome::Malformed);
        }
        skip_ows(line, &mut at);
        if line.get(at) == Some(&b'=') {
            at += 1;
            skip_ows(line, &mut at);
            if line.get(at) == Some(&b'"') {
                at += 1;
                loop {
                    match line.get(at).copied() {
                        Some(b'"') => {
                            at += 1;
                            break;
                        }
                        Some(b'\\') => {
                            at += 1;
                            if !line
                                .get(at)
                                .is_some_and(|byte| *byte == b'\t' || *byte >= b' ' && *byte != 127)
                            {
                                return Err(RejectionBodyOutcome::Malformed);
                            }
                            at += 1;
                        }
                        Some(byte) if byte == b'\t' || byte >= b' ' && byte != 127 => at += 1,
                        _ => return Err(RejectionBodyOutcome::Malformed),
                    }
                }
            } else {
                let value = at;
                while line.get(at).is_some_and(|byte| token(*byte)) {
                    at += 1;
                }
                if value == at {
                    return Err(RejectionBodyOutcome::Malformed);
                }
            }
            skip_ows(line, &mut at);
        }
    }
    Ok(size)
}

fn valid_trailer(line: &[u8]) -> bool {
    let Some(colon) = line.iter().position(|byte| *byte == b':') else {
        return false;
    };
    let name = &line[..colon];
    !name.is_empty()
        && name.iter().all(|byte| token(*byte))
        && !name.eq_ignore_ascii_case(b"content-length")
        && !name.eq_ignore_ascii_case(b"transfer-encoding")
        && line[colon + 1..]
            .iter()
            .all(|byte| *byte == b'\t' || *byte >= b' ' && *byte != 127)
}

impl<S> RejectionStream<S> {
    fn new(stream: S, capture: Capture) -> Self {
        let header_received = Instant::now();
        Self {
            stream,
            state: State::Headers {
                bytes: Vec::new(),
                reads: 0,
            },
            capture,
            header_received,
            timer: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn get_ref(&self) -> &S {
        &self.stream
    }

    fn finish_rejection(
        &mut self,
        headers: Vec<u8>,
        decoder: Option<Decoder>,
        retry_after: Option<RetryAfter>,
        outcome: RejectionBodyOutcome,
    ) {
        let entity_bytes_accepted = decoder.as_ref().map(|decoder| decoder.body.len());
        let body = decoder
            .filter(|_| outcome == RejectionBodyOutcome::Complete)
            .map(|decoder| decoder.body);
        let decoded_body_bytes = body.as_ref().map(Vec::len);
        *self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Captured {
            body,
            metadata: RejectionMetadata {
                retry_after,
                outcome,
                capture_elapsed: self.header_received.elapsed(),
                decoded_body_bytes,
                entity_bytes_accepted,
            },
        });
        self.timer = None;
        self.state = State::Replay {
            bytes: headers,
            at: 0,
            upgraded: false,
        };
    }

    fn timed_out(&mut self, cx: &mut Context<'_>) -> bool {
        self.timer
            .as_mut()
            .is_some_and(|timer| timer.as_mut().poll(cx).is_ready())
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for RejectionStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        for _ in 0..READS_PER_POLL {
            if matches!(this.state, State::Pass) {
                return Pin::new(&mut this.stream).poll_read(cx, output);
            }
            if let State::Replay {
                bytes,
                at,
                upgraded,
            } = &mut this.state
            {
                let take = (bytes.len() - *at).min(output.remaining());
                output.put_slice(&bytes[*at..*at + take]);
                *at += take;
                if *at == bytes.len() {
                    this.state = if *upgraded { State::Pass } else { State::Done };
                }
                return Poll::Ready(Ok(()));
            }
            if matches!(this.state, State::Done) {
                return Poll::Ready(Ok(()));
            }
            if this.timed_out(cx) {
                let State::Body {
                    headers,
                    decoder,
                    retry_after,
                } = std::mem::replace(&mut this.state, State::Done)
                else {
                    unreachable!("only a rejected entity has an adapter deadline");
                };
                this.finish_rejection(
                    headers,
                    Some(decoder),
                    retry_after,
                    RejectionBodyOutcome::TimedOut,
                );
                continue;
            }
            let mut buffer = [0_u8; 4096];
            let mut read = ReadBuf::new(&mut buffer);
            let read_result = match Pin::new(&mut this.stream).poll_read(cx, &mut read) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => result,
            };
            let bytes = read.filled();
            let state = std::mem::replace(&mut this.state, State::Done);
            match state {
                State::Headers {
                    bytes: mut accumulated,
                    mut reads,
                } => {
                    read_result?;
                    if bytes.is_empty() {
                        this.timer = None;
                        this.state = State::Replay {
                            bytes: accumulated,
                            at: 0,
                            upgraded: false,
                        };
                        continue;
                    }
                    reads += 1;
                    accumulated.extend_from_slice(bytes);
                    let parsed = Response::try_parse(&accumulated);
                    let header_bytes = match &parsed {
                        Ok(Some((length, _))) => *length,
                        _ => accumulated.len(),
                    };
                    if header_bytes > MAX_HEADERS
                        || reads > MAX_HEADER_READS
                        || reads > 64 && reads * 128 > accumulated.len()
                    {
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "websocket upgrade header budget exceeded",
                        )));
                    }
                    match parsed {
                        Ok(Some((length, response))) => {
                            if response.status() == StatusCode::SWITCHING_PROTOCOLS {
                                this.timer = None;
                                this.state = State::Replay {
                                    bytes: accumulated,
                                    at: 0,
                                    upgraded: true,
                                };
                                continue;
                            }
                            this.header_received = Instant::now();
                            let retry_after = RetryAfter::from_headers(response.headers());
                            let headers = accumulated[..length].to_vec();
                            let mut decoder =
                                match Decoder::new(response.headers(), response.status()) {
                                    Ok(decoder) => decoder,
                                    Err(outcome) => {
                                        this.finish_rejection(headers, None, retry_after, outcome);
                                        continue;
                                    }
                                };
                            match decoder.feed(&accumulated[length..]) {
                                Ok(true) => this.finish_rejection(
                                    headers,
                                    Some(decoder),
                                    retry_after,
                                    RejectionBodyOutcome::Complete,
                                ),
                                Ok(false) => {
                                    this.timer = Some(Box::pin(tokio::time::sleep_until(
                                        this.header_received + BODY_TIMEOUT,
                                    )));
                                    this.state = State::Body {
                                        headers,
                                        decoder,
                                        retry_after,
                                    };
                                }
                                Err(outcome) => this.finish_rejection(
                                    headers,
                                    Some(decoder),
                                    retry_after,
                                    outcome,
                                ),
                            }
                        }
                        Ok(None) => {
                            this.state = State::Headers {
                                bytes: accumulated,
                                reads,
                            }
                        }
                        Err(_) => {
                            // Replay invalid headers unchanged so the original verifier owns
                            // their protocol classification; never log or reinterpret them.
                            this.timer = None;
                            this.state = State::Replay {
                                bytes: accumulated,
                                at: 0,
                                upgraded: false,
                            };
                        }
                    }
                }
                State::Body {
                    headers,
                    mut decoder,
                    retry_after,
                } => {
                    if let Err(error) = read_result {
                        // A custody refusal must never be replaced by a saved HTTP result.
                        if error.get_ref().is_some_and(|source| {
                            source.is::<codex_http_client::NetworkPolicyDenied>()
                        }) {
                            return Poll::Ready(Err(error));
                        }
                        this.finish_rejection(
                            headers,
                            Some(decoder),
                            retry_after,
                            RejectionBodyOutcome::ReadError,
                        );
                    } else if bytes.is_empty() {
                        let outcome = decoder.eof();
                        this.finish_rejection(headers, Some(decoder), retry_after, outcome);
                    } else {
                        match decoder.feed(bytes) {
                            Ok(true) => this.finish_rejection(
                                headers,
                                Some(decoder),
                                retry_after,
                                RejectionBodyOutcome::Complete,
                            ),
                            Ok(false) => {
                                this.state = State::Body {
                                    headers,
                                    decoder,
                                    retry_after,
                                }
                            }
                            Err(outcome) => {
                                this.finish_rejection(headers, Some(decoder), retry_after, outcome)
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for RejectionStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

fn domain(request: &Request) -> Result<String, WebSocketError> {
    let host = request
        .uri()
        .host()
        .ok_or(WebSocketError::Url(UrlError::NoHostName))?;
    Ok(host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .to_owned())
}

pub(super) async fn client_async_tls_with_config<S>(
    request: Request,
    stream: S,
    config: Option<WebSocketConfig>,
    connector: Option<Connector>,
) -> Result<(WebSocketStream<GuardedStream<S>>, Response), WebSocketError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let mode = uri_mode(request.uri())?;
    let stream = match mode {
        Mode::Plain => MaybeTlsStream::Plain(stream),
        Mode::Tls => {
            let config: Arc<ClientConfig> = match connector {
                Some(Connector::Rustls(config)) => config,
                Some(Connector::Plain) => {
                    return Err(WebSocketError::Url(UrlError::TlsFeatureNotEnabled));
                }
                Some(_) => return Err(WebSocketError::Url(UrlError::TlsFeatureNotEnabled)),
                None => codex_http_client::build_native_only_websocket_tls_config()
                    .map_err(WebSocketError::Io)?,
            };
            let name = ServerName::try_from(domain(&request)?)
                .map_err(|_| WebSocketError::Tls(TlsError::InvalidDnsName))?;
            let stream = TlsConnector::from(config)
                .connect(name, stream)
                .await
                .map_err(WebSocketError::Io)?;
            MaybeTlsStream::Rustls(stream)
        }
    };
    let capture = Arc::new(Mutex::new(None));
    let guarded = RejectionStream::new(stream, Arc::clone(&capture));
    client_async_with_config(request, guarded, config)
        .await
        .map_err(|error| match error {
            WebSocketError::Http(mut response) => {
                if let Some(captured) = capture
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    *response.body_mut() = captured.body;
                    response.extensions_mut().insert(captured.metadata);
                }
                WebSocketError::Http(response)
            }
            other => other,
        })
}

/// URI brackets belong to the proxy's TCP authority, not its TLS IP identity.
pub(super) fn proxy_server_name(
    host: String,
) -> Result<ServerName<'static>, rustls::pki_types::InvalidDnsNameError> {
    let bare_ipv6 = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .filter(|host| host.parse::<Ipv6Addr>().is_ok());
    ServerName::try_from(bare_ipv6.map(str::to_owned).unwrap_or(host))
}

/// Keep URI/TLS host spelling separate from each proxy protocol's address form.
pub(super) async fn connect_via_proxy<S>(
    stream: S,
    proxy: &ProxyConfig,
    host: &str,
    port: u16,
) -> Result<S, WebSocketError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let bracketed;
    let host = match proxy.scheme {
        ProxyScheme::Http if host.parse::<Ipv6Addr>().is_ok() => {
            bracketed = format!("[{host}]");
            bracketed.as_str()
        }
        ProxyScheme::Socks5 | ProxyScheme::Socks5h => host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .filter(|host| host.parse::<Ipv6Addr>().is_ok())
            .unwrap_or(host),
        _ => host,
    };
    library_connect_via_proxy(stream, proxy, host, port).await
}

pub(super) async fn connect_async_tls_with_config(
    request: Request,
    config: Option<WebSocketConfig>,
    disable_nagle: bool,
    connector: Option<Connector>,
) -> Result<(WebSocketStream<GuardedStream<TcpStream>>, Response), WebSocketError> {
    let host = domain(&request)?;
    let port = super::websocket_port(&request)?;
    let nodelay = if disable_nagle {
        crate::TcpNodelay::Enabled
    } else {
        crate::TcpNodelay::Default
    };
    // The library's parse error can contain the complete URL or a password.
    // Match explicit-route redaction before API errors and telemetry stringify it.
    let proxy = ProxyConfig::from_env(request.uri()).map_err(|error| match error {
        WebSocketError::Url(UrlError::InvalidProxyConfig(_)) => super::invalid_proxy_config(),
        other => other,
    })?;
    let socket = match proxy {
        Some(proxy) => {
            let socket = super::connect_tcp(proxy.authority(), nodelay)
                .await
                .map_err(WebSocketError::Io)?;
            connect_via_proxy(socket, &proxy, &host, port).await?
        }
        None => super::connect_tcp(super::host_port(&host, port), nodelay)
            .await
            .map_err(WebSocketError::Io)?,
    };
    client_async_tls_with_config(request, socket, config, connector).await
}

#[cfg(test)]
#[path = "test_child_process.rs"]
mod test_child_process;

#[cfg(test)]
#[path = "websocket_rejection_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "websocket_rejection_tls_tests.rs"]
mod tls_tests;

#[cfg(test)]
#[path = "websocket_proxy_ipv6_tests.rs"]
mod proxy_ipv6_tests;

#[cfg(test)]
#[path = "websocket_ipv6_proxy_tls_tests.rs"]
mod proxy_tls_ipv6_tests;

#[cfg(test)]
#[path = "websocket_proxy_config_error_tests.rs"]
mod proxy_config_error_tests;
