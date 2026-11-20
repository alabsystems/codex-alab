// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual provider UI -> Write::start -> typed client command -> original event.
//! CloudClient::channel is a real owned in-process command transport fixture;
//! this does not start an embedded server or supply any Native/Source grant.
use crate::app::AppEvent;
use crate::app::provider;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudOperation;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ModelProviderListResponse;
use codex_app_server_protocol::ModelProviderSummary;
use codex_app_server_protocol::ProviderSettings;
use std::collections::VecDeque;
use std::future::Future;
use std::future::poll_fn;
use std::io;
use std::task::Poll;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(60);
fn failure(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}

#[tokio::test]
async fn actual_provider_write_cut_keeps_one_rpc_and_its_completion_through_cancelled_observation()
-> io::Result<()> {
    for confirmed in [true, false] {
        let (mut app, mut events, _ops) =
            crate::boxed_future!(crate::app::tests::make_test_app_with_channels()).await;
        let mut tui = crate::tui::test_support::make_test_tui().map_err(failure)?;
        let (client, mut commands, backend_events) = CloudClient::channel(2);
        let session = AppServerSession::new(
            crate::backend_id::BackendId::new(),
            AppServerClient::Cloud(client),
            ThreadParamsMode::Remote,
        );
        let backend = session.backend_id();
        let original_model_provider = app.chat_widget.config_ref().model_provider_id.clone();
        let body = async {
            // Construct a native provider list through its original Loaded handler,
            // then use the actual list's Space callback to call Write::start.
            app.chat_widget.show_selection_view(provider::loading().0);
            let (_dismiss, dismissed) = tokio::sync::watch::channel(());
            provider::handle(&mut app, &mut tui, &session, provider::Event::Loaded {
                backend, dismissed, notice: None,
                result: Ok(ModelProviderListResponse {
                    data: vec![ModelProviderSummary { id: "original-provider".into(), name: "Original provider".into(), can_edit: false, base_url: None, settings: ProviderSettings::OpenAi }],
                    default_provider_id: "old-default".into(), can_change_default: true,
                }),
            });
            app.chat_widget.handle_key_event(crossterm::event::KeyCode::Char(' ').into());
            let mut original_fifo = VecDeque::new();
            let write = tokio::time::timeout(WAIT, async {
                loop {
                    let event = events.recv().await.ok_or_else(|| failure("original App event receiver closed"))?;
                    match event {
                        AppEvent::Provider(event @ provider::Event::Write { .. }) => break Ok::<_, io::Error>(event),
                        event => original_fifo.push_back(event),
                    }
                }
            }).await.map_err(failure)??;
            provider::handle(&mut app, &mut tui, &session, write);
            let lock = app.provider_writes.get(&backend).ok_or_else(|| failure("actual provider guard missing"))?.clone();
            if !std::sync::Arc::ptr_eq(&lock, &app.model_catalog.provider_write) { return Err(failure("provider map and catalog lost the same original guard")); }
            if lock.try_lock().is_ok() { return Err(failure("same original write must retain the actual guard")); }
            let command = tokio::time::timeout(WAIT, commands.recv()).await.map_err(failure)?.ok_or_else(|| failure("original provider write RPC absent"))?;
            let original_request = match &command.operation {
                CloudOperation::Request(request) => match request.as_ref() {
                    ClientRequest::ConfigValueWrite { request_id, params } if params.key_path == "model_provider" && params.value == serde_json::json!("original-provider") => request_id.clone(),
                    _ => return Err(failure("actual Write::start changed its typed provider RPC")),
                },
                _ => return Err(failure("actual provider path did not issue its write RPC")),
            };
            let cut = session.alab_capture_original_session_jobs()?;
            // Dropping only the observer must return its join lease to the SAME
            // original job. A wrapper-only test would miss an untracked Write::start.
            {
                let mut observing = std::pin::pin!(cut.observe());
                poll_fn(|cx| match observing.as_mut().poll(cx) {
                    Poll::Pending => Poll::Ready(Ok(())),
                    Poll::Ready(_) => Poll::Ready(Err(failure("provider cut settled before original RPC/completion publication"))),
                }).await?;
            }
            if lock.try_lock().is_ok() { return Err(failure("cancelled observation released the original write guard")); }
            if commands.try_recv().is_ok() { return Err(failure("cancelled observation resubmitted original write")); }
            if confirmed {
                command.response.send(Ok(Ok(serde_json::json!({"status":"ok","version":"original-version","filePath":app.config.codex_home.join("config.toml"),"overriddenMetadata":null})))).map_err(|_| failure("same original write response owner lost"))?;
            } else {
                command.response.send(Err(io::Error::other("original response lost"))).map_err(|_| failure("same original write failure owner lost"))?;
            }
            let ready = tokio::time::timeout(WAIT, cut.observe()).await.map_err(failure)??;
            ready.current(&session)?;
            if !*lock.try_lock().map_err(failure)? { return Err(failure("confirmed and possible writes both preserve stale-catalog state")); }
            // A completed join must imply publication AFTER the real guard drop.
            // Move the SAME original queued events into local FIFO custody; never
            // synthesize, clone, dispatch or apply a replacement completion.
            while let Ok(event) = events.try_recv() { original_fifo.push_back(event); }
            let completions = original_fifo.iter().filter(|event| matches!(event, AppEvent::Provider(provider::Event::Written(_)))).count();
            if completions != 1 { return Err(failure(format!("joined original write has {completions} actual completion events"))); }
            if commands.try_recv().is_ok() { return Err(failure(format!("write {original_request:?} was resubmitted"))); }
            if app.chat_widget.config_ref().model_provider_id != original_model_provider { return Err(failure("provider write changed the current thread provider")); }
            Ok::<_, io::Error>(())
        }.await;
        // Always close the same real command transport and join its original
        // work. Outstanding test requests get a definite local cleanup failure.
        let cleanup = tokio::time::timeout(WAIT, async {
            let respond = async {
                while let Some(command) = commands.recv().await {
                    if matches!(command.operation, CloudOperation::Shutdown) {
                        command
                            .response
                            .send(Ok(Ok(serde_json::Value::Null)))
                            .map_err(|_| failure("original shutdown response owner lost"))?;
                        break;
                    }
                    let _sent = command
                        .response
                        .send(Err(failure("owned provider fixture cleanup")));
                }
                Ok::<_, io::Error>(())
            };
            let (closed, responded) = tokio::join!(session.shutdown(), respond);
            responded?;
            closed
        })
        .await
        .map_err(failure)?;
        drop(backend_events);
        body?;
        cleanup?;
    }
    Ok(())
}
