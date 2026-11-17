// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real local-auth, Unix IO and retained app-server component controls. Test IO
//! is not Native admission, and deterministic picker choices are not human ACKs.
use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RetainedRuntimeOwner;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::RequestId;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::ensure;
use futures::FutureExt;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

struct Provider {
    queue: Mutex<mpsc::Receiver<UnixStream>>,
    withdrawn: CancellationToken,
}
impl IncomingStreamProvider for Provider {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::poll_fn(|cx| {
            self.queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .poll_recv(cx)
                .map(|next| {
                    Ok(next.map(codex_alab_core::native_incoming::AcceptedConnection::stream_only))
                })
        }))
    }
    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(self.withdrawn.cancelled())
    }
}
fn incoming() -> (mpsc::Sender<UnixStream>, Incoming) {
    let (send, receive) = mpsc::channel(2);
    (
        send,
        Incoming::new(Provider {
            queue: Mutex::new(receive),
            withdrawn: CancellationToken::new(),
        }),
    )
}
async fn bounded<F: Future>(future: F) -> color_eyre::Result<F::Output> {
    tokio::time::timeout(Duration::from_secs(30), future)
        .await
        .wrap_err("Source preparation control timed out")
}
async fn inputs(home: &TempDir, explicit: Option<&str>) -> color_eyre::Result<BootstrapInputs> {
    let cli_kv_overrides = vec![
        (
            "features.shell_snapshot".into(),
            toml::Value::Boolean(false),
        ),
        ("features.plugins".into(), toml::Value::Boolean(false)),
        (
            "model".into(),
            toml::Value::String("phase-original-model".into()),
        ),
        (
            "cli_auth_credentials_store".into(),
            toml::Value::String("file".into()),
        ),
    ];
    let loader_overrides = LoaderOverrides::without_managed_config_for_tests();
    let local = crate::legacy_core::config::load_config_toml_with_layer_stack(
        home.path(),
        None,
        cli_kv_overrides.clone(),
        codex_config::ConfigLoadOptions {
            loader_overrides: loader_overrides.clone(),
            strict_config: false,
            cloud_config_bundle: CloudConfigBundleLoader::default(),
        },
    )
    .await?;
    Ok(BootstrapInputs {
        local,
        codex_home: home.path().into(),
        config_cwd: None,
        cli_kv_overrides,
        loader_overrides,
        strict_config: false,
        network_policy: EmbeddedNetworkPolicy::default(),
        explicit_oss_provider: explicit.map(Into::into),
    })
}
fn needs_selection() -> crate::oss_selection::OssProviderDetection {
    crate::oss_selection::OssProviderDetection::NeedsSelection {
        lmstudio_status: crate::oss_selection::ProviderStatus::Unknown,
        ollama_status: crate::oss_selection::ProviderStatus::NotRunning,
    }
}
async fn final_inputs(
    bootstrap: &PreparedBootstrap,
) -> color_eyre::Result<super::super::owned_local_bootstrap::PreparedLocalInputs> {
    let original = bootstrap.original_inputs();
    let mut overrides = original.cli_kv_overrides.clone();
    overrides.push((
        "model_provider".into(),
        toml::Value::String(bootstrap.provider().into()),
    ));
    let cloud = bootstrap.cloud_bundle().clone();
    let config = ConfigBuilder::default()
        .codex_home(original.codex_home.clone())
        .fallback_cwd(Some(original.codex_home.clone()))
        .cli_overrides(overrides.clone())
        .loader_overrides(original.loader_overrides.clone())
        .cloud_config_bundle(cloud.clone())
        .strict_config(original.strict_config)
        .build()
        .await?;
    // Explicit fixture resources exercise the factory; they do not claim that
    // the unconnected ordinary CLI/worktree/environment recipe has run.
    Ok(super::super::owned_local_bootstrap::PreparedLocalInputs {
        arg0_paths: codex_arg0::Arg0DispatchPaths::default(),
        config,
        cli_kv_overrides: overrides,
        loader_overrides: original.loader_overrides.clone(),
        strict_config: original.strict_config,
        cloud_config_bundle: cloud,
        feedback: codex_feedback::CodexFeedback::new(),
        log_db: None,
        state_db: None,
        environment_manager: Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        embedded_network_policy: original.network_policy.clone(),
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
async fn join_frontend(
    mut task: tokio::task::JoinHandle<io::Result<()>>,
) -> color_eyre::Result<()> {
    match tokio::time::timeout(Duration::from_secs(30), &mut task).await {
        Ok(result) => {
            result??;
            Ok(())
        }
        Err(_) => {
            task.abort();
            let _ = task.await;
            Err(color_eyre::eyre::eyre!(
                "preparation frontend did not drain"
            ))
        }
    }
}

#[tokio::test]
async fn observed_choice_keeps_actual_bootstrap_and_hands_same_input_to_real_runtime()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let (queue, incoming) = incoming();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut preparation = Some(start_with_detection(
        inputs(&home, None).await?,
        incoming.clone(),
        move || async move {
            called.fetch_add(1, Ordering::SeqCst);
            needs_selection()
        },
    )?);
    let (frontend, backend) = UnixStream::pair()?;
    let (release, gate) = oneshot::channel();
    let mut release = Some(release);
    let (entered, entry) = oneshot::channel();
    let mut frontend = Some(tokio::spawn(channel::serve_test_choice(
        frontend, "ollama", gate, entered,
    )));
    let mut ready = None;
    let mut runtime: Option<PreparedRuntime> = None;
    let mut client = None;
    let result: color_eyre::Result<()> = async {
        bounded(queue.send(backend)).await??;
        bounded(entry).await??;
        let owner = preparation
            .as_ref()
            .ok_or_else(|| io::Error::other("test preparation absent"))?;
        ensure!(owner.wait().now_or_never().is_none()); // Cancel only observation.
        ensure!(!incoming.is_stopped());
        release
            .take()
            .ok_or_else(|| io::Error::other("test release absent"))?
            .send(())
            .map_err(|_| io::Error::other("picker observer lost"))?;
        join_frontend(
            frontend
                .take()
                .ok_or_else(|| io::Error::other("frontend absent"))?,
        )
        .await?;
        let completed = bounded(owner.wait()).await?;
        let repeated = bounded(owner.wait()).await?;
        ensure!(completed.is_ok() && Arc::ptr_eq(&completed, &repeated));
        ready = Some(
            bounded(
                preparation
                    .take()
                    .ok_or_else(|| io::Error::other("test preparation absent"))?
                    .take_ready(),
            )
            .await??,
        );
        let bootstrap = ready
            .as_ref()
            .ok_or_else(|| io::Error::other("test bootstrap absent"))?;
        ensure!(bootstrap.provider() == "ollama" && bootstrap.manually_selected());
        ensure!(calls.load(Ordering::SeqCst) == 1);
        ensure!(bootstrap.original_inputs().codex_home == home.path());
        let manager = bootstrap.bootstrap_manager().clone();
        let final_inputs = final_inputs(bootstrap).await?;
        runtime = Some(
            bounded(
                ready
                    .take()
                    .ok_or_else(|| io::Error::other("test bootstrap absent"))?
                    .start_runtime(final_inputs),
            )
            .await??,
        );
        let owner = runtime
            .as_ref()
            .ok_or_else(|| io::Error::other("runtime absent"))?;
        ensure!(Arc::ptr_eq(&manager, owner._bootstrap.bootstrap_manager()));
        // Preparation used/closed its own unique endpoint. This fresh endpoint
        // reaches the actual final dispatcher through the SAME provider object.
        client = Some(connect(&queue, owner.runtime()).await?);
        let response: ConfigReadResponse = bounded(
            client
                .as_ref()
                .ok_or_else(|| io::Error::other("client absent"))?
                .request_typed(ClientRequest::ConfigRead {
                    request_id: RequestId::Integer(1),
                    params: ConfigReadParams {
                        include_layers: false,
                        cwd: None,
                    },
                }),
        )
        .await??;
        ensure!(response.config.model.as_deref() == Some("phase-original-model"));
        ensure!(response.config.model_provider.as_deref() == Some("ollama"));
        ensure!(!incoming.is_stopped());
        Ok(())
    }
    .await;
    // Drain before propagating the primary result on every path.
    if let Some(release) = release.take() {
        let _ = release.send(());
    }
    let preparation_completion = if let Some(owner) = preparation.as_mut() {
        Some(bounded(owner.shutdown()).await)
    } else {
        None
    };
    let runtime_completion = if let Some(owner) = runtime.as_mut() {
        Some(bounded(owner.shutdown()).await)
    } else {
        None
    };
    let client_completion = if let Some(client) = client {
        Some(bounded(client.shutdown()).await)
    } else {
        None
    };
    let frontend_completion = if let Some(frontend) = frontend {
        Some(join_frontend(frontend).await)
    } else {
        None
    };
    let ready_completion = if let Some(owner) = ready.as_mut() {
        Some(bounded(owner.shutdown()).await)
    } else {
        None
    };
    drop(ready);
    result?;
    if let Some(completion) = preparation_completion {
        let completion = completion?;
        ensure!(completion.is_ok());
    }
    if let Some(completion) = runtime_completion {
        let completion = completion?;
        ensure!(completion.is_ok());
    }
    if let Some(completion) = client_completion {
        completion??;
    }
    if let Some(completion) = frontend_completion {
        completion?;
    }
    if let Some(completion) = ready_completion {
        let completion = completion?;
        ensure!(completion.is_ok());
    }
    ensure!(incoming.is_stopped());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn complete_frame_waits_for_actual_preparation_peer_close() -> color_eyre::Result<()> {
    let (mut frontend, mut backend) = UnixStream::pair()?;
    let mut worker = tokio::spawn(async move {
        channel::select(
            &mut backend,
            crate::oss_selection::OssProviderDetection::AutoSelected(
                crate::oss_selection::OssProviderSelection {
                    provider: "ollama".into(),
                    manually_selected: false,
                },
            ),
        )
        .await
    });
    let result: color_eyre::Result<()> = async {
        let message: channel::Request = bounded(channel::read(&mut frontend)).await??;
        let channel::Request::Automatic { request, provider } = message else {
            color_eyre::eyre::bail!("preparation did not send its automatic selection");
        };
        ensure!(provider == "ollama");
        bounded(channel::write(
            &mut frontend,
            &channel::Reply::Observed {
                request: request.clone(),
            },
        ))
        .await??;
        let message: channel::Request = bounded(channel::read(&mut frontend)).await??;
        let channel::Request::Complete { request: actual } = message else {
            color_eyre::eyre::bail!("preparation did not send its completion frame");
        };
        ensure!(actual == request);
        bounded(channel::write(
            &mut frontend,
            &channel::Reply::Observed { request },
        ))
        .await??;
        tokio::task::yield_now().await;
        // A completed data frame cannot release the phase while its physical
        // peer still owns this endpoint. This is no Native frontier receipt.
        ensure!(!worker.is_finished());
        Ok(())
    }
    .await;
    drop(frontend);
    let completion = match tokio::time::timeout(Duration::from_secs(30), &mut worker).await {
        Ok(joined) => joined.wrap_err("preparation protocol worker failed"),
        Err(_) => {
            worker.abort();
            let _ = worker.await;
            Err(color_eyre::eyre::eyre!(
                "preparation peer close did not drain"
            ))
        }
    };
    result?;
    let selected = completion??;
    ensure!(selected.provider == "ollama" && !selected.manually_selected);
    Ok(())
}

#[tokio::test]
async fn wrong_reply_is_one_stored_failure_without_detection_or_delivery_retry()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let (queue, incoming) = incoming();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut owner = start_with_detection(
        inputs(&home, None).await?,
        incoming.clone(),
        move || async move {
            called.fetch_add(1, Ordering::SeqCst);
            needs_selection()
        },
    )?;
    let (mut frontend, backend) = UnixStream::pair()?;
    let peer = tokio::spawn(async move {
        let _: channel::Request = channel::read(&mut frontend).await?;
        channel::write(
            &mut frontend,
            &channel::Reply::Selected {
                request: uuid::Uuid::new_v4().to_string(),
                provider: "ollama".into(),
            },
        )
        .await
    });
    let result: color_eyre::Result<()> = async {
        bounded(queue.send(backend)).await??;
        let first = bounded(owner.wait()).await?;
        let second = bounded(owner.wait()).await?;
        ensure!(
            first.as_ref().as_ref().err().map(io::Error::kind) == Some(io::ErrorKind::InvalidData)
        );
        ensure!(Arc::ptr_eq(&first, &second));
        ensure!(calls.load(Ordering::SeqCst) == 1);
        Ok(())
    }
    .await;
    let completion = bounded(owner.shutdown()).await;
    let peer = join_frontend(peer).await;
    result?;
    let completion = completion?;
    ensure!(completion.is_err());
    peer?;
    ensure!(incoming.is_stopped());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn unpolled_worker_destruction_records_unknown_and_keeps_original_input()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let (_queue, incoming) = incoming();
    let mut owner = start_with_detection(inputs(&home, None).await?, incoming.clone(), || async {
        needs_selection()
    })?;
    let worker = owner
        .worker
        .take()
        .ok_or_else(|| io::Error::other("test worker absent"))?;
    worker.abort();
    let joined = bounded(worker).await;
    let observed = bounded(owner.wait()).await;
    let repeated = bounded(owner.shutdown()).await;
    let joined = joined?;
    ensure!(
        joined
            .as_ref()
            .is_err_and(tokio::task::JoinError::is_cancelled)
    );
    let observed = observed?;
    let repeated = repeated?;
    ensure!(
        observed
            .as_ref()
            .as_ref()
            .err()
            .is_some_and(|error| error.to_string() == "Source preparation outcome unobserved")
    );
    ensure!(Arc::ptr_eq(&observed, &repeated));
    ensure!(owner.shared.inputs.codex_home == home.path());
    ensure!(incoming.is_stopped());
    Ok(())
}

#[tokio::test]
async fn explicit_provider_skips_detection_and_dropping_ready_withdraws_exact_input()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let (queue, incoming) = incoming();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut owner = Some(start_with_detection(
        inputs(&home, Some("custom-oss-provider")).await?,
        incoming.clone(),
        move || async move {
            called.fetch_add(1, Ordering::SeqCst);
            needs_selection()
        },
    )?);
    let (frontend, backend) = UnixStream::pair()?;
    // The genuine frontend method never opens the picker for Automatic.
    let frontend = tokio::spawn(channel::serve_frontend(frontend));
    let result: color_eyre::Result<()> = async {
        bounded(queue.send(backend)).await??;
        let ready = bounded(
            owner
                .take()
                .ok_or_else(|| io::Error::other("test owner absent"))?
                .take_ready(),
        )
        .await??;
        ensure!(ready.provider() == "custom-oss-provider" && !ready.manually_selected());
        ensure!(calls.load(Ordering::SeqCst) == 0);
        ensure!(!incoming.is_stopped());
        drop(ready);
        ensure!(incoming.is_stopped());
        Ok(())
    }
    .await;
    if let Some(owner) = owner.as_mut() {
        let _ = bounded(owner.shutdown()).await;
    }
    let frontend = join_frontend(frontend).await;
    result?;
    frontend?;
    Ok(())
}

#[tokio::test]
async fn cancelled_picker_finishes_both_actual_protocol_sides_without_complete()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let (queue, incoming) = incoming();
    let mut owner = start_with_detection(inputs(&home, None).await?, incoming.clone(), || async {
        needs_selection()
    })?;
    let (frontend, backend) = UnixStream::pair()?;
    let (release, gate) = oneshot::channel();
    let (entered, entry) = oneshot::channel();
    let mut release = Some(release);
    let mut frontend = tokio::spawn(channel::serve_test_choice(
        frontend,
        "__CANCELLED__",
        gate,
        entered,
    ));
    let result: color_eyre::Result<()> = async {
        bounded(queue.send(backend)).await??;
        bounded(entry).await??;
        release
            .take()
            .ok_or_else(|| io::Error::other("test release absent"))?
            .send(())
            .map_err(|_| io::Error::other("picker observer lost"))?;
        // Observe frontend termination BEFORE shutting down backend custody.
        // It cannot depend on owner Drop/EOF to escape a missing Complete.
        let reply = bounded(&mut frontend).await??;
        ensure!(reply.as_ref().err().map(io::Error::kind) == Some(io::ErrorKind::Interrupted));
        let result = bounded(owner.wait()).await?;
        ensure!(
            result.as_ref().as_ref().err().map(io::Error::kind) == Some(io::ErrorKind::Interrupted)
        );
        Ok(())
    }
    .await;
    if let Some(release) = release {
        let _ = release.send(());
    }
    let completion = bounded(owner.shutdown()).await;
    // A cancelled successful JoinHandle observation remains owned. Do not poll
    // an already completed JoinHandle again; its finished bit is only cleanup.
    if !frontend.is_finished() {
        frontend.abort();
        let _ = frontend.await;
    }
    result?;
    let completion = completion?;
    ensure!(completion.is_err());
    ensure!(incoming.is_stopped());
    Ok(())
}

