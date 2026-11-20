// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual App save -> original discovery/write/readback -> original result owners.
//! CloudClient::channel is an owned typed command transport, not a Native grant.
use crate::app::AppEvent;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudOperation;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_protocol::ThreadId;
use std::collections::VecDeque;
use std::future::Future;
use std::future::poll_fn;
use std::io;
use std::task::Poll;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(60);
const OVERRIDDEN: &str = "Changes were saved, but the configured values differ from your selections. A higher-priority setting may override them.";
const WRITE_ERROR: &str = "Failed to save experimental features. Reopen /experimental to check configured values before retrying.";
fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn features(enabled: bool) -> serde_json::Value {
    serde_json::json!({"data":[{"name":"owned_feature","stage":"beta","displayName":null,"description":null,"announcement":null,"enabled":enabled,"defaultEnabled":false}],"nextCursor":null})
}

#[tokio::test]
async fn actual_feature_save_cut_retains_one_write_and_original_result_after_cancelled_observation()
-> io::Result<()> {
    for popup_open in [true, false] {
        let (mut app, mut events, _ops) =
            crate::boxed_future!(crate::app::tests::make_test_app_with_channels()).await;
        let (client, mut commands, backend_events) = CloudClient::channel(4);
        let session = AppServerSession::new(
            crate::backend_id::BackendId::new(),
            AppServerClient::Cloud(client),
            ThreadParamsMode::Remote,
        );
        let thread = ThreadId::new();
        let original_model = app.chat_widget.config_ref().model.clone();
        let original_feature_setting = app.config.features.clone();
        let mut original_fifo = VecDeque::new();
        while let Ok(event) = events.try_recv() {
            original_fifo.push_back(event);
        }
        let original_boundary = original_fifo.len();
        let body = async {
            // This is the SAME new actual Session's empty, joined Book, not an
            // unrelated pending task that could make old untracked code Pending.
            let baseline = session.alab_capture_original_session_jobs()?;
            let baseline_ready = tokio::time::timeout(WAIT, baseline.observe()).await.map_err(failure)??;
            baseline_ready.current(&session)?;
            let (result_tx, result_rx) = tokio::sync::oneshot::channel();
            let mut original_result = Some(result_rx);
            app.save_experimental_features(&session, thread, vec![("owned_feature".into(), true)], result_tx);
            if !popup_open { drop(original_result.take()); }
            let first = tokio::time::timeout(WAIT, commands.recv()).await.map_err(failure)?.ok_or_else(||failure("original feature discovery absent"))?;
            match &first.operation {
                CloudOperation::Request(request) => match request.as_ref() {
                    ClientRequest::ExperimentalFeatureList { request_id, params }
                        if request_id == &RequestId::String("tui-experimental-save-readback".into())
                        && params.thread_id.as_deref() == Some(thread.to_string().as_str())
                        && params.cursor.is_none() && params.limit == Some(100) => {},
                    _ => return Err(failure("actual save changed its original pre-write discovery")),
                },
                _ => return Err(failure("actual save did not issue discovery through the real client")),
            }
            first.response.send(Ok(Ok(features(false)))).map_err(|_|failure("original discovery response owner lost"))?;
            let write = tokio::time::timeout(WAIT, commands.recv()).await.map_err(failure)?.ok_or_else(||failure("original config write absent"))?;
            let original_request = match &write.operation {
                CloudOperation::Request(request) => match request.as_ref() {
                    ClientRequest::ConfigBatchWrite { request_id, params }
                        if request_id == &RequestId::String("tui-experimental-feature-write".into())
                        && params.edits.len() == 1
                        && params.edits[0].key_path == "features.\"owned_feature\""
                        && params.edits[0].value == serde_json::json!(true)
                        && params.reload_user_config && params.file_path.is_none()
                        && params.expected_version.is_none() => request_id.clone(),
                    _ => return Err(failure("actual save changed its original typed config write")),
                },
                _ => return Err(failure("actual save did not issue a config write through the real client")),
            };
            if app.feature_write_lock.try_lock().is_ok() { return Err(failure("actual feature save released its guard before the held write response")); }
            let cut = session.alab_capture_original_session_jobs()?;
            {
                let mut observing = std::pin::pin!(cut.observe());
                poll_fn(|cx| match observing.as_mut().poll(cx) {
                    Poll::Pending => Poll::Ready(Ok(())),
                    Poll::Ready(_) => Poll::Ready(Err(failure("feature cut settled before original write/readback/result publication"))),
                }).await?;
            }
            if app.feature_write_lock.try_lock().is_ok() { return Err(failure("cancelled join observation released the original save guard")); }
            if commands.try_recv().is_ok() { return Err(failure("cancelled join observation repeated the original write")); }
            let expected_warning = if popup_open {
                write.response.send(Ok(Ok(serde_json::json!({"status":"ok","version":"same-feature-version","filePath":app.config.codex_home.join("config.toml"),"overriddenMetadata":null})))).map_err(|_|failure("original write response owner lost"))?;
                let readback = tokio::time::timeout(WAIT, commands.recv()).await.map_err(failure)?.ok_or_else(||failure("original post-write readback absent"))?;
                match &readback.operation {
                    CloudOperation::Request(request) => match request.as_ref() {
                        ClientRequest::ExperimentalFeatureList { request_id, params }
                            if request_id == &RequestId::String("tui-experimental-save-readback".into())
                            && params.thread_id.as_deref() == Some(thread.to_string().as_str())
                            && params.cursor.is_none() && params.limit == Some(100) => {},
                        _ => return Err(failure("actual save changed its original readback")),
                    },
                    _ => return Err(failure("actual save did not read back through the real client")),
                }
                // A real successful write can still be overridden by configured
                // readback; preserve that warning rather than applying a choice.
                readback.response.send(Ok(Ok(features(false)))).map_err(|_|failure("original readback response owner lost"))?;
                OVERRIDDEN
            } else {
                // Closing the popup does not cancel this accepted write. The
                // original unresolved outcome must still reach App history.
                write.response.send(Err(failure("same feature write transport failure"))).map_err(|_|failure("original write failure response owner lost"))?;
                WRITE_ERROR
            };
            let observed = tokio::time::timeout(WAIT, cut.observe()).await.map_err(failure)?;
            if popup_open {
                // The successful write creates its real post-write discovery
                // child AFTER the held-write cut. Its SAME Book must reject that
                // stale generation, even though the original parent was joined.
                match observed {
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {},
                    Err(error) => return Err(failure(format!("old feature cut failed for another reason: {error}"))),
                    Ok(_) => return Err(failure("old feature cut accepted a generation changed by original readback")),
                }
                // Capture the settled generation of the SAME Book. The joined
                // parent can no longer submit another child. No save, write or
                // discovery is resubmitted by this test to obtain a fresh cut.
                let settled = session.alab_capture_original_session_jobs()?;
                let ready = tokio::time::timeout(WAIT, settled.observe()).await.map_err(failure)??;
                ready.current(&session)?;
            } else {
                // A failed write returns before post-write fetch, so this exact
                // held-write cut remains valid after joining its original jobs.
                let ready = observed?;
                ready.current(&session)?;
            }
            if app.feature_write_lock.try_lock().is_err() { return Err(failure("joined original save still holds its actual write guard")); }
            while let Ok(event) = events.try_recv() { original_fifo.push_back(event); }
            let notices: Vec<_> = original_fifo.iter().skip(original_boundary).filter_map(|event| {
                if let AppEvent::InsertHistoryCell(cell) = event {
                    Some(cell.warning_entries())
                } else { None }
            }).collect();
            // Ordinary warnings retain their complete diagnostic even when the
            // transcript hides their rendered lines. Check the actual queued
            // warning owner, independently of that presentation policy.
            let expected_entry = crate::history_cell::WarningEntry {
                id: crate::history_cell::WarningId::Message(expected_warning.into()),
                source: "Warning".into(),
                details: expected_warning.into(),
            };
            if notices != vec![vec![expected_entry]] { return Err(failure("joined original save did not queue its one actual warning")); }
            if let Some(mut receiver) = original_result {
                let result = receiver.try_recv().map_err(failure)?.map_err(failure)?;
                if result.warning.as_deref() != Some(OVERRIDDEN) || result.features.len() != 1 || result.features[0].name != "owned_feature" || result.features[0].enabled { return Err(failure("joined save lost the original popup result/readback")); }
            }
            if commands.try_recv().is_ok() { return Err(failure(format!("write {original_request:?} repeated after its completion"))); }
            if app.chat_widget.config_ref().model != original_model || app.config.features != original_feature_setting { return Err(failure("configured feature save changed the running task's settings")); }
            Ok::<_, io::Error>(())
        }.await;
        // Every explicit semantic failure follows the same ordinary cleanup.
        // Do not cancel, retry or substitute the actual accepted save future.
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
                        .send(Err(failure("owned feature fixture cleanup")));
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
