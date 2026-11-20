// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Apply a newly available successor to the settings a conversation uses for future turns.
use super::Session;
use super::SessionSettingsUpdate;
use super::StepSettings;
use super::thread_settings;
use crate::config::Config;
use crate::session::SessionConfiguration;
use crate::session::step_settings::ResolvedStepSettings;
use codex_alab_core::home::ModelCacheRevision;
use codex_models_manager::ModelOrigin;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::RefreshedModelCatalog;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::error::CodexErr;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::future::Shared;
use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::watch;

impl SessionConfiguration {
    fn alab_follows_serving_catalog_preference(&self) -> bool {
        let config = &self.original_config_do_not_use;
        config.alab_saved_model_can_follow.is_some()
            && config.alab_follows_saved_model()
            && (config.alab_uses_sol_default()
                || config.alab_follows_explicit_api_catalog()
                || (config.model_provider.model_catalog_url.is_some()
                    && config.model_catalog.is_none()))
            && self.parent_thread_id.is_none()
            && !self.session_source.is_non_root_agent()
            && !crate::guardian::is_basic_session_source(&self.session_source)
    }

    /// Normalization of durable preferences still applies to service tiers. A
    /// temporary restriction on a followed model's effort belongs to the turn.
    pub(in crate::session) fn alab_normalize_catalog(&mut self, catalog: &[ModelInfo]) {
        let saved = self
            .alab_follows_serving_catalog_preference()
            .then(|| self.step_settings.collaboration_mode.reasoning_effort())
            .flatten();
        super::normalize(&mut self.step_settings, catalog);
        if let Some(saved) = saved
            && self
                .step_settings
                .collaboration_mode
                .reasoning_effort()
                .as_ref()
                != Some(&saved)
        {
            Arc::make_mut(&mut self.step_settings)
                .collaboration_mode
                .settings
                .reasoning_effort = Some(saved);
        }
    }

    /// When a previously confirmed restriction rules out the saved selection,
    /// recheck with this manager's credential before making any model request.
    pub(in crate::session) async fn alab_resolve_followed_turn(
        &mut self,
        manager: &SharedModelsManager,
    ) -> Result<(), CodexErr> {
        if !self.alab_follows_serving_catalog_preference() {
            return Ok(());
        }
        let Some(catalog) = manager.refreshed_model_catalog() else {
            return Ok(());
        };
        let mode = &self.step_settings.collaboration_mode;
        let requested = mode.reasoning_effort();
        let current = mode.model();
        if catalog.iter().any(|row| {
            row.slug == current
                && requested.as_ref().is_none_or(|effort| {
                    row.supported_reasoning_levels
                        .iter()
                        .any(|level| &level.effort == effort)
                })
        }) {
            return Ok(());
        }
        let config = &self.original_config_do_not_use;
        manager
            .list_models(RefreshStrategy::Online, config.http_client_factory())
            .await;
        let unavailable = || {
            let effort = requested
                .as_ref()
                .map(|value| format!(" at {value} effort"))
                .unwrap_or_default();
            CodexErr::InvalidRequest(format!(
                "The serving account currently offers no eligible model for this thread's saved {current}{effort}. Retry when its catalog changes, or choose another model explicitly."
            ))
        };
        let confirmed = manager.refreshed_model_catalog().ok_or_else(unavailable)?;
        let listed = manager.build_available_models(confirmed.to_vec());
        if let Some(index) = config.alab_catalog_successor(&listed, current, requested.as_ref()) {
            Arc::make_mut(&mut self.step_settings)
                .collaboration_mode
                .settings
                .model = listed[index].model.clone();
        } else {
            let current_row = confirmed
                .iter()
                .find(|row| row.slug == current)
                .ok_or_else(unavailable)?;
            if requested.is_some() && normalized_effort(&self.step_settings, current_row).is_none()
            {
                return Err(unavailable());
            }
        }
        Ok(())
    }
}

/// Derive a temporary, advertised request effort using the same normalization
/// as an ordinary settings update. Keep the selected value in the rollout.
fn normalized_effort(selected: &StepSettings, model: &ModelInfo) -> Option<ReasoningEffort> {
    let mut effective = Arc::new(selected.clone());
    super::normalize(&mut effective, std::slice::from_ref(model));
    effective
        .collaboration_mode
        .reasoning_effort()
        .filter(|effort| {
            model
                .supported_reasoning_levels
                .iter()
                .any(|level| &level.effort == effort)
        })
}

