// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::daemon_login::DaemonLoginChanged;
use crate::daemon_login::ManagedLogin;
use crate::legacy_core::config::ConfigBuilder;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::JSONRPCMessage;
use codex_utils_absolute_path::AbsolutePathBuf;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn replacement_daemon_login_is_checked_before_model_bootstrap_or_thread_resume() -> Result<()>
{
    let home = tempfile::TempDir::new()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let path = home.path().join("daemon.sock");
    let listener = tokio::net::UnixListener::bind(&path)?;
    let target = AppServerTarget::LocalDaemon {
        endpoint: RemoteAppServerEndpoint::UnixSocket {
            socket_path: AbsolutePathBuf::from_absolute_path_checked(path)?,
        },
        allow_embedded_fallback: false,
    };
    let server = tokio::spawn(async move {
        for workspace in ["B", "A"] {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut read_count = 0;
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let JSONRPCMessage::Request(request) = serde_json::from_str(&text).unwrap() else {
                    continue;
                };
                let result = match request.method.as_str() {
                    "initialize" => json!({"userAgent": "daemon-test"}),
                    "accountSession/list" => {
                        assert_ne!(
                            request
                                .params
                                .as_ref()
                                .and_then(|p| p.get("refreshWorkspaceMetadata")),
                            Some(&json!(true))
                        );
                        read_count += 1;
                        json!({"activeSessionId": "same-session", "sessions": [{
                            "sessionId": "same-session", "email": "same@example.test", "userId": "same-user",
                            "isActive": true, "selectedWorkspaceAccountId": workspace,
                            "lastUsedAt": 0, "workspaces": []
                        }]})
                    }
                    other => panic!("request {other} escaped the ownership check"),
                };
                socket
                    .send(Message::Text(
                        json!({"id": request.id, "result": result})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
            }
            assert_eq!(read_count, 1);
        }
    });
    let owner =
        Arc::new(ManagedLogin::new("same-session".into(), "same-user".into(), "B".into()).unwrap());
    let client = crate::app_server_connection::connect(&target).await?;
    crate::daemon_login::check_replacement(Some(&owner), &target, client)
        .await?
        .shutdown()
        .await?;
    let local_settings = crate::local_settings::LocalSettings::from(&config);
    let replacement = tokio::time::timeout(
        Duration::from_secs(15),
        reconnect(
            crate::daemon_login::ReconnectOwner {
                backend: crate::backend_id::BackendId::new(),
                login: Some(owner),
            },
            target,
            config,
            local_settings,
            Some(ThreadId::new()),
            None,
            ThreadToolTransport::Dynamic,
            ReconnectPresentation::Conversation,
        ),
    )
    .await?;
    assert!(replacement.is_err_and(|error| error.is::<DaemonLoginChanged>()));
    tokio::time::timeout(Duration::from_secs(5), server).await??;
    Ok(())
}
