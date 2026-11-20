// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual private close Guard Drop; target DATA grants no installed Side authority.
use super::super::side_guard_test_fixture::Fixture;
use super::super::side_guard_test_fixture::Mode;
use super::super::side_guard_test_fixture::retire;
use super::*;
use anyhow::Result;
use anyhow::ensure;
use tokio::sync::oneshot;

async fn one(mode: Mode) -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = async {
        fixture.assert_input(false, false)?;
        let job = Arc::new(Closing {
            actual: fixture.h.parent.clone(),
            target: CloseTarget::Installed,
            parameters: serde_json::json!({}),
            args: ThreadUnsubscribeParams {
                thread_id: fixture.h.parent_id.to_string(),
            },
            progress: Mutex::new(Progress::default()),
        });
        let mut guard = Guard {
            actual: job.clone(),
            scope: fixture.scope.clone(),
            finished: false,
            _tracked: fixture.operation.tracker.token(),
        };
        let (entered, started) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let (finished, completed) = oneshot::channel();
        let work = async move {
            let job = guard.actual.clone();
            let _ = entered.send(());
            let _ = released.await;
            if mode == Mode::EarlyReturn {
                return;
            }
            job.progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .joined = true;
            guard.finish();
            let _ = finished.send(());
            if mode == Mode::FinishedObserverPending {
                std::future::pending::<()>().await;
            }
        };
        retire(
            mode,
            work,
            started,
            release,
            completed,
            &fixture.operation.tracker,
        )
        .await?;
        let lost = !mode.finished();
        let progress = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
        ensure!(progress.unknown == lost);
        ensure!(progress.joined == mode.finished());
        drop(progress);
        ensure!(Arc::ptr_eq(&job.actual, &fixture.h.parent));
        // This Drop poisons input, not the initial selection's unknown bit.
        fixture.assert_input(lost, false)
    }
    .await;
    let cleanup = fixture.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn actual_side_guard_close_preserves_finished_and_cancelled_owner_lifetimes() -> Result<()> {
    let mut failures = Vec::new();
    for mode in Mode::ALL {
        if let Err(error) = one(mode).await {
            failures.push(format!("{mode:?}: {error:#}"));
        }
    }
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[tokio::test]
async fn side_close_readback_uses_only_the_exact_existing_result() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = (|| -> Result<()> {
        let view = metadata::SideView {
            schema: 1,
            claim: "a".repeat(32),
            thread_id: fixture.h.parent_id.to_string(),
        };
        let operation = "b".repeat(32);
        let parameters =
            serde_json::json!({"threadId":view.thread_id,"_meta":{"foreign":[1,null]}});
        let read = |params: &serde_json::Value| {
            fixture
                .operation
                .read_side_close_result(&view, &operation, params)
        };
        ensure!(read(&parameters).is_err());
        fixture
            .operation
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clients
            .insert(view.claim.clone(), fixture.scope.clone());
        ensure!(read(&parameters).is_err());
        let job = Arc::new(Closing {
            actual: fixture.h.parent.clone(),
            target: CloseTarget::Installed,
            parameters: parameters.clone(),
            args: ThreadUnsubscribeParams {
                thread_id: view.thread_id.clone(),
            },
            progress: Mutex::new(Progress::default()),
        });
        {
            let mut held = fixture
                .scope
                .side_closings
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            ensure!(held.jobs.is_empty());
            held.jobs.insert(operation.clone(), job.clone());
        }
        ensure!(read(&parameters).is_err());
        let reply = codex_app_server_protocol::ThreadUnsubscribeResponse {
            status: codex_app_server_protocol::ThreadUnsubscribeStatus::NotSubscribed,
        };
        job.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .result = Some(Ok(reply));
        ensure!(read(&serde_json::json!({"threadId":view.thread_id})).is_err());
        let recovered = serde_json::to_value(
            read(&parameters).map_err(|error| anyhow::anyhow!(error.message))?,
        )?;
        ensure!(
            recovered["reply"] == serde_json::json!({"kind":"success", "status":"notSubscribed"})
        );
        for data in [
            None,
            Some(serde_json::Value::Null),
            Some(serde_json::json!({"detail":1})),
        ] {
            let error = JSONRPCErrorError {
                code: -7,
                message: "original rejection".into(),
                data: data.clone(),
            };
            job.progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .result = Some(Err(error));
            let recovered = serde_json::to_value(
                read(&parameters).map_err(|error| anyhow::anyhow!(error.message))?,
            )?;
            ensure!(
                recovered["reply"]
                    == serde_json::json!({"kind":"rejected","code":-7,"message":"original rejection","hadData":data.is_some(),"value":data})
            );
        }
        {
            let mut progress = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
            ensure!(!progress.observed && !progress.joined && progress.observers.is_empty());
            progress.unknown = true;
        }
        ensure!(read(&parameters).is_err());
        let held = fixture
            .scope
            .side_closings
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        ensure!(held.jobs.len() == 1 && Arc::ptr_eq(&held.jobs[&operation], &job));
        ensure!(fixture.operation.tracker.is_empty());
        Ok(())
    })();
    let cleanup = fixture.shutdown().await;
    result.and(cleanup)
}
