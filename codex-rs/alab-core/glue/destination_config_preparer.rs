// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Host-supplied configuration preparation. None of these values proves native
//! installation, resource completion, or Applied; those remain separate inputs.
use crate::ThreadConfigSnapshot;
use crate::config::Config;
use codex_alab_core::assignment::Binding;
use codex_extension_api::ExtensionData;
use codex_login::AuthConfig;
use codex_login::AuthManager;
use codex_login::auth::AlabDestinationResourceContext;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;

#[path = "destination_config_cohort.rs"]
pub mod cohort;

pub type BeginFuture<'a> =
    Pin<Box<dyn Future<Output = io::Result<Arc<dyn ConfigPlan>>> + Send + 'a>>;
pub type PrepareFuture<'a> =
    Pin<Box<dyn Future<Output = io::Result<Arc<dyn PreparedConfigOwner>>> + Send + 'a>>;
pub type RegistrationFuture<'a> =
    Pin<Box<dyn Future<Output = io::Result<Registration>> + Send + 'a>>;
pub type PlanFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Arc<StagedPlan>>> + Send + 'a>>;
pub type ConfigurationFuture<'a> =
    Pin<Box<dyn Future<Output = io::Result<PreparedConfig>> + Send + 'a>>;

/// Actual Core Session mailbox. This contains no Session graph or native proof.
/// A resource actor queries every captured participant before beginning effects.
pub trait SessionParticipant: Send + Sync {
    fn registration(&self) -> RegistrationFuture<'_>;
    fn begin<'a>(&'a self, registration: Registration) -> PlanFuture<'a>;
    fn prepare<'a>(
        &'a self,
        plan: Arc<StagedPlan>,
        context: AlabDestinationResourceContext,
    ) -> ConfigurationFuture<'a>;
}
#[derive(Clone)]
pub struct ParticipantHandle(Arc<dyn SessionParticipant>);
impl ParticipantHandle {
    pub(crate) fn install(store: &ExtensionData, participant: Arc<dyn SessionParticipant>) {
        store.insert(Self(participant));
    }
    pub fn from_store(store: &ExtensionData) -> io::Result<Self> {
        store
            .get::<Self>()
            .map(|handle| handle.as_ref().clone())
            .ok_or_else(unavailable)
    }
    pub fn participant(&self) -> &Arc<dyn SessionParticipant> {
        &self.0
    }
}

/// One actual host backend occurrence, shared by its Session registrations.
/// Implementations retain the actual ConfigManager/local C/typed loader proof.
pub trait ConfigPreparer: Send + Sync {
    fn source_manager(&self) -> &Arc<AuthManager>;
    fn begin<'a>(&'a self, thread: ThreadConfigSnapshot) -> BeginFuture<'a>;
}
/// Created before B construction. AuthConfig and later configuration MUST use
/// this same fresh B account controller, never a second independently made one.
pub trait ConfigPlan: Send + Sync {
    fn source_manager(&self) -> &Arc<AuthManager>;
    fn auth_config(&self) -> &AuthConfig;
    fn validate(&self) -> io::Result<()>;
    fn prepare<'a>(
        &'a self,
        context: AlabDestinationResourceContext,
        thread: ThreadConfigSnapshot,
    ) -> PrepareFuture<'a>;
}
/// An actual prepared configuration plus its live backend ownership. Validation
/// checks the backend's coherent local/cloud inputs; it is NOT a resources-ready
/// setter. Next preparer owns CM_B and real B for a later B-to-C transition.
pub trait PreparedConfigOwner: Send + Sync {
    fn manager(&self) -> &Arc<AuthManager>;
    fn config(&self) -> &Arc<Config>;
    fn validate(&self) -> io::Result<()>;
    /// Synchronous actual configuration-occurrence gate. The backend keeps C
    /// stable through the callback; validation followed by an unlocked callback
    /// is insufficient. This is NOT native/resource publication authority.
    fn with_current_configuration(
        &self,
        publish: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<()>;
    fn next_preparer(&self) -> Arc<dyn ConfigPreparer>;
}

fn unavailable() -> io::Error {
    io::Error::other("destination configuration preparation unavailable")
}
fn sanitized(error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        "destination configuration preparation unavailable",
    )
}
fn under_configuration_gate(
    run: impl FnOnce(&mut dyn FnMut() -> io::Result<()>) -> io::Result<()>,
    publish: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let mut called = false;
    let mut invalid = false;
    let mut failed = None;
    let result = {
        let mut once = || {
            if called {
                invalid = true;
                return Err(unavailable());
            }
            called = true;
            match publish() {
                Ok(()) => Ok(()),
                Err(error) => {
                    let kind = error.kind();
                    failed = Some(sanitized(error));
                    Err(io::Error::new(
                        kind,
                        "destination configuration adoption failed",
                    ))
                }
            }
        };
        run(&mut once)
    };
    if let Some(error) = failed {
        return Err(error);
    }
    result.map_err(sanitized)?;
    if !called || invalid {
        return Err(unavailable());
    }
    Ok(())
}

