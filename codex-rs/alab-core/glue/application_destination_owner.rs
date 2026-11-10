// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Construction only. B has fresh unavailable account policy ownership; no
//! requirements load, native credential authority or resource publication occurs.
use super::*;
#[path = "application_destination_preparer.rs"]
pub(crate) mod preparer;

#[path = "application_destination_bootstrap.rs"]
mod bootstrap;
#[path = "application_local_inputs.rs"]
pub(crate) mod local_inputs;
pub(crate) use bootstrap::DestinationBootstrap;
use codex_login::AuthRouteConfig;
use std::sync::OnceLock;
use std::sync::Weak;

/// Proven local input supplied at the actual ConfigManager construction site.
/// There is deliberately no constructor from an arbitrary dynamic loader, raw
/// origin label or boolean. Pointer equality binds this occurrence to the manager.
pub(crate) struct DestinationThreadConfigSource {
    loader: Arc<codex_config::NoopThreadConfigLoader>,
}
static LOCAL_THREAD_SOURCES: OnceLock<Mutex<Vec<Weak<codex_config::NoopThreadConfigLoader>>>> =
    OnceLock::new();

fn local_thread_sources() -> &'static Mutex<Vec<Weak<codex_config::NoopThreadConfigLoader>>> {
    LOCAL_THREAD_SOURCES.get_or_init(Mutex::default)
}

impl DestinationThreadConfigSource {
    // Registration exists only at actual concrete construction. No API can
    // register an arbitrary existing Arc or declare its provenance afterward.
    fn construct(source: codex_config::NoopThreadConfigLoader) -> Self {
        let loader = Arc::new(source);
        let mut sources = local_thread_sources()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sources.retain(|source| source.strong_count() != 0);
        sources.push(Arc::downgrade(&loader));
        Self { loader }
    }

    #[cfg(test)]
    fn no_external_sources() -> Self {
        Self::construct(codex_config::NoopThreadConfigLoader)
    }

    fn loader(&self) -> Arc<dyn ThreadConfigLoader> {
        self.loader.clone()
    }

    fn lookup(actual: &Arc<dyn ThreadConfigLoader>) -> Option<Self> {
        let mut sources = local_thread_sources()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sources.retain(|source| source.strong_count() != 0);
        sources.iter().filter_map(Weak::upgrade).find_map(|loader| {
            let dynamic: Arc<dyn ThreadConfigLoader> = loader.clone();
            Arc::ptr_eq(actual, &dynamic).then_some(Self { loader })
        })
    }
}

/// Construct the actual account-independent local loader. The caller receives
/// only its normal dynamic interface; provenance is a weak allocation join.
/// This neither pins the source nor declares any dynamic loader independent.
pub fn alab_no_external_thread_config_loader(
    source: codex_config::NoopThreadConfigLoader,
) -> Arc<dyn ThreadConfigLoader> {
    DestinationThreadConfigSource::construct(source).loader()
}

/// Private preparation ownership. This intentionally exposes neither its
/// ConfigManager nor load/publish methods: local bootstrap and B requirements
/// need their actual destination resource scope and coherent C revision first.
pub(crate) struct DestinationConfigOwner {
    manager: ConfigManager,
    routes: AuthRouteConfig,
    _thread_source: Arc<codex_config::NoopThreadConfigLoader>,
}

impl ConfigManager {
    pub(crate) fn alab_destination_thread_source(
        &self,
    ) -> std::io::Result<DestinationThreadConfigSource> {
        DestinationThreadConfigSource::lookup(&self.thread_config_loader).ok_or_else(|| {
            std::io::Error::other("destination thread configuration authority is unavailable")
        })
    }

    /// Only a genuine constructor registration of this manager's actual loader
    /// can supply the typed input; an unknown dynamic loader stays unsupported.
    pub(crate) fn alab_begin_registered_destination(
        &self,
        source: &AuthRouteConfig,
    ) -> std::io::Result<DestinationConfigOwner> {
        let thread = self.alab_destination_thread_source()?;
        self.alab_begin_destination(source, &thread)
    }

