// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::CONNECT_TIMEOUT;
use super::ErrorKind;
use super::IoError;
use super::IoResult;
use super::RemoteAppServerClient;
use super::RemoteAppServerConnectArgs;
use super::UDS_WEBSOCKET_HANDSHAKE_URL;
use super::client_async_with_config;
use super::remote_websocket_config;
use super::timeout;
use super::websocket_request;
use codex_app_server_protocol::InitializeParams;

#[path = "owned_local_client.rs"]
mod owned_local;

impl RemoteAppServerClient {
    /// Use an actual supplied stream with the existing remote-client metadata
    /// policy. Endpoint data is not resolved or connected. This convenience
    /// entry intentionally keeps RemoteArgs' current InitializeParams mapping.
    pub async fn connect_native_stream(
        stream: tokio::net::UnixStream,
        args: RemoteAppServerConnectArgs,
    ) -> IoResult<Self> {
        let initialize = args.initialize_params();
        Self::connect_native_stream_with_initialize(stream, initialize, args.channel_capacity).await
    }

    /// Preserve the caller's entire InitializeParams in the existing WebSocket
    /// initialize/request worker. No capability, title, extension, attestation
    /// or gateway value is reconstructed here. These are client declarations,
    /// not native SDK admission or a grant to use credentials or the foreground.
    ///
    /// Queue capacity uses the original embedded startup's max(1) rule. It is
    /// independent of server capabilities, native ownership and initialization
    /// success; a socket without the proper reply cannot produce a client.
    pub async fn connect_native_stream_with_initialize(
        stream: tokio::net::UnixStream,
        initialize: InitializeParams,
        channel_capacity: usize,
    ) -> IoResult<Self> {
        let request = websocket_request(UDS_WEBSOCKET_HANDSHAKE_URL).map_err(|error| {
            IoError::new(
                ErrorKind::InvalidInput,
                format!("native websocket request: {error}"),
            )
        })?;
        let handshake = timeout(
            CONNECT_TIMEOUT,
            client_async_with_config(request, stream, Some(remote_websocket_config())),
        )
        .await
        .map_err(|_| IoError::new(ErrorKind::TimedOut, "native websocket handshake timed out"))?
        .map_err(|error| IoError::other(format!("native websocket handshake: {error}")))?;
        Self::connect_with_stream(
            channel_capacity.max(1),
            "native://retained-stream".to_owned(),
            handshake,
            initialize,
        )
        .await
    }
}

#[cfg(test)]
#[path = "native_incoming_client_tests.rs"]
mod tests;
