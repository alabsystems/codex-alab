// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual Worker Drop only; no injection completion or Native acceptance is faked.
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
        let job = Arc::new(SideInjection {
            scope: Arc::downgrade(&fixture.scope),
            input: fixture.h.input.clone(),
            thread: fixture.h.parent.clone(),
            prepared: fixture.h.parent.prepare_retained_side_boundary()?,
            parameters: serde_json::json!({}),
            arguments: ThreadInjectItemsParams {
                thread_id: fixture.h.parent_id.to_string(),
                items: Vec::new(),
            },
            claim: "lower-guard-fixture".into(),
            progress: Mutex::new(Progress::default()),
            changed: watch::channel(()).0,
        });
        let mut worker = Worker {
            actual: job.clone(),
            finished: false,
            _tracked: fixture.operation.tracker.token(),
        };
        let (entered, started) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let (finished, completed) = oneshot::channel();
        let work = async move {
            let actual = worker.actual.clone();
            let _ = entered.send(());
            let _ = released.await;
            if mode == Mode::EarlyReturn {
                return;
            }
            actual.changed.send_replace(());
            // Same production order: finish BEFORE awaiting observer publication.
            worker.finish();
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
        ensure!(
            job.progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .unknown
                == lost
        );
        ensure!(Arc::ptr_eq(&job.thread, &fixture.h.parent));
        fixture.assert_input(lost, lost)
    }
    .await;
    let cleanup = fixture.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn actual_side_guard_injection_finishes_before_observer_wait_and_keeps_cancelled_unknown()
-> Result<()> {
    let mut failures = Vec::new();
    for mode in Mode::ALL {
        if let Err(error) = one(mode).await {
            failures.push(format!("{mode:?}: {error:#}"));
        }
    }
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
