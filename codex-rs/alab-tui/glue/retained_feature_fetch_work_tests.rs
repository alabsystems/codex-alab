// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual App popup fetch root, original lock wait, same discovery child/result.
//! CloudClient::channel supplies real owned typed transport, not Native authority.
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudOperation;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_protocol::ThreadId;
use std::future::Future;
use std::future::poll_fn;
use std::io;
use std::task::Poll;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(60);
fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

#[tokio::test]
async fn actual_feature_fetch_cut_retains_lock_wait_child_and_same_popup_result() -> io::Result<()>
{
    for outcome in ["success", "rpc_error", "popup_closed"] {
        let (app, _events, _ops) =
            crate::boxed_future!(crate::app::tests::make_test_app_with_channels()).await;
        let (client, mut commands, backend_events) = CloudClient::channel(2);
        let session = AppServerSession::new(
            crate::backend_id::BackendId::new(),
            AppServerClient::Cloud(client),
            ThreadParamsMode::Remote,
        );
        let thread = ThreadId::new();
        let thread_text = thread.to_string();
        // Hold the SAME App lock that the actual wrapper uses. No substitute
        // worker/API or artificial Session job makes the initial cut pending.
        let mut held = Some(app.feature_write_lock.clone().lock_owned().await);
        let body = async {
            let baseline = session.alab_capture_original_session_jobs()?;
            let baseline_ready = tokio::time::timeout(WAIT, baseline.observe()).await.map_err(failure)??;
            baseline_ready.current(&session)?;
            let (result_tx, mut result_rx) = tokio::sync::oneshot::channel();
            app.fetch_experimental_features(&session, thread, result_tx);
            let waiting = session.alab_capture_original_session_jobs()?;
            {
                let mut observing = std::pin::pin!(waiting.observe());
                poll_fn(|cx| match observing.as_mut().poll(cx) {
                    Poll::Pending => Poll::Ready(Ok(())),
                    Poll::Ready(_) => Poll::Ready(Err(failure("popup cut settled while its original wrapper still waited for the actual feature lock"))),
                }).await?;
            }
            if commands.try_recv().is_ok() { return Err(failure("popup discovery issued an RPC before the original lock was released")); }
            if outcome == "popup_closed" {
                // Original select policy: popup closure ends the accepted root
                // while the same lock is STILL held, without starting discovery.
                drop(result_rx);
                let ready = tokio::time::timeout(WAIT, waiting.observe()).await.map_err(failure)??;
                ready.current(&session)?;
                if held.is_none() || app.feature_write_lock.try_lock().is_ok() { return Err(failure("closed popup required releasing the original held lock")); }
                if commands.try_recv().is_ok() { return Err(failure("closed popup started an unwanted discovery RPC")); }
                return Ok::<_, io::Error>(());
            }
            // Canceling only observe above kept the SAME root/handle. Release
            // the actual lock once so that original root admits its real child.
            drop(held.take());
            let command = tokio::time::timeout(WAIT, commands.recv()).await.map_err(failure)?.ok_or_else(||failure("original popup discovery RPC absent after lock release"))?;
            match &command.operation {
                CloudOperation::Request(request) => match request.as_ref() {
                    ClientRequest::ExperimentalFeatureList { request_id, params }
                        if request_id == &RequestId::String("tui-experimental-features".into())
                        && params.thread_id.as_deref() == Some(thread_text.as_str())
                        && params.cursor.is_none() && params.limit == Some(100) => {},
                    _ => return Err(failure("actual App popup fetch changed its original typed discovery request")),
                },
                _ => return Err(failure("actual popup fetch did not use the real typed request transport")),
            }
            // The lock-wait cut predates actual child admission. Preserve the
            // existing generation rule; do not untrack the child to obtain Ready.
            match tokio::time::timeout(WAIT, waiting.observe()).await.map_err(failure)? {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {},
                Err(error) => return Err(failure(format!("original wrapper cut failed for another reason: {error}"))),
                Ok(_) => return Err(failure("old wrapper cut accepted a generation changed by original child admission")),
            }
            let child_cut = session.alab_capture_original_session_jobs()?;
            {
                let mut observing = std::pin::pin!(child_cut.observe());
                poll_fn(|cx| match observing.as_mut().poll(cx) {
                    Poll::Pending => Poll::Ready(Ok(())),
                    Poll::Ready(_) => Poll::Ready(Err(failure("fresh same-Book cut settled while original discovery response was held"))),
                }).await?;
            }
            if result_rx.try_recv().is_ok() || commands.try_recv().is_ok() { return Err(failure("pending original discovery completed or was repeated before its one response")); }
            if outcome == "success" {
                command.response.send(Ok(Ok(serde_json::json!({"data":[{"name":"owned_popup_feature","stage":"beta","displayName":null,"description":null,"announcement":null,"enabled":true,"defaultEnabled":false}],"nextCursor":null})))).map_err(|_|failure("original popup response owner lost"))?;
            } else {
                command.response.send(Err(failure("same original popup discovery transport error"))).map_err(|_|failure("original popup error response owner lost"))?;
            }
            // Same child/capture after canceled observation; no second App fetch.
            let ready = tokio::time::timeout(WAIT, child_cut.observe()).await.map_err(failure)??;
            ready.current(&session)?;
            match (outcome, result_rx.try_recv().map_err(failure)?) {
                ("success", Ok(features)) if features.len() == 1 && features[0].name == "owned_popup_feature" && features[0].enabled => {},
                ("rpc_error", Err(error)) if error == "Experimental feature request failed" => {},
                _ => return Err(failure("joined original discovery lost the same popup result or exact error")),
            }
            if commands.try_recv().is_ok() { return Err(failure("original popup discovery was submitted more than once")); }
            Ok::<_, io::Error>(())
        }.await;
        // Release our own lock token, then always service ordinary physical
        // transport shutdown and join this same Session's actual accepted work.
        drop(held.take());
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
                        .send(Err(failure("owned popup fixture cleanup")));
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

#[tokio::test(flavor = "current_thread")]
async fn actual_feature_fetch_shutdown_waits_for_popup_with_original_lock_held() -> io::Result<()> {
    let (app, _events, _ops) =
        crate::boxed_future!(crate::app::tests::make_test_app_with_channels()).await;
    let (client, mut commands, backend_events) = CloudClient::channel(2);
    let session = AppServerSession::new(
        crate::backend_id::BackendId::new(),
        AppServerClient::Cloud(client),
        ThreadParamsMode::Remote,
    );
    // A non-owning view is only a public SAME-Book probe. It never shuts down
    // another connection or supplies a physical-owner Ready witness.
    let probe = session.request_view();
    let mut held = Some(app.feature_write_lock.clone().lock_owned().await);
    let baseline_lock_refs = std::sync::Arc::strong_count(&app.feature_write_lock);
    let baseline = async {
        let cut = probe.alab_capture_original_session_jobs()?;
        let ready = tokio::time::timeout(WAIT, cut.observe())
            .await
            .map_err(failure)??;
        ready.current(&session)
    }
    .await;
    let (response, receiver) = tokio::sync::oneshot::channel();
    let mut popup = Some(receiver);
    if baseline.is_ok() {
        app.fetch_experimental_features(&session, ThreadId::new(), response);
    } else {
        drop(response);
    }
    let cut = probe.alab_capture_original_session_jobs();
    // Construct once, drive this exact consuming future, and never poll again
    // after it returns Ready. Even setup/semantic errors reach the cleanup below.
    let mut shutdown = Box::pin(session.shutdown());
    let mut shutdown_finished = false;
    let body = async {
        baseline?;
        let cut = cut?;
        let mut changes = cut.changes();
        let physical = tokio::time::timeout(WAIT, async {
            tokio::select! {
                result = shutdown.as_mut() => {
                    shutdown_finished = true;
                    Err(failure(format!("shutdown completed before its physical command was answered: {result:?}")))
                }
                command = commands.recv() => command.ok_or_else(|| failure("actual Cloud Shutdown command absent")),
            }
        }).await.map_err(failure)??;
        if !matches!(&physical.operation, CloudOperation::Shutdown) {
            return Err(failure("fetch issued discovery while the actual feature lock was held"));
        }
        // shutdown_actual closes the work Book BEFORE awaiting physical close.
        // Consume/mark this first notification before replying; it alone would
        // leave a false oracle caused by CloudClient's unanswered transport wait.
        tokio::time::timeout(WAIT, changes.changed()).await.map_err(failure)?.map_err(failure)?;
        drop(changes.borrow_and_update());
        if probe.alab_capture_original_session_jobs().is_ok() {
            return Err(failure("original physical shutdown did not close the SAME work Book"));
        }
        physical.response.send(Ok(Ok(serde_json::Value::Null)))
            .map_err(|_| failure("actual Cloud Shutdown response owner lost"))?;
        // The SECOND notification is work.drain()->close(), reached only after
        // physical close and the fresh Session's empty model Book have drained.
        // No accepted root can finish/admit children while this lock and popup
        // remain held, and the test registers no other work in this fresh Book.
        tokio::time::timeout(WAIT, changes.changed()).await.map_err(failure)?.map_err(failure)?;
        if probe.alab_capture_original_session_jobs().is_ok() {
            return Err(failure("drain-stage work Book unexpectedly reopened"));
        }
        let completed = poll_fn(|cx| Poll::Ready(match shutdown.as_mut().poll(cx) {
            Poll::Pending => None,
            Poll::Ready(result) => Some(result),
        })).await;
        if let Some(result) = completed {
            shutdown_finished = true;
            return Err(failure(format!("shutdown settled while its accepted fetch root still waited for the held lock/popup: {result:?}")));
        }
        if popup.is_none() || held.is_none() || app.feature_write_lock.try_lock().is_ok() {
            return Err(failure("pending shutdown lost the original popup or actual held lock"));
        }
        if commands.try_recv().is_ok() {
            return Err(failure("pending shutdown issued another physical command or discovery RPC"));
        }
        // Close ONLY the original popup. The SAME shutdown must now join the
        // accepted root, while the real lock is still held and no RPC can start.
        drop(popup.take());
        let result = tokio::time::timeout(WAIT, shutdown.as_mut()).await.map_err(failure)?;
        shutdown_finished = true;
        result?;
        if held.is_none() || app.feature_write_lock.try_lock().is_ok() {
            return Err(failure("shutdown required releasing the actual feature lock"));
        }
        if commands.try_recv().is_ok() {
            return Err(failure("closed popup admitted discovery or repeated physical shutdown"));
        }
        Ok::<_, io::Error>(())
    }.await;

    // On every explicit Result path, close the popup first and retain the held
    // lock until the wrapper drops its real lock Arc. This cleanup witness also
    // covers the old UNTRACKED wrapper after the intended premature-Ready failure;
    // it is not a replacement for the shutdown/Book assertions above.
    drop(popup.take());
    let wrapper_retired = tokio::time::timeout(WAIT, async {
        loop {
            match std::sync::Arc::strong_count(&app.feature_write_lock) {
                count if count == baseline_lock_refs => return Ok::<_, io::Error>(()),
                count if count < baseline_lock_refs => {
                    return Err(failure(
                        "owned lock references changed during wrapper cleanup",
                    ));
                }
                _ => tokio::task::yield_now().await,
            }
        }
    })
    .await
    .map_err(failure)
    .and_then(|result| result);
    drop(held.take());
    let cleanup = if shutdown_finished {
        Ok(())
    } else {
        tokio::time::timeout(WAIT, async {
            loop {
                tokio::select! {
                    result = shutdown.as_mut() => return result,
                    command = commands.recv() => {
                        let Some(command) = command else {
                            return Err(failure("original Cloud transport closed before shutdown joined"));
                        };
                        let reply = if matches!(&command.operation, CloudOperation::Shutdown) {
                            Ok(Ok(serde_json::Value::Null))
                        } else {
                            Err(failure("owned pending-fetch shutdown cleanup"))
                        };
                        let _sent = command.response.send(reply);
                    }
                }
            }
        }).await.map_err(failure).and_then(|result| result)
    };
    drop(backend_events);
    body?;
    wrapper_retired?;
    cleanup?;
    Ok(())
}