#[derive(Clone)]
pub struct Registration(Arc<Registered>);
struct Registered {
    backend: Arc<dyn ConfigPreparer>,
    source: Arc<AuthManager>,
    domain: Arc<codex_alab_core::assignment::Domain>,
}
#[derive(Default)]
struct Slot(Mutex<Option<Result<Registration, ()>>>);
impl Registration {
    pub fn new(backend: Arc<dyn ConfigPreparer>) -> io::Result<Self> {
        let source = backend.source_manager().clone();
        if !Arc::ptr_eq(&source.alab_capture_current_manager()?, &source) {
            return Err(unavailable());
        }
        let domain = source.alab_assignment_domain();
        Ok(Self(Arc::new(Registered {
            backend,
            source,
            domain,
        })))
    }
    pub(crate) fn check_source(&self, source: &Arc<AuthManager>) -> io::Result<()> {
        if !Arc::ptr_eq(&self.0.source, source)
            || !Arc::ptr_eq(&self.0.source, self.0.backend.source_manager())
            || !Arc::ptr_eq(&source.alab_capture_current_manager()?, source)
            || !Arc::ptr_eq(&self.0.domain, &source.alab_assignment_domain())
        {
            return Err(unavailable());
        }
        Ok(())
    }
    /// Equality of actual retained occurrences, never a label or numeric id.
    pub fn same_occurrence(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0.source, &other.0.source)
            && Arc::ptr_eq(&self.0.backend, &other.0.backend)
    }
    /// ThreadLifecycleContributor installs this into its actual session_store.
    /// Conflicting contributors invalidate the slot rather than selecting one.
    pub fn install(&self, store: &ExtensionData) -> io::Result<()> {
        self.check_source(&self.0.source)?;
        let slot = store.get_or_init(Slot::default);
        let mut slot = slot.0.lock().map_err(|_| unavailable())?;
        match slot.as_ref() {
            None => {
                *slot = Some(Ok(self.clone()));
                Ok(())
            }
            Some(Ok(existing)) if self.same_occurrence(existing) => Ok(()),
            _ => {
                *slot = Some(Err(()));
                Err(unavailable())
            }
        }
    }
    pub fn from_store(store: &ExtensionData, actual_source: &Arc<AuthManager>) -> io::Result<Self> {
        let slot = store.get::<Slot>().ok_or_else(unavailable)?;
        let registration = slot
            .0
            .lock()
            .map_err(|_| unavailable())?
            .as_ref()
            .and_then(|slot| slot.as_ref().ok())
            .cloned()
            .ok_or_else(unavailable)?;
        registration.check_source(actual_source)?;
        Ok(registration)
    }
    pub async fn begin(&self, thread: ThreadConfigSnapshot) -> io::Result<StagedPlan> {
        self.check_source(&self.0.source)?;
        let backend = self.0.backend.begin(thread).await.map_err(sanitized)?;
        self.check_source(&self.0.source)?;
        backend.validate().map_err(sanitized)?;
        if !Arc::ptr_eq(backend.source_manager(), &self.0.source) {
            return Err(unavailable());
        }
        backend.auth_config().validate().map_err(sanitized)?;
        let policy = backend
            .auth_config()
            .auth_route_config
            .http_client_factory()
            .network_policy();
        let source = self.0.source.runtime_config();
        let original = source
            .auth_route_config
            .http_client_factory()
            .network_policy();
        if !policy.alab_managed_configuration().map_err(sanitized)?
            || (original.alab_managed_configuration().map_err(sanitized)?
                && policy.clone().with_account_binding(original).is_some())
        {
            return Err(unavailable());
        }
        Ok(StagedPlan {
            registration: self.clone(),
            backend,
        })
    }
}

