// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Model polling follows live experience changes, including an already sleeping worker.

use super::*;

pub(super) fn spawn(model_catalog: &Arc<ModelCatalog>) -> ModelsRefreshWorker {
    spawn_with_mode(
        model_catalog,
        codex_alab_core::experience_changes(),
        codex_alab_core::MODELS_REFRESH_INTERVAL,
        MODELS_REFRESH_INTERVAL,
    )
}

pub(super) fn spawn_with_mode(
    model_catalog: &Arc<ModelCatalog>,
    mut mode: tokio::sync::watch::Receiver<bool>,
    enhanced_interval: Duration,
    vanilla_interval: Duration,
) -> ModelsRefreshWorker {
    let model_catalog = Arc::downgrade(model_catalog);
    let shutdown = CancellationToken::new();
    let worker_shutdown = shutdown.clone();
    let task = tokio::spawn(async move {
        loop {
            if worker_shutdown.is_cancelled() {
                break;
            }
            let enhanced = *mode.borrow_and_update();
            let Some(model_catalog) = model_catalog.upgrade() else {
                break;
            };
            let strategy = if enhanced {
                RefreshStrategy::OnlineIfUncached
            } else {
                RefreshStrategy::Online
            };
            if let Err(err) = model_catalog.list_models(strategy).await {
                tracing::warn!(error_kind = ?err.kind(), "model catalog refresh blocked by provider requirements");
            }
            drop(model_catalog);
            let interval = if enhanced {
                enhanced_interval
            } else {
                vanilla_interval
            };
            tokio::select! {
                _ = worker_shutdown.cancelled() => break,
                result = mode.changed() => if result.is_err() { break; },
                _ = tokio::time::sleep(interval) => {}
            }
        }
    });
    ModelsRefreshWorker {
        shutdown,
        _task: task,
    }
}
