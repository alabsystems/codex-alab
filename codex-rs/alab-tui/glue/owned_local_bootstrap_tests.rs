// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual local-runtime controls for the full-input factory. Test-owned IO
//! supplies no Native SDK admission and no custom app-server responder.

use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::ensure;
use futures::FutureExt;
use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct Provider {
    queue: Mutex<mpsc::Receiver<UnixStream>>,
    withdrawn: CancellationToken,
}

impl IncomingStreamProvider for Provider {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::poll_fn(|context| {
            self.queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .poll_recv(context)
                .map(|next| {
                    Ok(next.map(codex_alab_core::native_incoming::AcceptedConnection::stream_only))
                })
        }))
    }

    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(self.withdrawn.cancelled())
    }
}

async fn bounded<F: Future>(future: F) -> color_eyre::Result<F::Output> {
    tokio::time::timeout(Duration::from_secs(30), future)
        .await
        .wrap_err("prepared local factory control timed out")
}

async fn prepared(home: &TempDir) -> color_eyre::Result<PreparedLocalInputs> {
    let cli_kv_overrides = vec![
        (
            "features.shell_snapshot".into(),
            toml::Value::Boolean(false),
        ),
        ("features.plugins".into(), toml::Value::Boolean(false)),
        (
            "model".into(),
            toml::Value::String("prepared-original-model".into()),
        ),
        (
            "model_provider".into(),
            toml::Value::String("prepared_fixture".into()),
        ),
        (
            "model_providers.prepared_fixture".into(),
            toml::Value::Table(toml::toml! {
                name = "Prepared local fixture"
                base_url = "http://127.0.0.1:0/v1"
                supports_websockets = false
                requires_openai_auth = false
            }),
        ),
    ];
    let loader_overrides = LoaderOverrides::without_managed_config_for_tests();
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .cli_overrides(cli_kv_overrides.clone())
        .loader_overrides(loader_overrides.clone())
        .build()
        .await?;
    Ok(PreparedLocalInputs {
        arg0_paths: Arg0DispatchPaths::default(),
        config,
        cli_kv_overrides,
        loader_overrides,
        strict_config: false,
        cloud_config_bundle: CloudConfigBundleLoader::default(),
        feedback: CodexFeedback::new(),
        log_db: None,
        state_db: None,
        environment_manager: Arc::new(EnvironmentManager::default_for_tests()),
        embedded_network_policy: EmbeddedNetworkPolicy::default(),
    })
}

async fn connect(
    queue: &mpsc::Sender<UnixStream>,
    owner: &RetainedRuntimeOwner,
) -> color_eyre::Result<RemoteAppServerClient> {
    let (client, server) = UnixStream::pair()?;
    bounded(queue.send(server)).await??;
    Ok(bounded(
        RemoteAppServerClient::connect_native_stream_with_initialize(
            client,
            owner.frontend_metadata().initialize.clone(),
            owner.frontend_metadata().channel_capacity,
        ),
    )
    .await??)
}

async fn config(client: &RemoteAppServerClient) -> color_eyre::Result<ConfigReadResponse> {
    Ok(bounded(client.request_typed(ClientRequest::ConfigRead {
        request_id: RequestId::Integer(1),
        params: ConfigReadParams {
            include_layers: false,
            cwd: None,
        },
    }))
    .await??)
}

#[tokio::test]
async fn prepared_local_factory_preserves_cli_inputs_and_one_runtime_across_eof()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let inputs = prepared(&home).await?;
    let (queue, receive) = mpsc::channel(2);
    let incoming = Incoming::new(Provider {
        queue: Mutex::new(receive),
        withdrawn: CancellationToken::new(),
    });
    let owner = bounded(inputs.start_retained(incoming.clone())).await??;
    let mut clients = Vec::new();
    let result: color_eyre::Result<()> = async {
        clients.push(connect(&queue, &owner).await?);
        let first = &clients[0];
        let initial = config(first).await?;
        ensure!(initial.config.model.as_deref() == Some("prepared-original-model"));
        ensure!(initial.config.model_provider.as_deref() == Some("prepared_fixture"));
        ensure!(owner.frontend_metadata().initialize.client_info.name == "codex-tui");
        let started: ThreadStartResponse =
            bounded(first.request_typed(ClientRequest::ThreadStart {
                request_id: RequestId::Integer(2),
                params: ThreadStartParams {
                    ephemeral: Some(true),
                    ..Default::default()
                },
            }))
            .await??;
        ensure!(started.thread.ephemeral && started.thread.path.is_none());
        let first = clients.remove(0);
        bounded(first.shutdown()).await??;
        ensure!(owner.wait().now_or_never().is_none());
        ensure!(!incoming.is_stopped());

        clients.push(connect(&queue, &owner).await?);
        let second = &clients[0];
        let current = config(second).await?;
        ensure!(current.config.model.as_deref() == Some("prepared-original-model"));
        let loaded: ThreadLoadedListResponse =
            bounded(second.request_typed(ClientRequest::ThreadLoadedList {
                request_id: RequestId::Integer(3),
                params: Default::default(),
            }))
            .await??;
        // No rollout exists from which another runtime could cold-resume it.
        ensure!(loaded.data.contains(&started.thread.id));
        ensure!(owner.wait().now_or_never().is_none());
        Ok(())
    }
    .await;
    // Always finish the actual owner and client workers, including on an RPC
    // error. Assertions above return errors rather than skipping this drain.
    let completion = bounded(owner.shutdown()).await;
    let mut client_completions = Vec::new();
    for client in clients {
        client_completions.push(bounded(client.shutdown()).await);
    }
    // Drain every worker before reporting the primary RPC/assertion failure.
    // Cleanup failures must also fail an otherwise successful control.
    result?;
    let completion = completion?;
    ensure!(completion.is_ok());
    for completion in client_completions {
        completion??;
    }
    ensure!(incoming.is_stopped());
    Ok(())
}

#[tokio::test]
async fn prepared_local_factory_cancelled_shutdown_observation_keeps_completion()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let (_queue, receive) = mpsc::channel(2);
    let incoming = Incoming::new(Provider {
        queue: Mutex::new(receive),
        withdrawn: CancellationToken::new(),
    });
    let owner = prepared(&home)
        .await?
        .start_retained(incoming.clone())
        .await?;
    let mut observation = Box::pin(owner.shutdown());
    let _ = futures::poll!(observation.as_mut());
    drop(observation);
    let stopped_when_observation_dropped = incoming.is_stopped();
    // Even a synchronous-withdrawal regression must attempt the actual owner
    // drain before the invariant is reported. Neither result short-circuits
    // the second observation of the same retained completion.
    let completed = bounded(owner.shutdown()).await;
    let observed_again = bounded(owner.wait()).await;
    ensure!(stopped_when_observation_dropped);
    let completed = completed?;
    let observed_again = observed_again?;
    ensure!(Arc::ptr_eq(&completed, &observed_again));
    ensure!(completed.is_ok());
    Ok(())
}
