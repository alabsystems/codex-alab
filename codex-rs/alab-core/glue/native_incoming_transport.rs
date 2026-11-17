// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! WebSocket framing for already-owned native streams. Admission belongs to the
//! provider; this module opens no listener and reconstructs no SDK authority.

use super::TransportEvent;
use crate::websocket_compression::WEBSOCKET_COMPRESSION_HEADER;
use crate::websocket_compression::WebSocketCompression;
use crate::websocket_compression::ZSTD_STREAM_PROTOCOL;
use codex_alab_core::native_incoming::Incoming;
use futures::StreamExt;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tokio::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::accept_hdr_async_with_config;
use tokio_tungstenite::tungstenite::handshake::server::Request;
use tokio_tungstenite::tungstenite::handshake::server::Response;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_util::sync::CancellationToken;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

// Closing a JoinSet task must also close the actual socket used by the existing
// WebSocket loop's child tasks. A duplicate is custody only, never a new route.
struct SocketFence(std::os::unix::net::UnixStream);
impl SocketFence {
    fn new(stream: &tokio::net::UnixStream) -> io::Result<Self> {
        let _receiving = codex_alab_core::native_fd_custody::receiving()?;
        let descriptor = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = unsafe { OwnedFd::from_raw_fd(descriptor) };
        Ok(Self(owned.into()))
    }

    fn close(&self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}
impl Drop for SocketFence {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn start_native_incoming_acceptor(
    incoming: Incoming,
    transport_event_tx: mpsc::Sender<TransportEvent>,
    shutdown: CancellationToken,
    compression: WebSocketCompression,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut connections = JoinSet::new();
        let stop_connections = CancellationToken::new();
        loop {
            tokio::select! {
                biased;
                () = shutdown.cancelled() => break,
                () = incoming.withdrawn() => break,
                completed = connections.join_next(), if !connections.is_empty() => {
                    if let Some(Err(error)) = completed {
                        tracing::warn!("native stream connection task failed: {error}");
                    }
                }
                accepted = incoming.accept_envelope() => {
                    let accepted = match accepted {
                        Ok(Some(stream)) => stream,
                        Ok(None) => break,
                        Err(error) => {
                            tracing::warn!("native stream provider ended: {error}");
                            break;
                        }
                    };
                    let (stream, resource) = accepted.into_parts();
                    let fence = match SocketFence::new(&stream) {
                        Ok(fence) => fence,
                        Err(error) => {
                            tracing::warn!("native stream custody failed: {error}");
                            incoming.fail(error);
                            break;
                        }
                    };
                    let events = transport_event_tx.clone();
                    let stop = stop_connections.clone();
                    connections.spawn(async move {
                        let mut negotiated = WebSocketCompression::Plain;
                        let config = WebSocketConfig::default();
                        let max_bytes = [config.max_frame_size, config.max_message_size]
                            .into_iter().flatten().min();
                        let handshake = accept_hdr_async_with_config(
                            stream,
                            |request: &Request, mut response: Response| {
                                if compression == WebSocketCompression::Zstd
                                    && request.headers().get(WEBSOCKET_COMPRESSION_HEADER)
                                        .is_some_and(|value| value == ZSTD_STREAM_PROTOCOL)
                                {
                                    negotiated = WebSocketCompression::Zstd;
                                    response.headers_mut().insert(
                                        WEBSOCKET_COMPRESSION_HEADER,
                                        HeaderValue::from_static(ZSTD_STREAM_PROTOCOL),
                                    );
                                }
                                if let Some(max_bytes) = max_bytes {
                                    response.headers_mut().insert(
                                        "x-codex-websocket-max-unfragmented-message-bytes",
                                        HeaderValue::from(max_bytes),
                                    );
                                }
                                Ok(response)
                            },
                            Some(config),
                        );
                        let upgraded = tokio::select! {
                            biased;
                            () = stop.cancelled() => return,
                            result = timeout(HANDSHAKE_TIMEOUT, handshake) => result,
                        };
                        let websocket = match upgraded {
                            Ok(Ok(websocket)) => websocket,
                            Ok(Err(error)) => {
                                tracing::warn!("native stream websocket handshake failed: {error}");
                                return;
                            }
                            Err(_) => {
                                tracing::warn!("native stream websocket handshake timed out");
                                return;
                            }
                        };
                        let (writer, reader) = websocket.split();
                        let connection = super::websocket::run_websocket_connection(
                            writer, reader, events, negotiated, resource,
                        );
                        tokio::pin!(connection);
                        tokio::select! {
                            () = &mut connection => {}
                            () = stop.cancelled() => {
                                // Let the existing loop stop its children
                                // and publish Closed before releasing this task.
                                fence.close();
                                connection.await;
                            }
                        }
                    });
                }
            }
        }
        stop_connections.cancel();
        while connections.join_next().await.is_some() {}
    })
}

#[cfg(test)]
#[path = "native_incoming_transport_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "native_incoming_resource_tests.rs"]
mod resource_tests;
