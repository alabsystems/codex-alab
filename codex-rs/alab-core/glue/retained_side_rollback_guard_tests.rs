// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual private rollback Guard Drop; no delete/admission result is manufactured.
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
        let job = Arc::new(Rollback {
            actual: fixture.h.parent.clone(),
            id: fixture.h.parent_id,
            operation: "lower-guard-fixture".into(),
            parameters: serde_json::json!({}),
            args: ThreadDeleteParams {
                thread_id: fixture.h.parent_id.to_string(),
            },
            progress: Mutex::new(Progress::default()),
        });
        let mut guard = Guard {
            job: job.clone(),
            scope: fixture.scope.clone(),
            finished: false,
            _tracked: fixture.operation.tracker.token(),
        };
        let (entered, started) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let (finished, completed) = oneshot::channel();
        // Exercise the production capture and completion path on the actual owner.
        let work = async move {
            let job = guard.job.clone();
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
        fixture.assert_input(lost, lost)
    }
    .await;
    let cleanup = fixture.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn actual_side_guard_rollback_preserves_finished_and_cancelled_owner_lifetimes() -> Result<()>
{
    let mut failures = Vec::new();
    for mode in Mode::ALL {
        if let Err(error) = one(mode).await {
            failures.push(format!("{mode:?}: {error:#}"));
        }
    }
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
