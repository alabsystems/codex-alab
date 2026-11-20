// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Lower owner lifetime controls, not Native admission or successful Side effects.
use super::*;
use crate::message_processor::alab_task_mcp::tests::Harness;
use crate::outgoing_message::ConnectionId;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use std::future::Future;
use std::time::Duration;
use tokio::sync::oneshot;
use wiremock::MockServer;

// Match the existing publication fixture's hang detector; no latency budget.
const HANG: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Unpolled,
    Pending,
    EarlyReturn,
    Finished,
    FinishedObserverPending,
}
impl Mode {
    pub(super) const ALL: [Self; 5] = [
        Self::Unpolled,
        Self::Pending,
        Self::EarlyReturn,
        Self::Finished,
        Self::FinishedObserverPending,
    ];
    pub(super) fn finished(self) -> bool {
        matches!(self, Self::Finished | Self::FinishedObserverPending)
    }
}

pub(super) struct Fixture {
    pub(super) h: Harness,
    pub(super) operation: Arc<InitialSessionOperation>,
    pub(super) scope: Arc<Scope>,
    model: MockServer,
}
impl Fixture {
    pub(super) async fn new() -> Result<Self> {
        let model = MockServer::start().await;
        let h = Harness::new(&model.uri()).await?;
        Self::from_harness(model, h).await
    }

    pub(super) async fn from_harness(model: MockServer, h: Harness) -> Result<Self> {
        let prepared = async {
            let (context, _, processor) = super::publication_tests::dispatched_context_for(
                &h,
                ConnectionId(41),
                &h.c1,
                9191,
                h.parent_id,
            )
            .await?;
            ensure!(context.alab_connection_resource.is_none());
            super::publication_tests::publication_fixture(&h, processor, context, &h.c1)
        }
        .await;
        match prepared {
            Ok((operation, scope)) => Ok(Self {
                h,
                operation,
                scope,
                model,
            }),
            Err(error) => {
                let cleanup = h.shutdown().await;
                cleanup.context("guard fixture setup cleanup")?;
                Err(error)
            }
        }
    }

    pub(super) fn assert_input(&self, lost: bool, selection_unknown: bool) -> Result<()> {
        ensure!(self.h.input.is_stopped() == lost);
        ensure!(self.h.input.failure().is_some() == lost);
        ensure!(self.scope.input.is_stopped() == lost);
        ensure!(
            self.scope
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .selection_unknown
                == selection_unknown
        );
        Ok(())
    }

    pub(super) async fn shutdown(self) -> Result<()> {
        let Self {
            h,
            operation,
            scope,
            model,
        } = self;
        operation.tracker.close();
        let drained = tokio::time::timeout(HANG, operation.tracker.wait())
            .await
            .context("owned guard token survived fixture cleanup");
        drop((scope, operation));
        let cleanup = h.shutdown().await;
        drop(model);
        drained?;
        cleanup
    }
}

/// Poll only the supplied actual-owner future; never launch a Source operation.
/// Its capture is formed at the caller, using that guard's original field accesses.
pub(super) async fn retire<F: Future<Output = ()>>(
    mode: Mode,
    future: F,
    entered: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
    finished: oneshot::Receiver<()>,
    tracker: &TaskTracker,
) -> Result<()> {
    let mut future = Box::pin(future);
    ensure!(
        tracker.len() == 1,
        "owner token missing before the first poll"
    );
    if mode != Mode::Unpolled {
        tokio::time::timeout(HANG, async {
            tokio::select! {
                () = &mut future => anyhow::bail!("owner returned before its pending barrier"),
                result = entered => result.context("owner did not enter its pending barrier"),
            }
        })
        .await??;
        ensure!(
            tracker.len() == 1,
            "owner retired while its future was pending"
        );
        if mode != Mode::Pending {
            release
                .send(())
                .map_err(|_| anyhow::anyhow!("owner lost its release receiver"))?;
            if mode == Mode::FinishedObserverPending {
                tokio::time::timeout(HANG, async {
                    tokio::select! {
                        () = &mut future => anyhow::bail!("owner skipped its observer barrier"),
                        result = finished => result.context("owner did not mark completion"),
                    }
                })
                .await??;
                ensure!(
                    tracker.len() == 1,
                    "completed owner lost observer-wait custody"
                );
            } else {
                tokio::time::timeout(HANG, &mut future).await?;
                if mode == Mode::Finished {
                    ensure!(
                        tracker.is_empty(),
                        "completed owner retained its tracker token after Poll::Ready"
                    );
                }
            }
        }
    }
    drop(future);
    tracker.close();
    tokio::time::timeout(HANG, tracker.wait())
        .await
        .context("owner token survived its future's destruction")?;
    ensure!(tracker.is_empty());
    Ok(())
}
