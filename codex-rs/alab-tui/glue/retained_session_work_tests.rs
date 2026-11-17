// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual Tokio workers/descendants and native picker channels; no Native/Source grants.
use super::*;
use std::future::poll_fn;
const WAIT: std::time::Duration = std::time::Duration::from_secs(60);
fn actual_error(message: &str) -> io::Error {
    invalid(message)
}
async fn cleanup(book: &Book) {
    let jobs = {
        book.0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .jobs
            .clone()
    };
    for job in jobs {
        if let Some(worker) = job
            .life
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .worker
            .as_ref()
        {
            worker.abort();
        }
    }
    let _joined = book.drain().await;
}
#[tokio::test]
async fn canceled_actual_worker_observation_restores_handle_without_canceling_original()
-> io::Result<()> {
    let book = Book::new();
    let (release, released) = tokio::sync::oneshot::channel();
    book.spawn_original(async move {
        let _released = released.await;
    })
    .map_err(|refused| refused.error)?;
    let body = async {
        let cut = book.capture()?;
        {
            let mut observing = std::pin::pin!(cut.observe());
            poll_fn(|cx| match observing.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => Poll::Ready(Err(actual_error(
                    "original worker ended before actual release",
                ))),
            })
            .await?;
        }
        if book.0.progress.lock().map_err(invalid)?.jobs[0]
            .life
            .lock()
            .map_err(invalid)?
            .worker
            .is_none()
        {
            return Err(actual_error("canceled join lost same original handle"));
        }
        release
            .send(())
            .map_err(|_| actual_error("original release lost"))?;
        tokio::time::timeout(WAIT, cut.observe())
            .await
            .map_err(|_| actual_error("same worker did not join"))??;
        Ok(())
    }
    .await;
    cleanup(&book).await;
    body
}
#[tokio::test]
async fn actual_already_accepted_parent_can_register_child_after_root_close_and_both_join()
-> io::Result<()> {
    let book = Book::new();
    let (parent_release, parent_released) = tokio::sync::oneshot::channel();
    let (child_release, child_released) = tokio::sync::oneshot::channel();
    let (child_started, started) = tokio::sync::oneshot::channel();
    book.spawn_original(async move {
        let _released = parent_released.await;
        let result = spawn_original_descendant(async move {
            let _released = child_released.await;
        });
        let _reported = child_started.send(result.is_ok());
    })
    .map_err(|refused| refused.error)?;
    let body = async {
        book.close();
        let refused = book.spawn_original(std::future::pending());
        if refused.is_ok() {
            return Err(actual_error("fresh root worker bypassed actual close"));
        }
        parent_release
            .send(())
            .map_err(|_| actual_error("actual parent release lost"))?;
        if !tokio::time::timeout(WAIT, started)
            .await
            .map_err(|_| actual_error("actual child not registered"))?
            .map_err(invalid)?
        {
            return Err(actual_error(
                "accepted original descendant was suppressed by root closure",
            ));
        }
        {
            let mut draining = std::pin::pin!(book.drain());
            poll_fn(|cx| match draining.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => {
                    Poll::Ready(Err(actual_error("shutdown skipped held actual child")))
                }
            })
            .await?;
        }
        child_release
            .send(())
            .map_err(|_| actual_error("actual child release lost"))?;
        tokio::time::timeout(WAIT, book.drain())
            .await
            .map_err(|_| actual_error("original parent/child joins timed out"))??;
        Ok(())
    }
    .await;
    cleanup(&book).await;
    body
}
#[tokio::test]
async fn actual_joined_completed_jobs_retire_but_real_worker_loss_does_not_become_empty_ready()
-> io::Result<()> {
    let book = Book::new();
    book.spawn_original(async {})
        .map_err(|refused| refused.error)?;
    let body = async {
        let ready = tokio::time::timeout(WAIT, book.capture()?.observe())
            .await
            .map_err(|_| actual_error("actual worker join timed out"))??;
        ready.check()?;
        let next = book.capture()?;
        if !next.jobs.is_empty() {
            return Err(actual_error(
                "actual joined completed worker history remained pinned",
            ));
        }
        book.spawn_original(std::future::pending())
            .map_err(|refused| refused.error)?;
        let job = {
            book.0
                .progress
                .lock()
                .map_err(invalid)?
                .jobs
                .last()
                .cloned()
                .ok_or_else(|| actual_error("actual pending job absent"))?
        };
        if let Some(worker) = job.life.lock().map_err(invalid)?.worker.as_ref() {
            worker.abort();
        }
        if tokio::time::timeout(WAIT, job.join())
            .await
            .map_err(|_| actual_error("actual abort join timed out"))?
            .is_ok()
        {
            return Err(actual_error("actual abort became success"));
        }
        if book.capture()?.observe().await.is_ok() {
            return Err(actual_error(
                "actual lost worker was reaped as a successful empty cut",
            ));
        }
        Ok(())
    }
    .await;
    cleanup(&book).await;
    body
}
#[tokio::test]
async fn actual_picker_keeps_original_request_uuid_result_and_channel_while_worker_is_joined()
-> io::Result<()> {
    let (client, mut commands, events) = codex_app_server_client::CloudClient::channel(2);
    let session = super::super::AppServerSession::new(
        crate::backend_id::BackendId::new(),
        codex_app_server_client::AppServerClient::Cloud(client),
        super::super::ThreadParamsMode::Remote,
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let id = uuid::Uuid::new_v4();
    fetch_picker(
        &session,
        id,
        crate::app_event_sender::AppEventSender::new(tx),
    );
    let body=async {
        let command=tokio::time::timeout(WAIT,commands.recv()).await.map_err(|_|actual_error("original picker request missing"))?.ok_or_else(||actual_error("original picker command closed"))?;
        if !matches!(command.operation,codex_app_server_client::CloudOperation::Request(ref request) if matches!(request.as_ref(),codex_app_server_protocol::ClientRequest::ModelList {request_id,..} if *request_id==codex_app_server_protocol::RequestId::String(format!("model-list-{id}")))) {return Err(actual_error("original picker method/request UUID changed"));}
        command.response.send(Ok(Ok(serde_json::json!({"data":[],"nextCursor":null})))).map_err(|_|actual_error("same picker response observer lost"))?;
        let cut=session.alab_original_work.capture()?;tokio::time::timeout(WAIT,cut.observe()).await.map_err(|_|actual_error("actual picker worker join timed out"))??;
        let event=rx.try_recv().map_err(invalid)?;
        if !matches!(event,crate::app_event::AppEvent::ModelsLoaded {request_id,result:Ok(ref models)} if request_id==id&&models.is_empty()) {return Err(actual_error("original picker native result/UUID was not handed to original event queue"));}Ok(())
    }.await;
    drop(commands);
    drop(events);
    cleanup(&session.alab_original_work).await;
    body
}

#[tokio::test]
async fn native_cleanup_admitted_during_actual_close_is_still_joined_without_a_synthetic_title_result()
-> io::Result<()> {
    let book = Book::new();
    let (a_release, a_released) = tokio::sync::oneshot::channel();
    let (b_release, b_released) = tokio::sync::oneshot::channel();
    book.spawn_native_original(async move {
        let _released = a_released.await;
    });
    let body = async {
        book.close();
        {
            let mut joining = std::pin::pin!(book.drain());
            poll_fn(|cx| match joining.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => {
                    Poll::Ready(Err(actual_error("actual first worker unexpectedly joined")))
                }
            })
            .await?;
        }
        // The actual original App handler can deliver late cleanup after stop;
        // tracking may not replace it with a manufactured successful/error event.
        book.spawn_native_original(async move {
            let _released = b_released.await;
        });
        a_release
            .send(())
            .map_err(|_| actual_error("actual first release lost"))?;
        {
            let mut joining = std::pin::pin!(book.drain());
            poll_fn(|cx| match joining.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => Poll::Ready(Err(actual_error("actual late cleanup was skipped"))),
            })
            .await?;
        }
        b_release
            .send(())
            .map_err(|_| actual_error("actual late cleanup release lost"))?;
        tokio::time::timeout(WAIT, book.drain())
            .await
            .map_err(|_| actual_error("actual late cleanup join timed out"))??;
        Ok(())
    }
    .await;
    cleanup(&book).await;
    body
}
#[tokio::test]
async fn actual_native_feature_discovery_child_and_original_parent_are_both_retained_and_joined()
-> io::Result<()> {
    use codex_app_server_protocol::ClientRequest;
    use codex_app_server_protocol::ExperimentalFeature;
    use codex_app_server_protocol::ExperimentalFeatureListResponse;
    use codex_app_server_protocol::ExperimentalFeatureStage;
    use codex_app_server_protocol::RequestId;
    let (client, mut commands, events) = codex_app_server_client::CloudClient::channel(2);
    let session = super::super::AppServerSession::new(
        crate::backend_id::BackendId::new(),
        codex_app_server_client::AppServerClient::Cloud(client),
        super::super::ThreadParamsMode::Remote,
    );
    let handle = session.request_handle();
    let thread = codex_protocol::ThreadId::new();
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
    session.alab_spawn_original_work(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        crate::experimental_features::fetch(handle, Some(thread), "tui-daybreak-forwarding", tx);
        let _sent = result_tx.send(rx.await);
    });
    let body=async {
        let command=tokio::time::timeout(WAIT,commands.recv()).await.map_err(|_|actual_error("original feature request missing"))?.ok_or_else(||actual_error("original feature channel closed"))?;
        if !matches!(command.operation,codex_app_server_client::CloudOperation::Request(ref request) if matches!(request.as_ref(),ClientRequest::ExperimentalFeatureList {request_id,params} if *request_id==RequestId::String("tui-daybreak-forwarding".into())&&params.thread_id.as_deref()==Some(thread.to_string().as_str()))) {return Err(actual_error("same reserved feature request/thread identity changed"));}
        let cut=session.alab_capture_original_session_jobs()?;
        if cut.jobs.len()!=2 {return Err(actual_error("actual discovery child was not registered in the original parent domain"));}
        let expected=ExperimentalFeature {name:"api_key_cyber_access_programs".into(),stage:ExperimentalFeatureStage::Beta,display_name:Some("Original native feature".into()),description:None,announcement:None,enabled:true,default_enabled:false};
        let response=ExperimentalFeatureListResponse {data:vec![expected.clone()],next_cursor:None};
        command.response.send(Ok(Ok(serde_json::to_value(response).map_err(invalid)?))).map_err(|_|actual_error("actual discovery result receiver lost"))?;
        let result=tokio::time::timeout(WAIT,result_rx).await.map_err(|_|actual_error("actual parent result delivery timed out"))?.map_err(invalid)?.map_err(invalid)?.map_err(invalid)?;
        if result!=vec![expected] {return Err(actual_error("actual original feature result changed"));}
        let ready=tokio::time::timeout(WAIT,cut.observe()).await.map_err(|_|actual_error("actual parent and child joins timed out"))??;ready.current(&session)?;Ok(())
    }.await;
    drop(commands);
    drop(events);
    cleanup(&session.alab_original_work).await;
    body
}
#[tokio::test]
async fn actual_original_title_start_error_is_delivered_once_and_joins_without_repeating_a_thread_request()
-> io::Result<()> {
    let (client, mut commands, events) = codex_app_server_client::CloudClient::channel(2);
    let session = super::super::AppServerSession::new(
        crate::backend_id::BackendId::new(),
        codex_app_server_client::AppServerClient::Cloud(client),
        super::super::ThreadParamsMode::Remote,
    );
    let handle = session.request_handle();
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
    session.alab_spawn_original_work(async move {
        let options = crate::temporary_structured_request::TemporaryStructuredThreadOptions {
            kind: crate::temporary_structured_request::TemporaryThreadKind::Title,
            model: "original-title-model".into(),
            model_provider: "openai".into(),
            cwd: "/original/title/cwd".into(),
            active_permission_profile: None,
        };
        let result = crate::temporary_structured_request::start_temporary_thread(&handle, options)
            .await
            .map(|thread| thread.thread.id)
            .map_err(|error| error.to_string());
        let _sent = result_tx.send(result);
    });
    let body=async {
        let command=tokio::time::timeout(WAIT,commands.recv()).await.map_err(|_|actual_error("actual original title config read missing"))?.ok_or_else(||actual_error("original title request channel closed"))?;
        if !matches!(command.operation,codex_app_server_client::CloudOperation::Request(ref request) if matches!(request.as_ref(),codex_app_server_protocol::ClientRequest::ConfigRead {..})) {return Err(actual_error("original title startup policy was bypassed"));}
        command.response.send(Err(io::Error::other("original title transport failed"))).map_err(|_|actual_error("actual title error receiver lost"))?;
        let result=tokio::time::timeout(WAIT,result_rx).await.map_err(|_|actual_error("actual title result timed out"))?.map_err(invalid)?;
        // Original title startup displays the config helper's outer context;
        // it deliberately does not expand the underlying transport error chain.
        std::assert_eq!(result,Err("config/read failed in TUI".to_owned()));
        let cut=session.alab_capture_original_session_jobs()?;tokio::time::timeout(WAIT,cut.observe()).await.map_err(|_|actual_error("actual title task join timed out"))??;
        if commands.try_recv().is_ok() {return Err(actual_error("tracking repeated a model/title thread request after original failure"));}Ok(())
    }.await;
    drop(commands);
    drop(events);
    cleanup(&session.alab_original_work).await;
    body
}