#[tokio::test]
async fn owner_stop_closes_actual_waiting_preparation_peer_and_reuses_shutdown_result()
-> color_eyre::Result<()> {
    use tokio::io::AsyncReadExt;
    let home = TempDir::new()?;
    let (queue, incoming) = incoming();
    let mut owner = start_with_detection(inputs(&home, None).await?, incoming.clone(), || async {
        needs_selection()
    })?;
    let (mut frontend, backend) = UnixStream::pair()?;
    let result: color_eyre::Result<()> = async {
        bounded(queue.send(backend)).await??;
        let request: channel::Request = bounded(channel::read(&mut frontend)).await??;
        ensure!(matches!(request, channel::Request::Select { .. }));
        // No response: stopping the actual owner must close its retained phase
        // endpoint, even while the completed owner object remains alive.
        let mut observation = Box::pin(owner.shutdown());
        let _ = futures::poll!(observation.as_mut());
        drop(observation);
        let result = bounded(owner.shutdown()).await?;
        let repeated = bounded(owner.shutdown()).await?;
        ensure!(result.is_err() && Arc::ptr_eq(&result, &repeated));
        let mut byte = [0_u8; 1];
        ensure!(bounded(frontend.read(&mut byte)).await?? == 0);
        ensure!(
            owner
                .shared
                .resources
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .stream
                .is_none()
        );
        Ok(())
    }
    .await;
    let completion = bounded(owner.shutdown()).await;
    result?;
    let completion = completion?;
    ensure!(completion.is_err());
    ensure!(incoming.is_stopped());
    Ok(())
}

