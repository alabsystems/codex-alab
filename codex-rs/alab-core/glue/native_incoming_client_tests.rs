// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Unix/WebSocket wire controls only. No Config responder, native SDK
//! permission, Source admission or installed application is simulated here.
use super::*;
use crate::RemoteAppServerEndpoint;
use codex_app_server_protocol::{ClientInfo, InitializeCapabilities};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::net::UnixStream;
use tokio_tungstenite::tungstenite::Message;

fn arguments() -> RemoteAppServerConnectArgs {
    RemoteAppServerConnectArgs {
        required_human_approval: true,
        endpoint: RemoteAppServerEndpoint::UnixSocket {
            socket_path: "/native-incoming-data-only-do-not-open".try_into().unwrap(),
        },
        client_name: "native-incoming-fixture".into(),
        client_version: "1.2.3".into(),
        experimental_api: true,
        mcp_server_openai_form_elicitation: true,
        opt_out_notification_methods: vec!["test/opt-out".into()],
        channel_capacity: 8,
    }
}

fn complete_initialize() -> InitializeParams {
    InitializeParams {
        client_info: ClientInfo {
            name: "native-exact-λ".into(),
            title: Some("Original frontend title".into()),
            version: "1.2.3-exact".into(),
        },
        capabilities: Some(InitializeCapabilities {
            required_human_approval: true,
            explicit_gateway_oauth: true,
            experimental_api: true,
            request_attestation: true,
            mcp_server_openai_form_elicitation: true,
            opt_out_notification_methods: Some(vec![
                "thread/started".into(),
                "item/agentMessage/delta".into(),
            ]),
            extensions: Some(HashMap::from([
                (
                    "io.modelcontextprotocol/ui".into(),
                    json!({"mimeTypes": ["text/html;profile=mcp-app"], "extra": {"enabled": true}}),
                ),
                ("openai/form".into(), json!({})),
            ])),
        }),
    }
}

fn complete_wire_params() -> Value {
    // Explicit expected wire fields catch the old lossy reconstruction of
    // title/attestation/gateway/extensions, rather than mirroring that mapper.
    json!({
        "clientInfo": {"name": "native-exact-λ", "title": "Original frontend title", "version": "1.2.3-exact"},
        "capabilities": {
            "requiredHumanApproval": true,
            "explicitGatewayOauth": true,
            "experimentalApi": true,
            "requestAttestation": true,
            "mcpServerOpenaiFormElicitation": true,
            "optOutNotificationMethods": ["thread/started", "item/agentMessage/delta"],
            "extensions": {
                "io.modelcontextprotocol/ui": {"mimeTypes": ["text/html;profile=mcp-app"], "extra": {"enabled": true}},
                "openai/form": {}
            }
        }
    })
}

async fn receive_initialize(socket: &mut tokio_tungstenite::WebSocketStream<UnixStream>) -> Value {
    let message = socket.next().await.unwrap().unwrap();
    let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
    assert_eq!(request["method"], "initialize");
    assert_eq!(request["id"], "initialize");
    request
}

#[tokio::test]
async fn native_incoming_client_preserves_every_nondefault_initialize_field_on_real_wire() {
    let (client, server) = UnixStream::pair().unwrap();
    let peer = tokio::spawn(async move {
        let mut socket = tokio_tungstenite::accept_async(server).await.unwrap();
        assert_eq!(
            receive_initialize(&mut socket).await["params"],
            complete_wire_params()
        );
        // Physical connection plus correct bytes is not a successful initialize
        // response, much less native admission. Keep that negative control.
        socket.close(None).await.unwrap();
    });
    assert!(
        RemoteAppServerClient::connect_native_stream_with_initialize(
            client,
            complete_initialize(),
            8
        )
        .await
        .is_err()
    );
    peer.await.unwrap();
}

