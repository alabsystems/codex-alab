// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! App-server owns staged configuration; Core owns resource/native publication.
use super::*;
use codex_core::alab_destination_config as api;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistry;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_login::AuthConfig;
use std::io;
#[path = "application_destination_prepare.rs"]
mod prepare;
fn unavailable() -> io::Error {
    io::Error::other("destination configuration owner unavailable")
}
struct Backend {
    source: Arc<AuthManager>,
    manager: ConfigManager,
    routes: AuthRouteConfig,
}
struct Plan {
    source: Arc<AuthManager>,
    owner: Arc<DestinationConfigOwner>,
    bootstrap: Arc<DestinationBootstrap>,
    destination: std::sync::Mutex<Option<Arc<Backend>>>,
    preparation: tokio::sync::Semaphore,
}
struct Prepared {
    owner: Arc<DestinationConfigOwner>,
    bootstrap: Arc<DestinationBootstrap>,
    next: Arc<Backend>,
    config: Arc<Config>,
    policy: ApplicationPolicyLoad,
}
impl api::ConfigPreparer for Backend {
    fn source_manager(&self) -> &Arc<AuthManager> {
        &self.source
    }
    fn begin<'a>(&'a self, thread: codex_core::ThreadConfigSnapshot) -> api::BeginFuture<'a> {
        Box::pin(async move {
            if !Arc::ptr_eq(&self.source, &self.source.alab_capture_current_manager()?) {
                return Err(unavailable());
            }
            let mut owner = self
                .manager
                .alab_begin_registered_destination(&self.routes)?;
            let bootstrap = Arc::new(
                owner
                    .local_bootstrap(thread.cwd().as_path().to_path_buf())
                    .await?,
            );
            Ok(Arc::new(Plan {
                source: self.source.clone(),
                owner: Arc::new(owner),
                bootstrap,
                destination: Default::default(),
                preparation: tokio::sync::Semaphore::new(1),
            }) as Arc<dyn api::ConfigPlan>)
        })
    }
}
impl api::ConfigPlan for Plan {
    fn source_manager(&self) -> &Arc<AuthManager> {
        &self.source
    }
    fn auth_config(&self) -> &AuthConfig {
        self.bootstrap.auth_config()
    }
    fn validate(&self) -> io::Result<()> {
        self.bootstrap.validate(&self.owner)
    }
    fn prepare<'a>(
        &'a self,
        context: codex_login::auth::AlabDestinationResourceContext,
        thread: codex_core::ThreadConfigSnapshot,
    ) -> api::PrepareFuture<'a> {
        Box::pin(async move {
            context
                .with_resources(self.prepare_configuration(context.clone(), thread))
                .await?
        })
    }
}
impl api::PreparedConfigOwner for Prepared {
    fn manager(&self) -> &Arc<AuthManager> {
        &self.next.source
    }
    fn config(&self) -> &Arc<Config> {
        &self.config
    }
    fn validate(&self) -> io::Result<()> {
        self.bootstrap.validate(&self.owner)?;
        self.owner
            .manager
            .check_application_policy_load(&self.policy)
    }
    fn with_current_configuration(
        &self,
        publish: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        self.bootstrap.with_current(&self.owner, || {
            self.owner
                .manager
                .check_application_policy_load(&self.policy)?;
            publish()
        })
    }
    fn next_preparer(&self) -> Arc<dyn api::ConfigPreparer> {
        self.next.clone()
    }
}
struct Contributor {
    registration: Option<api::Registration>,
}
impl ThreadLifecycleContributor<Config> for Contributor {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(registration) = &self.registration {
                let _ = registration.install(input.session_store);
            }
        })
    }
}
/// One registration occurrence per actual processor CM/source, shared by all
/// its sessions. Unknown dynamic thread loaders cannot acquire a plan later.
pub(crate) fn register(
    extensions: Arc<ExtensionRegistry<Config>>,
    manager: &ConfigManager,
    source: &Arc<AuthManager>,
    config: &Config,
) -> Arc<ExtensionRegistry<Config>> {
    let registration = registration(manager, source, config);
    let mut builder = extensions.to_builder();
    builder.thread_lifecycle_contributor(Arc::new(Contributor { registration }));
    Arc::new(builder.build())
}
fn registration(
    manager: &ConfigManager,
    source: &Arc<AuthManager>,
    config: &Config,
) -> Option<api::Registration> {
    source
        .alab_capture_current_manager()
        .and_then(|source| {
            manager.alab_destination_thread_source()?;
            api::Registration::new(Arc::new(Backend {
                source,
                manager: manager.clone(),
                routes: config.auth_route_config(),
            }))
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn actual_contributor_shares_backend_occurrence_and_unknown_loader_stays_missing() {
        let home = tempfile::tempdir().unwrap();
        let make = |loader| {
            ConfigManager::new(
                home.path().to_path_buf(),
                Vec::new(),
                LoaderOverrides::without_managed_config_for_tests(),
                true,
                CloudConfigBundleLoader::default(),
                Arg0DispatchPaths::default(),
                loader,
            )
        };
        let cm = make(alab_no_external_thread_config_loader(
            codex_config::NoopThreadConfigLoader,
        ));
        let unknown =
            make(Arc::new(codex_config::NoopThreadConfigLoader) as Arc<dyn ThreadConfigLoader>);
        let mut config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .build()
            .await
            .unwrap();
        cm.apply_network_policy(&mut config);
        let source = AuthManager::from_auth_for_testing(codex_login::CodexAuth::from_api_key(
            "fixture-only",
        ));
        assert!(
            registration(&unknown, &source, &config).is_none(),
            "same concrete type is not registered provenance"
        );
        let contributor = Contributor {
            registration: registration(&cm, &source, &config),
        };
        assert!(contributor.registration.is_some());
        let first = codex_extension_api::ExtensionData::new("destination-first");
        let second = codex_extension_api::ExtensionData::new("destination-second");
        let thread_store = codex_extension_api::ExtensionData::new("destination-thread");
        let session_source = codex_protocol::protocol::SessionSource::Cli;
        for store in [&first, &second] {
            contributor
                .on_thread_start(ThreadStartInput {
                    config: &config,
                    session_source: &session_source,
                    persistent_thread_state_available: false,
                    environments: &[],
                    mcp_resource_client: None,
                    extension_metrics: None,
                    session_store: store,
                    thread_store: &thread_store,
                })
                .await;
        }
        let first_registration = api::Registration::from_store(&first, &source).unwrap();
        let second_registration = api::Registration::from_store(&second, &source).unwrap();
        assert!(first_registration.same_occurrence(&second_registration));
        let foreign = AuthManager::from_auth_for_testing(codex_login::CodexAuth::from_api_key(
            "other-fixture",
        ));
        assert!(api::Registration::from_store(&first, &foreign).is_err());
    }
}