    /// The source and its typed local thread input must be the actual manager's.
    /// Allocate the routes/controller as one occurrence internally; callers
    /// cannot pair separately supplied account controllers and route snapshots.
    pub(crate) fn alab_begin_destination(
        &self,
        source: &AuthRouteConfig,
        thread: &DestinationThreadConfigSource,
    ) -> std::io::Result<DestinationConfigOwner> {
        if !Arc::ptr_eq(&self.thread_config_loader, &thread.loader()) {
            return Err(std::io::Error::other(
                "destination thread configuration authority is unavailable",
            ));
        }
        // Endpoint limits may differ; exact configuration owner identity must
        // agree. The fresh-route factory preserves every original limit.
        if !source.alab_has_configuration_owners(
            &self.network_policy.policy(),
            &self.local_network_policy.policy(),
        )? {
            return Err(std::io::Error::other("source configuration owner differs"));
        }
        if self
            .cloud_config_bundle
            .read()
            .map_err(|_| std::io::Error::other("source requirements ownership poisoned"))?
            .config_server()
            .is_some()
        {
            return Err(std::io::Error::other(
                "independent config-server destination authority is unavailable",
            ));
        }
        let (routes, network_policy) = source.alab_fresh_destination_routes()?;
        let manager = Self {
            codex_home: self.codex_home.clone(),
            cli_overrides: Arc::clone(&self.cli_overrides),
            runtime_feature_enablement: Arc::clone(&self.runtime_feature_enablement),
            local_inputs: Arc::clone(&self.local_inputs),
            loader_overrides: self.loader_overrides.clone(),
            strict_config: self.strict_config,
            cloud_config_bundle: Arc::new(RwLock::new(CloudConfigBundleLoader::default())),
            arg0_paths: self.arg0_paths.clone(),
            thread_config_loader: thread.loader(),
            network_policy,
            local_network_policy: self.local_network_policy.clone(),
            network_policy_reload: Arc::new(tokio::sync::Semaphore::new(/*permits*/ 1)),
            network_policy_snapshot: Arc::default(),
            mcp_ema_cancellation: Arc::default(),
        };
        Ok(DestinationConfigOwner {
            manager,
            routes,
            _thread_source: Arc::clone(&thread.loader),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_http_client::DestinationPolicy;
    use codex_http_client::HttpClientFactory;
    use codex_http_client::NetworkPolicyDenied;
    use codex_http_client::OutboundProxyPolicy;

    fn source(thread: &DestinationThreadConfigSource) -> ConfigManager {
        ConfigManager::new(
            PathBuf::from("/not-read-by-destination-construction"),
            Vec::new(),
            LoaderOverrides::default(),
            /*strict_config*/ true,
            CloudConfigBundleLoader::default(),
            Arg0DispatchPaths::default(),
            thread.loader(),
        )
    }

    fn routes(manager: &ConfigManager) -> AuthRouteConfig {
        let factory = |policy| {
            HttpClientFactory::new(OutboundProxyPolicy::RespectSystemProxy)
                .with_network_policy(policy)
        };
        AuthRouteConfig::from_http_client_factory(factory(manager.network_policy.policy()))
            .with_local_bootstrap_factory(factory(manager.local_network_policy.policy()))
    }

    fn registered_source() -> ConfigManager {
        ConfigManager::new(
            PathBuf::from("/not-read-by-registered-construction"),
            Vec::new(),
            LoaderOverrides::default(),
            /*strict_config*/ true,
            CloudConfigBundleLoader::default(),
            Arg0DispatchPaths::default(),
            alab_no_external_thread_config_loader(codex_config::NoopThreadConfigLoader),
        )
    }

    #[test]
    fn production_constructor_lookup_joins_only_the_actual_manager_allocation() {
        let source = registered_source();
        let proof = source.alab_destination_thread_source().unwrap();
        assert!(Arc::ptr_eq(&source.thread_config_loader, &proof.loader()));
        let destination = source
            .alab_begin_registered_destination(&routes(&source))
            .unwrap();
        assert!(Arc::ptr_eq(
            &destination.manager.thread_config_loader,
            &source.thread_config_loader
        ));
        let foreign = registered_source();
        let foreign_proof = foreign.alab_destination_thread_source().unwrap();
        assert!(!Arc::ptr_eq(
            &source.thread_config_loader,
            &foreign_proof.loader()
        ));
        assert!(
            source
                .alab_begin_destination(&routes(&source), &foreign_proof)
                .is_err()
        );
        let mut unknown = source;
        unknown.thread_config_loader = Arc::new(codex_config::NoopThreadConfigLoader);
        assert!(unknown.alab_destination_thread_source().is_err());
        assert!(
            unknown
                .alab_begin_registered_destination(&routes(&unknown))
                .is_err()
        );
        unknown.thread_config_loader = Arc::new(codex_config::StaticThreadConfigLoader::default());
        assert!(unknown.alab_destination_thread_source().is_err());
    }

    #[test]
    fn weak_constructor_registry_does_not_pin_loaders_and_prunes_dead_occurrences() {
        let actual = alab_no_external_thread_config_loader(codex_config::NoopThreadConfigLoader);
        let proof = DestinationThreadConfigSource::lookup(&actual).unwrap();
        let weak = Arc::downgrade(&proof.loader);
        drop(proof);
        drop(actual);
        assert!(
            weak.upgrade().is_none(),
            "registry must not retain the actual loader"
        );
        let unknown: Arc<dyn ThreadConfigLoader> = Arc::new(codex_config::NoopThreadConfigLoader);
        assert!(DestinationThreadConfigSource::lookup(&unknown).is_none());
        assert!(
            !local_thread_sources()
                .lock()
                .unwrap()
                .iter()
                .any(|entry| Weak::ptr_eq(entry, &weak))
        );
        // A later registered constructor is a different occurrence; no dead
        // registration or matching concrete type can authorize the old one.
        let next = alab_no_external_thread_config_loader(codex_config::NoopThreadConfigLoader);
        let next_proof = DestinationThreadConfigSource::lookup(&next).unwrap();
        assert!(!Weak::ptr_eq(&Arc::downgrade(&next_proof.loader), &weak));
        assert!(DestinationThreadConfigSource::lookup(&unknown).is_none());
    }

    #[tokio::test]
    async fn destination_owns_unavailable_account_state_and_preserves_live_local_inputs() {
        let thread = DestinationThreadConfigSource::no_external_sources();
        let source = source(&thread);
        let source_cloud_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = Arc::clone(&source_cloud_calls);
        *source.cloud_config_bundle.write().unwrap() =
            CloudConfigBundleLoader::from_getter(move || {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::ready(Ok(None))
            });
        let url: url::Url = "https://destination.example.test/".parse().unwrap();
        assert!(source.network_policy.publish(
            source.network_policy.policy().revision(),
            DestinationPolicy::Unrestricted,
        ));
        assert!(source.local_network_policy.publish(
            source.local_network_policy.policy().revision(),
            DestinationPolicy::Unrestricted,
        ));
        let source_permit = source.network_policy.policy().acquire(&url).unwrap();
        let source_snapshot = Arc::new(ApplicationPolicySnapshot {
            revision: source.network_policy.policy().revision(),
            policy: DestinationPolicy::Unrestricted,
            cloud: None,
        });
        *source.network_policy_snapshot.write().unwrap() = Some(Arc::clone(&source_snapshot));
        let source_ema = source.mcp_ema_cancellation.lock().unwrap().startup.clone();
        let destination = source
            .alab_begin_destination(&routes(&source), &thread)
            .unwrap();
        let manager = &destination.manager;
        assert!(Arc::ptr_eq(&manager.cli_overrides, &source.cli_overrides));
        assert!(Arc::ptr_eq(
            &manager.runtime_feature_enablement,
            &source.runtime_feature_enablement
        ));
        assert!(Arc::ptr_eq(
            &manager.thread_config_loader,
            &source.thread_config_loader
        ));
        assert_eq!(
            manager.local_network_policy.policy(),
            source.local_network_policy.policy()
        );
        assert_ne!(
            manager.network_policy.policy(),
            source.network_policy.policy()
        );
        assert!(!Arc::ptr_eq(
            &manager.cloud_config_bundle,
            &source.cloud_config_bundle
        ));
        assert!(!Arc::ptr_eq(
            &manager.network_policy_reload,
            &source.network_policy_reload
        ));
        assert!(!Arc::ptr_eq(
            &manager.network_policy_snapshot,
            &source.network_policy_snapshot
        ));
        assert!(!Arc::ptr_eq(
            &manager.mcp_ema_cancellation,
            &source.mcp_ema_cancellation
        ));
        assert!(manager.network_policy_snapshot.read().unwrap().is_none());
        assert!(
            manager
                .current_managed_config_loader()
                .get()
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            source_cloud_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "B must not inherit A's cloud getter or refresher"
        );
        assert!(
            source
                .current_managed_config_loader()
                .get()
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            source_cloud_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert!(manager.strict_config);
        assert_eq!(manager.codex_home, source.codex_home);
        assert_eq!(
            destination.routes.http_client_factory().network_policy(),
            &manager.network_policy.policy()
        );
        assert!(matches!(
            manager.network_policy.policy().acquire(&url),
            Err(NetworkPolicyDenied::Unavailable)
        ));
        assert!(source_permit.check().is_ok());
        assert!(!source_ema.is_cancelled());
        assert!(Arc::ptr_eq(
            source
                .network_policy_snapshot
                .read()
                .unwrap()
                .as_ref()
                .unwrap(),
            &source_snapshot
        ));
        source
            .cli_overrides
            .write()
            .unwrap()
            .push(("live".into(), TomlValue::Boolean(true)));
        assert_eq!(
            manager.current_cli_overrides(),
            source.current_cli_overrides()
        );
        source
            .runtime_feature_enablement
            .write()
            .unwrap()
            .insert("live".into(), true);
        assert_eq!(
            *manager.runtime_feature_enablement.read().unwrap(),
            *source.runtime_feature_enablement.read().unwrap()
        );
        let local_permit = manager.local_network_policy.policy().acquire(&url).unwrap();
        source.local_network_policy.policy().invalidate();
        assert!(matches!(
            local_permit.check(),
            Err(NetworkPolicyDenied::Revoked)
        ));
        assert!(matches!(
            manager.local_network_policy.policy().acquire(&url),
            Err(NetworkPolicyDenied::Unavailable)
        ));
        assert!(matches!(
            manager.network_policy.policy().acquire(&url),
            Err(NetworkPolicyDenied::Unavailable)
        ));
    }

    #[test]
    fn restricted_routes_preserve_actual_owners_and_endpoint_limits() {
        let thread = DestinationThreadConfigSource::no_external_sources();
        let source = source(&thread);
        let allowed: url::Url = "https://allowed.example.test/config".parse().unwrap();
        let denied: url::Url = "https://denied.example.test/config".parse().unwrap();
        assert!(source.local_network_policy.publish(
            source.local_network_policy.policy().revision(),
            DestinationPolicy::Unrestricted,
        ));
        let endpoints = || std::iter::once(allowed.clone()).collect();
        let factory = |policy| {
            HttpClientFactory::new(OutboundProxyPolicy::RespectSystemProxy)
                .with_network_policy(policy)
        };
        let restricted = AuthRouteConfig::from_http_client_factory(factory(
            source
                .network_policy
                .policy()
                .restrict_to_endpoints(endpoints()),
        ))
        .with_application_network_policy(
            source
                .network_policy
                .policy()
                .restrict_to_endpoints(endpoints()),
        )
        .with_local_bootstrap_factory(factory(
            source
                .local_network_policy
                .policy()
                .restrict_to_endpoints(endpoints()),
        ));
        let destination = source
            .alab_begin_registered_destination(&restricted)
            .unwrap();
        // No B requirements were loaded: permitted endpoints remain unavailable.
        let content = destination.routes.http_client_factory();
        assert!(matches!(
            content.network_policy().acquire(&allowed),
            Err(NetworkPolicyDenied::Unavailable)
        ));
        assert!(matches!(
            content.network_policy().acquire(&denied),
            Err(NetworkPolicyDenied::Destination)
        ));
        let bootstrap = destination
            .routes
            .http_client_factory_for_bootstrap([allowed.clone(), denied.clone()]);
        assert!(bootstrap.network_policy().acquire(&allowed).is_ok());
        assert!(matches!(
            bootstrap.network_policy().acquire(&denied),
            Err(NetworkPolicyDenied::Destination)
        ));
    }

    #[test]
    fn unknown_thread_loader_and_foreign_route_owners_cannot_construct_destination() {
        let thread = DestinationThreadConfigSource::no_external_sources();
        let source = source(&thread);
        let unrelated = DestinationThreadConfigSource::no_external_sources();
        assert!(
            source
                .alab_begin_destination(&routes(&source), &unrelated)
                .is_err()
        );
        let mut unknown = source.clone();
        unknown.thread_config_loader = Arc::new(codex_config::NoopThreadConfigLoader);
        assert!(
            unknown
                .alab_begin_destination(&routes(&unknown), &thread)
                .is_err(),
            "even another Noop instance is not the actual constructor occurrence"
        );
        let foreign = self::source(&unrelated);
        assert!(
            source
                .alab_begin_destination(&routes(&foreign), &thread)
                .is_err()
        );
        let foreign_application =
            routes(&source).with_application_network_policy(foreign.network_policy.policy());
        assert!(
            source
                .alab_begin_destination(&foreign_application, &thread)
                .is_err()
        );
        let foreign_local = AuthRouteConfig::from_http_client_factory(
            routes(&source).http_client_factory().clone(),
        )
        .with_local_bootstrap_factory(routes(&foreign).http_client_factory().clone());
        assert!(
            source
                .alab_begin_destination(&foreign_local, &thread)
                .is_err()
        );
        assert!(source.network_policy_snapshot.read().unwrap().is_none());
    }
}