#[tokio::test]
async fn native_incoming_client_keeps_absent_capabilities_absent() {
    let (client, server) = UnixStream::pair().unwrap();
    let mut initialize = complete_initialize();
    initialize.capabilities = None;
    let peer = tokio::spawn(async move {
        let mut socket = tokio_tungstenite::accept_async(server).await.unwrap();
        let request = receive_initialize(&mut socket).await;
        assert!(request["params"].get("capabilities").is_none());
        assert_eq!(
            request["params"]["clientInfo"]["title"],
            "Original frontend title"
        );
        socket.close(None).await.unwrap();
    });
    assert!(
        RemoteAppServerClient::connect_native_stream_with_initialize(client, initialize, 8)
            .await
            .is_err()
    );
    peer.await.unwrap();
}

#[tokio::test]
async fn native_incoming_client_keeps_explicit_empty_extension_and_notification_options() {
    let (client, server) = UnixStream::pair().unwrap();
    let mut initialize = complete_initialize();
    let capabilities = initialize.capabilities.as_mut().unwrap();
    capabilities.extensions = Some(HashMap::new());
    capabilities.opt_out_notification_methods = Some(Vec::new());
    let peer = tokio::spawn(async move {
        let mut socket = tokio_tungstenite::accept_async(server).await.unwrap();
        let request = receive_initialize(&mut socket).await;
        assert_eq!(request["params"]["capabilities"]["extensions"], json!({}));
        assert_eq!(
            request["params"]["capabilities"]["optOutNotificationMethods"],
            json!([])
        );
        socket.close(None).await.unwrap();
    });
    assert!(
        RemoteAppServerClient::connect_native_stream_with_initialize(client, initialize, 8)
            .await
            .is_err()
    );
    peer.await.unwrap();
}

#[tokio::test]
async fn native_incoming_client_remote_args_wrapper_keeps_its_existing_initialize_mapping() {
    let (client, server) = UnixStream::pair().unwrap();
    let expected = serde_json::to_value(arguments().initialize_params()).unwrap();
    let peer = tokio::spawn(async move {
        let mut socket = tokio_tungstenite::accept_async(server).await.unwrap();
        assert_eq!(receive_initialize(&mut socket).await["params"], expected);
        socket.close(None).await.unwrap();
    });
    assert!(
        RemoteAppServerClient::connect_native_stream(client, arguments())
            .await
            .is_err()
    );
    peer.await.unwrap();
}

#[tokio::test]
async fn native_incoming_client_clamps_zero_and_preserves_positive_actual_queue_capacities() {
    for (requested, expected) in [(0_usize, 1_usize), (1, 1), (7, 7)] {
        let (client, server) = UnixStream::pair().unwrap();
        let (finish, finished) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(server).await.unwrap();
            let request = receive_initialize(&mut socket).await;
            assert_eq!(request["params"], complete_wire_params());
            // Ordinary transport handshake control only. No Config/Source/SDK
            // result is constructed, requested or claimed by this peer.
            socket
                .send(Message::Text(
                    json!({"id": request["id"], "result": {
                        "userAgent": "transport-capacity-control", "codexHome": "/fixture-only",
                        "platformFamily": "unix", "platformOs": "fixture"
                    }})
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            let initialized = socket.next().await.unwrap().unwrap();
            let notification: Value = serde_json::from_str(initialized.to_text().unwrap()).unwrap();
            assert_eq!(notification["method"], "initialized");
            let _ = finished.await;
        });
        let client = RemoteAppServerClient::connect_native_stream_with_initialize(
            client,
            complete_initialize(),
            requested,
        )
        .await
        .unwrap();
        assert_eq!(client.command_tx.max_capacity(), expected);
        assert_eq!(client.command_tx.capacity(), expected);
        let _ = finish.send(());
        peer.await.unwrap();
        drop(client);
    }
}

#[tokio::test]
async fn native_incoming_client_rejects_raw_socket_without_websocket_handshake() {
    use tokio::io::AsyncWriteExt;
    let (client, mut server) = UnixStream::pair().unwrap();
    let peer = tokio::spawn(async move {
        server
            .write_all(b"CONFIG is not a websocket or native grant\n")
            .await
            .unwrap();
        server.shutdown().await.unwrap();
    });
    assert!(
        RemoteAppServerClient::connect_native_stream_with_initialize(
            client,
            complete_initialize(),
            8
        )
        .await
        .is_err()
    );
    peer.await.unwrap();
}