/// The resource actor coalesces these by Registration::same_occurrence BEFORE
/// effects. Different backend owners cannot silently share a B controller.
pub struct StagedPlan {
    registration: Registration,
    backend: Arc<dyn ConfigPlan>,
}
impl StagedPlan {
    pub fn registration(&self) -> &Registration {
        &self.registration
    }
    pub fn auth_config(&self) -> &AuthConfig {
        self.backend.auth_config()
    }
    pub async fn prepare(
        &self,
        context: AlabDestinationResourceContext,
        thread: ThreadConfigSnapshot,
    ) -> io::Result<PreparedConfig> {
        self.registration
            .check_source(&self.registration.0.source)?;
        self.backend.validate().map_err(sanitized)?;
        let before = context.current_binding().await?;
        let runtime = context.manager().runtime_config();
        let configured = self
            .auth_config()
            .auth_route_config
            .http_client_factory()
            .network_policy();
        let actual = runtime
            .auth_route_config
            .http_client_factory()
            .network_policy();
        if !actual.alab_managed_configuration().map_err(sanitized)?
            || !configured.alab_managed_configuration().map_err(sanitized)?
            || configured.clone().with_account_binding(actual).is_none()
        {
            return Err(unavailable());
        }
        let owner = context
            .with_resources(async {
                let resource = context.manager().alab_account_resource()?;
                resource
                    .scope(self.backend.prepare(context.clone(), thread))
                    .await
            })
            .await??;
        self.backend.validate().map_err(sanitized)?;
        owner.validate().map_err(sanitized)?;
        if !Arc::ptr_eq(owner.manager(), context.manager()) {
            return Err(unavailable());
        }
        let config = owner.config().clone();
        context
            .with_resources(async {
                let auth = context.manager().auth_cached().ok_or_else(unavailable)?;
                if !config.auth_config().allows_auth(&auth)
                    || !config
                        .application_network_policy
                        .alab_managed_configuration()
                        .map_err(sanitized)?
                    || config
                        .application_network_policy
                        .clone()
                        .with_account_binding(actual)
                        .is_none()
                {
                    return Err(unavailable());
                }
                Ok::<(), io::Error>(())
            })
            .await??;
        let after = context.current_binding().await?;
        if before != after {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "destination credential snapshot changed",
            ));
        }
        let next = Registration::new(owner.next_preparer())?;
        next.check_source(context.manager())?;
        Ok(PreparedConfig {
            config,
            owner,
            next,
            binding: after,
        })
    }
}
/// Configuration only. The Core participant visitor still must construct real
/// ModelClient/models/MCP resources and obtain its separate completion proof.
pub struct PreparedConfig {
    config: Arc<Config>,
    owner: Arc<dyn PreparedConfigOwner>,
    next: Registration,
    binding: Binding,
}
impl PreparedConfig {
    pub fn config(&self) -> &Arc<Config> {
        &self.config
    }
    pub fn manager(&self) -> &Arc<AuthManager> {
        self.owner.manager()
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn next_registration(&self) -> &Registration {
        &self.next
    }
    /// Called only by the resource actor while separately retaining its native
    /// Applied and actual participant completion proofs. Error after the callback
    /// must be reconciled; it is not evidence that publication had no effects.
    pub fn with_current_configuration(
        &self,
        publish: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        under_configuration_gate(|once| self.owner.with_current_configuration(once), publish)
    }
    pub async fn validate(&self, context: &AlabDestinationResourceContext) -> io::Result<()> {
        self.owner.validate().map_err(sanitized)?;
        if !Arc::ptr_eq(self.manager(), context.manager())
            || !Arc::ptr_eq(&self.config, self.owner.config())
            || self.binding != context.current_binding().await?
        {
            return Err(unavailable());
        }
        self.next.check_source(context.manager())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Backend(Arc<AuthManager>);
    impl ConfigPreparer for Backend {
        fn source_manager(&self) -> &Arc<AuthManager> {
            &self.0
        }
        fn begin<'a>(&'a self, _: ThreadConfigSnapshot) -> BeginFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
    }
    #[test]
    fn configuration_gate_cannot_hide_failed_or_repeated_adoption_callback() {
        assert!(
            under_configuration_gate(
                |publish| {
                    let _ = publish();
                    Ok(())
                },
                &mut || Err(io::Error::other("fixture rejection"))
            )
            .is_err()
        );
        let mut effects = 0;
        assert!(
            under_configuration_gate(
                |publish| {
                    publish()?;
                    let _ = publish();
                    Ok(())
                },
                &mut || {
                    effects += 1;
                    Ok(())
                }
            )
            .is_err()
        );
        assert_eq!(effects, 1, "a repeated callback cannot repeat publication");
        assert!(
            under_configuration_gate(|_| Ok(()), &mut || {
                effects += 1;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(effects, 1);
    }
    #[test]
    fn missing_foreign_or_conflicting_registration_never_becomes_configuration_authority() {
        let a =
            AuthManager::from_auth_for_testing(codex_login::CodexAuth::from_api_key("fixture-a"));
        let b =
            AuthManager::from_auth_for_testing(codex_login::CodexAuth::from_api_key("fixture-b"));
        let store = ExtensionData::new("actual-session");
        assert!(Registration::from_store(&store, &a).is_err());
        let backend: Arc<dyn ConfigPreparer> = Arc::new(Backend(a.clone()));
        let first = Registration::new(backend.clone()).unwrap();
        let same = Registration::new(backend).unwrap();
        first.install(&store).unwrap();
        same.install(&store).unwrap();
        assert!(
            Registration::from_store(&store, &a)
                .unwrap()
                .same_occurrence(&first)
        );
        assert!(Registration::from_store(&store, &b).is_err());
        let foreign_owner = Registration::new(Arc::new(Backend(a.clone()))).unwrap();
        assert!(!first.same_occurrence(&foreign_owner));
        assert!(foreign_owner.install(&store).is_err());
        assert!(
            Registration::from_store(&store, &a).is_err(),
            "a known conflict cannot fall back to the earlier owner"
        );
    }
}