/// The idle actor and the first submitted turn can both observe the same refresh.
struct HandoffCatalogRefresh(Shared<BoxFuture<'static, ()>>);

#[derive(Default)]
struct TurnModelCacheRevision(Mutex<ModelCacheRevision>);

impl Session {
    pub(crate) async fn alab_finish_handoff_model_refresh(&self) {
        if !codex_alab_core::active() {
            return;
        }
        let attachments = &self.services.session_extension_data;
        if let Some(pending) = attachments.get::<HandoffCatalogRefresh>() {
            // The submission actor may be starting this turn. Wait for the provider
            // response itself, then apply it under the ordinary settings permit.
            pending.0.clone().await;
            let _settings_guard = thread_settings::acquire_persistence_lock(self).await;
            if attachments
                .get::<HandoffCatalogRefresh>()
                .is_some_and(|current| Arc::ptr_eq(&current, &pending))
            {
                promote(self).await;
                attachments.remove::<HandoffCatalogRefresh>();
            }
        }

        let Some((config, settings)) = following_root_settings(self).await else {
            return;
        };
        let manager = &self.services.models_manager;
        if config.model_catalog.is_some() {
            return;
        }
        let Some(mut publication) = manager.model_catalog_updates() else {
            return;
        };
        let revision = attachments.get_or_init(TurnModelCacheRevision::default);
        // Concurrent starts must see the same confirmation before choosing their model.
        let mut revision = revision.0.lock().await;
        let Some(cache_file) = manager.alab_peer_cache_file() else {
            return;
        };
        if !revision.changed(cache_file).await {
            return;
        }
        if cached_successor_needs_confirmation(
            &config,
            &settings,
            manager,
            RefreshStrategy::Offline,
        )
        .await
        {
            publication.borrow_and_update();
            manager
                .list_models(RefreshStrategy::Online, config.http_client_factory())
                .await;
            if matches!(publication.has_changed(), Ok(false)) {
                revision.retry_after(Duration::ZERO);
            }
        }
        // The watch actor may still be queued behind this submission. Apply only
        // the manager's credential-validated network publication before dispatch.
        let _settings_guard = thread_settings::acquire_persistence_lock(self).await;
        promote(self).await;
    }

    pub(in crate::session) async fn alab_effort_for_request(
        &self,
        settings: &ResolvedStepSettings,
    ) -> Option<ReasoningEffort> {
        let requested = settings.reasoning_effort()?;
        let model = &settings.model_info;
        // Kimi's packaged catalog explicitly distinguishes a model with no
        // effort parameter from one whose default can be overridden by a profile.
        if model.origin == ModelOrigin::ExplicitConfig
            && model.supported_reasoning_levels.is_empty()
            && model.default_reasoning_level.is_none()
            && self
                .state
                .lock()
                .await
                .session_configuration
                .original_config_do_not_use
                .alab_packaged_kimi_catalog
        {
            return None;
        }
        if model.origin == ModelOrigin::Provider
            && !model
                .supported_reasoning_levels
                .iter()
                .any(|level| &level.effort == requested)
            && self
                .state
                .lock()
                .await
                .session_configuration
                .alab_follows_serving_catalog_preference()
        {
            return normalized_effort(settings.selected(), model);
        }
        Some(requested.clone())
    }
}

impl crate::CodexThread {
    pub fn alab_peer_model_cache_file(&self) -> Option<&std::path::Path> {
        self.session.services.models_manager.alab_peer_cache_file()
    }

    /// Resolve the provider and originator that own this root's model settings.
    pub async fn alab_followed_catalog(&self) -> Option<(Arc<Config>, SharedModelsManager)> {
        let (config, _) = following_root_settings(&self.session).await?;
        config.model_catalog.is_none().then(|| {
            (config, Arc::clone(&self.session.services.models_manager))
        })
    }

    /// Prepare a serving-account refresh for a user reattaching to a loaded root.
    /// The request runs it after releasing the app-server thread-list permit.
    pub async fn alab_refresh_for_model_resume(
        &self,
    ) -> Option<impl Future<Output = ()> + Send + use<>> {
        let (config, _) = following_root_settings(&self.session).await?;
        let manager = Arc::clone(&self.session.services.models_manager);
        if config.model_catalog.is_some() {
            return None;
        }
        let published = manager.model_catalog_updates()?;
        let mut applied = self
            .session
            .services
            .session_extension_data
            .get_or_init(CatalogApplications::new)
            .0
            .subscribe();
        let stopped = self.io.session_loop_termination.clone();
        Some(async move {
            let refresh = async {
                manager
                    .list_models(RefreshStrategy::Online, config.http_client_factory())
                    .await;
                wait_for_catalog_application(&mut applied, || {
                    manager.refreshed_model_catalog()?;
                    published.borrow().clone()
                })
                .await;
            };
            // The real submission loop owns settings and its writer. A stopped
            // Core yields to the ordinary resume lookup even during network I/O.
            tokio::select! {
                biased;
                () = stopped => {},
                () = refresh => {},
            }
        })
    }

    /// A second tab can write a newer catalog without waking this session's
    /// provider watch. A cache hit may request confirmation from this thread's
    /// own provider. Only the normal successful network response changes settings.
    pub async fn alab_needs_model_confirmation(
        &self,
        manager: &SharedModelsManager,
        listed: &[ModelPreset],
    ) -> bool {
        if !Arc::ptr_eq(manager, &self.session.services.models_manager) {
            return false;
        }
        let Some((config, settings)) = following_root_settings(&self.session).await else {
            return false;
        };
        let mode = &settings.collaboration_mode;
        if config
            .alab_catalog_successor(listed, mode.model(), mode.reasoning_effort().as_ref())
            .is_none()
        {
            return false;
        }
        cached_successor_needs_confirmation(&config, &settings, manager, RefreshStrategy::InMemory)
            .await
    }

    /// Re-enable following only after a native idle forest has transferred
    /// ownership and verified the exact checkpoint model and effort.
    pub async fn alab_restore_model_following(&self) {
        if !codex_alab_core::active() {
            return;
        }
        let session = &self.session;
        let _settings_guard = thread_settings::acquire_persistence_lock(session).await;
        let client = {
            let mut state = session.state.lock().await;
            let config = &mut state.session_configuration.original_config_do_not_use;
            if !Arc::make_mut(config).alab_enable_recovered_model_following() {
                return;
            }
            config
                .model_catalog
                .is_none()
                .then(|| config.http_client_factory())
        };
        promote_from_catalog(session, true).await;
        let manager = Arc::clone(&session.services.models_manager);
        if let Some(client) = client
            && manager.model_catalog_updates().is_some()
        {
            let stopped = self.io.session_loop_termination.clone();
            session.services.session_extension_data.get_or_init(|| {
                // Native ownership has committed. A slow provider must not hold
                // up the other roots or prevent pending input from being released.
                let refresh = async move {
                    let request = manager.list_models(RefreshStrategy::Online, client);
                    tokio::select! {
                        biased;
                        () = stopped => {},
                        _ = tokio::time::timeout(Duration::from_secs(5), request) => {},
                    }
                }
                .boxed()
                .shared();
                tokio::spawn(refresh.clone());
                HandoffCatalogRefresh(refresh)
            });
        }
    }
}

/// A request observes only this Core's original catalog application. A weak
/// snapshot identifies the exact provider response without retaining its rows.
struct CatalogApplications(watch::Sender<Option<Weak<RefreshedModelCatalog>>>);

impl CatalogApplications {
    fn new() -> Self {
        Self(watch::channel(None).0)
    }
}

pub(super) async fn observe_application(session: &Session, apply: impl Future<Output = ()>) {
    // The mode can change after a request subscribed. Record what the real
    // actor finished in either mode; only the ordinary settings path may act.
    if !codex_alab_core::is_alab_binary() {
        apply.await;
        return;
    }
    let observer = session
        .services
        .session_extension_data
        .get_or_init(CatalogApplications::new);
    // Track the provider publication even when the actor rejects it after an
    // auth change. A delayed resume can then distinguish a handled update
    // from a still-pending one without treating its contents as confirmed.
    let publication = session
        .services
        .models_manager
        .model_catalog_updates()
        .and_then(|updates| updates.borrow().as_ref().map(Arc::downgrade));
    apply.await;
    observer.0.send_replace(publication);
}

async fn wait_for_catalog_application<T>(
    applied: &mut watch::Receiver<Option<Weak<T>>>,
    current: impl Fn() -> Option<Arc<T>>,
) {
    let _ = applied
        .wait_for(|seen| {
            let Some(latest) = current() else { return true };
            seen.as_ref()
                .is_some_and(|seen| seen.ptr_eq(&Arc::downgrade(&latest)))
        })
        .await;
}

/// The catalog observer holds the ordinary settings persistence permit through publication.
pub(super) async fn promote(session: &Session) -> bool {
    promote_from_catalog(session, false).await
}

async fn promote_from_catalog(session: &Session, restoring_idle_root: bool) -> bool {
    let manager = &session.services.models_manager;
    let catalog = manager.refreshed_model_catalog();
    if catalog.is_none() && !restoring_idle_root {
        return false;
    }
    let Some((config, settings)) = following_root_settings(session).await else {
        return false;
    };
    // An idle restore has the same serving catalog available as a fresh startup,
    // including when API-key auth uses bundled or cached models offline.
    let models = match &catalog {
        Some(refreshed) => manager.build_available_models(refreshed.to_vec()),
        None => {
            config
                .alab_models_for_successor(manager.as_ref(), RefreshStrategy::Offline)
                .await
        }
    };
    let mode = &settings.collaboration_mode;
    let Some(index) =
        config.alab_catalog_successor(&models, mode.model(), mode.reasoning_effort().as_ref())
    else {
        return false;
    };
    let mut update = SessionSettingsUpdate::default();
    update.step_settings.model = Some(models[index].model.clone());
    let Ok(Some(commit)) = session
        .update_settings_if(update, |current, _| {
            Arc::ptr_eq(&current.step_settings, &settings)
                && Arc::ptr_eq(&current.original_config_do_not_use, &config)
                && match (manager.refreshed_model_catalog(), &catalog) {
                    (Some(latest), Some(original)) => Arc::ptr_eq(&latest, original),
                    (None, None) => true,
                    _ => false,
                }
        })
        .await
    else {
        return false;
    };
    thread_settings::emit_applied(
        session,
        crate::session::new_submission_id(),
        commit.snapshot,
    )
    .await;
    true
}

async fn cached_successor_needs_confirmation(
    config: &Config,
    settings: &StepSettings,
    manager: &SharedModelsManager,
    strategy: RefreshStrategy,
) -> bool {
    let mode = &settings.collaboration_mode;
    let effort = mode.reasoning_effort();
    let successor =
        |rows: &[ModelPreset]| config.alab_catalog_successor(rows, mode.model(), effort.as_ref());
    // Display rows can include bundled fallback models. Only a row from this
    // provider's scoped cache can justify asking the serving account.
    let scoped = manager
        .model_catalog_with_origins(strategy, config.http_client_factory())
        .await;
    let cached = manager.build_available_models(
        scoped
            .into_iter()
            .filter(|row| row.origin == ModelOrigin::Provider)
            .map(|row| Arc::unwrap_or_clone(row.model))
            .collect(),
    );
    let Some(next) = successor(&cached) else {
        return false;
    };
    manager.refreshed_model_catalog().is_none_or(|confirmed| {
        let rows = manager.build_available_models(confirmed.to_vec());
        successor(&rows).is_none_or(|i| rows[i].model != cached[next].model)
    })
}

async fn following_root_settings(session: &Session) -> Option<(Arc<Config>, Arc<StepSettings>)> {
    let state = session.state.lock().await;
    let current = &state.session_configuration;
    let config = &current.original_config_do_not_use;
    (config.alab_follows_saved_model()
        && current.parent_thread_id.is_none()
        && !current.session_source.is_non_root_agent()
        && !crate::guardian::is_basic_session_source(&current.session_source))
    .then(|| (Arc::clone(config), Arc::clone(&current.step_settings)))
}

#[cfg(test)]
#[path = "live_model_successor_tests.rs"]
mod tests;
