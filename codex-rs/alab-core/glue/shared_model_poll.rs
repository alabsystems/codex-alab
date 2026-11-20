// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

use codex_alab_core::home::ModelCacheRevision;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

const PEER_RETRY_INTERVAL: Duration = Duration::from_secs(30);

impl ModelCatalog {
    /// A shared file is only a hint. Confirm it separately for each serving manager.
    pub(crate) async fn alab_confirm_peer_catalogs(
        &self,
        revisions: &mut HashMap<PathBuf, ModelCacheRevision>,
    ) {
        if !codex_alab_core::active() {
            return;
        }
        let mut threads_by_file = HashMap::<PathBuf, Vec<Arc<codex_core::CodexThread>>>::new();
        for id in self.thread_manager.list_thread_ids().await {
            if let Ok(thread) = self.thread_manager.get_thread(id).await
                && let Some(file) = thread.alab_peer_model_cache_file()
            {
                threads_by_file
                    .entry(file.to_owned())
                    .or_default()
                    .push(thread);
            }
        }
        revisions.retain(|file, _| threads_by_file.contains_key(file));
        for (file, threads) in threads_by_file {
            let revision = revisions.entry(file.clone()).or_default();
            if !revision.changed(&file).await {
                continue;
            }
            // Credentials can differ even for managers using the same disk file.
            let mut checked = Vec::new();
            for thread in threads {
                let Some((config, manager)) = thread.alab_followed_catalog().await else {
                    continue;
                };
                if checked.iter().any(|seen| Arc::ptr_eq(seen, &manager)) {
                    continue;
                }
                match self.confirm_peer_file(&config, &manager, revision).await {
                    Ok(()) => checked.push(manager),
                    Err(err) => {
                        // A policy change can restore this route without another cache write.
                        // Other threads sharing a manager may have a different retained config.
                        revision.retry_after(PEER_RETRY_INTERVAL);
                        tracing::warn!(error_kind = ?err.kind(), "peer model catalog refresh blocked by provider requirements");
                    }
                }
            }
        }
    }

    async fn confirm_peer_file(
        &self,
        config: &Arc<Config>,
        manager: &SharedModelsManager,
        revision: &mut ModelCacheRevision,
    ) -> std::io::Result<()> {
        let cached = self
            .list_models_from_snapshot(
                Arc::clone(config),
                Arc::clone(manager),
                RefreshStrategy::Offline,
            )
            .await?;
        self.confirm_cached_successor(config, manager, cached, Some(revision))
            .await?;
        Ok(())
    }

    pub(super) async fn confirm_followed_models(
        &self,
        config: &Arc<Config>,
        manager: &SharedModelsManager,
        strategy: RefreshStrategy,
        models: Vec<ModelPreset>,
    ) -> std::io::Result<Vec<ModelPreset>> {
        if !codex_alab_core::active() || strategy != RefreshStrategy::OnlineIfUncached {
            return Ok(models);
        }
        self.confirm_cached_successor(config, manager, models, None)
            .await
    }

    async fn confirm_cached_successor(
        &self,
        config: &Arc<Config>,
        manager: &SharedModelsManager,
        models: Vec<ModelPreset>,
        revision: Option<&mut ModelCacheRevision>,
    ) -> std::io::Result<Vec<ModelPreset>> {
        let Some(mut publication) = manager.model_catalog_updates() else {
            return Ok(models);
        };
        if models.is_empty() {
            return Ok(models);
        }
        for id in self.thread_manager.list_thread_ids().await {
            if let Ok(thread) = self.thread_manager.get_thread(id).await
                && thread.alab_needs_model_confirmation(manager, &models).await
            {
                // Recheck provider policy and reuse the same manager for the
                // single online confirmation shared by all its loaded threads.
                publication.borrow_and_update();
                let listed = self
                    .list_models_from_snapshot(
                        Arc::clone(config),
                        Arc::clone(manager),
                        RefreshStrategy::Online,
                    )
                    .await;
                if let Some(revision) = revision
                    && matches!(publication.has_changed(), Ok(false))
                {
                    // Polling must not query an unavailable provider every five seconds.
                    revision.retry_after(PEER_RETRY_INTERVAL);
                }
                return listed;
            }
        }
        Ok(models)
    }
}