#[tokio::test]
async fn ready_owner_can_observe_same_original_refresh_shutdown_before_runtime()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let (queue, incoming) = incoming();
    let mut owner = Some(start_with_detection(
        inputs(&home, Some("ollama")).await?,
        incoming.clone(),
        || async { needs_selection() },
    )?);
    let (frontend, backend) = UnixStream::pair()?;
    let frontend = tokio::spawn(channel::serve_frontend(frontend));
    let mut ready = None;
    let result: color_eyre::Result<()> = async {
        bounded(queue.send(backend)).await??;
        ready = Some(
            bounded(
                owner
                    .take()
                    .ok_or_else(|| io::Error::other("test owner absent"))?
                    .take_ready(),
            )
            .await??,
        );
        let ready = ready
            .as_mut()
            .ok_or_else(|| io::Error::other("test bootstrap absent"))?;
        let manager = ready.bootstrap_manager().clone();
        // Exercise pre-runtime cleanup without claiming a final-policy validator.
        // Cancellation discards only observation of the original refresh join.
        let mut observation = Box::pin(ready.shutdown());
        let _ = futures::poll!(observation.as_mut());
        drop(observation);
        let first = bounded(ready.shutdown()).await?;
        let second = bounded(ready.shutdown()).await?;
        ensure!(first.is_ok() && Arc::ptr_eq(&first, &second));
        ensure!(Arc::ptr_eq(&manager, ready.bootstrap_manager()));
        ensure!(incoming.is_stopped());
        Ok(())
    }
    .await;
    let phase = if let Some(owner) = owner.as_mut() {
        Some(bounded(owner.shutdown()).await)
    } else {
        None
    };
    let refresh = if let Some(owner) = ready.as_mut() {
        Some(bounded(owner.shutdown()).await)
    } else {
        None
    };
    let frontend = join_frontend(frontend).await;
    result?;
    if let Some(completion) = phase {
        let completion = completion?;
        ensure!(completion.is_ok());
    }
    if let Some(completion) = refresh {
        let completion = completion?;
        ensure!(completion.is_ok());
    }
    frontend?;
    Ok(())
}
