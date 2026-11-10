// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A destination is a separate real manager. Its old census stays owned until
//! actual destination resources and native ACK permit host publication.
use super::*;
use codex_alab_core::assignment_service::Target;
use codex_alab_core::manager_catalog::DestinationRecord;
use codex_alab_core::manager_catalog::Seal;
use codex_alab_core::native_target::Destination;
#[path = "auth_destination_applied.rs"]
mod applied;
pub use applied::DestinationAckObservation;
pub use applied::DestinationAppliedProof;

pub struct DestinationEpoch {
    manager: Arc<AuthManager>,
    prepared: Arc<PreparedDestination>,
    census: Arc<DestinationRecord>,
    construction: Option<codex_alab_core::native_target::ConstructionReceipt>,
    runtime: tokio::runtime::Handle,
}
#[derive(Clone)]
pub struct DestinationResourceContext {
    manager: Arc<AuthManager>,
    prepared: Arc<PreparedDestination>,
    census: Arc<DestinationRecord>,
}
impl std::fmt::Debug for DestinationResourceContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DestinationResources(<private>)")
    }
}
impl DestinationResourceContext {
    pub fn manager(&self) -> &Arc<AuthManager> {
        &self.manager
    }
    pub fn domain(&self) -> Arc<codex_alab_core::assignment::Domain> {
        self.manager.alab_assignment_domain()
    }
    fn validate(&self) -> std::io::Result<()> {
        self.census
            .validate()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        if !Arc::ptr_eq(self.census.domain(), &self.domain()) {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        self.prepared.validate()?;
        self.census
            .resource_scope(|| {
                self.manager
                    .alab_verify_native_target(&self.prepared.target)
            })
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?
    }
    pub async fn with_resources<F: std::future::Future>(
        &self,
        future: F,
    ) -> std::io::Result<F::Output> {
        self.validate()?;
        self.census
            .scope_resources(future)
            .await
            .map_err(|_| codex_alab_core::managed_auth::unavailable())
    }
    pub async fn current_binding(&self) -> std::io::Result<codex_alab_core::assignment::Binding> {
        self.validate()?;
        let binding = self.manager.alab_destination_binding().await?;
        self.validate()?;
        Ok(binding)
    }
    /// Actual member/Ready metadata; this is not all-participant completion.
    pub async fn resource_report(
        &self,
        boundary: &str,
    ) -> std::io::Result<codex_alab_core::native_target::ResourceReport> {
        self.validate()?;
        let report = self.manager.alab_destination_report(boundary).await?;
        self.validate()?;
        Ok(report)
    }
}
/// Pre-effect policy evidence only: no token, native SourceSeal or installation
/// permission. Captured while A was genuinely loaded, then rechecked without
/// trying to reopen A's credential reader after retirement.
pub struct PreparedDestination {
    source: Arc<AuthManager>,
    control: Arc<AuthManager>,
    epoch: codex_alab_core::manager_source_epoch::Epoch,
    target: Target,
    witness: super::alab_native_source::target_policy::Witness,
}
impl PreparedDestination {
    fn validate(&self) -> std::io::Result<()> {
        if !Arc::ptr_eq(&self.control.alab_capture_current_manager()?, &self.source)
            || !Arc::ptr_eq(&self.epoch.domain(), &self.source.alab_assignment_domain())
        {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        self.source.alab_destination_constraints(&self.target)?;
        self.witness.source_target(&self.target)
    }
}
impl std::fmt::Debug for DestinationEpoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DestinationEpoch(<private>)")
    }
}
impl DestinationEpoch {
    pub fn manager(&self) -> &Arc<AuthManager> {
        &self.manager
    }
    pub fn domain(&self) -> Arc<codex_alab_core::assignment::Domain> {
        self.manager.alab_assignment_domain()
    }
    pub fn resource_context(&self) -> std::io::Result<DestinationResourceContext> {
        self.validate()?;
        Ok(DestinationResourceContext {
            manager: self.manager.clone(),
            prepared: self.prepared.clone(),
            census: self.census.clone(),
        })
    }
    /// Retain this metadata with the destination census when abandoning B. It
    /// becomes Closed only after actual last-use cleanup, never local Drop/EOF.
    pub fn retirement_observer(
        &self,
    ) -> std::io::Result<codex_alab_core::native_reader_retirement::Observer> {
        self.construction
            .as_ref()
            .ok_or_else(codex_alab_core::managed_auth::unavailable)?
            .retirement_observer()
    }
    pub fn validate(&self) -> std::io::Result<()> {
        self.census
            .validate()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        if !Arc::ptr_eq(self.census.domain(), &self.domain()) {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        self.prepared.validate()?;
        self.census
            .resource_scope(|| {
                self.manager
                    .alab_verify_native_target(&self.prepared.target)
            })
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?
    }
    /// Destination-only initialization; ordinary turns/starts remain sealed.
    /// Native target/member proofs still authorize every actual credential use.
    pub async fn with_resources<F: std::future::Future>(
        &self,
        future: F,
    ) -> std::io::Result<F::Output> {
        self.validate()?;
        self.census
            .scope_resources(future)
            .await
            .map_err(|_| codex_alab_core::managed_auth::unavailable())
    }
    /// Actual cached B/Ready metadata only. No SOURCE RPC or file read is made
    /// under A's fence, and this grants no resource completion or publication.
    pub async fn current_binding(&self) -> std::io::Result<codex_alab_core::assignment::Binding> {
        self.validate()?;
        let binding = self.manager.alab_destination_binding().await?;
        self.validate()?;
        Ok(binding)
    }
}
impl Drop for DestinationEpoch {
    fn drop(&mut self) {
        if let Some(receipt) = self.construction.take() {
            let census = self.census.clone();
            census.close_resources();
            // Metadata contexts may retain Arc<M>. Cleanup drains real uses and
            // explicitly retires this exact member under the closed role.
            self.runtime.spawn(async move {
                census.abandon(receipt).await;
            });
        }
    }
}
impl AuthManager {
    pub fn alab_prepare_destination_policy(
        self: &Arc<Self>,
        target: &Target,
    ) -> std::io::Result<PreparedDestination> {
        let source = self.alab_capture_current_manager()?;
        source.alab_check_native_target(target)?;
        let (epoch, witness) = {
            let cached = source
                .inner
                .read()
                .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
            let Some(CodexAuth::Chatgpt(auth)) = cached.auth.as_ref() else {
                return Err(codex_alab_core::managed_auth::unavailable());
            };
            let epoch = source
                .alab_source_epoch()
                .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
            if !auth
                .state
                .alab_epoch
                .epoch()
                .is_some_and(|captured| captured.same(&epoch))
            {
                return Err(codex_alab_core::managed_auth::unavailable());
            }
            let tokens = auth
                .current_token_data()
                .ok_or_else(codex_alab_core::managed_auth::unavailable)?;
            (
                epoch,
                super::alab_native_source::target_policy::Witness::from_tokens(&tokens)?,
            )
        };
        let prepared = PreparedDestination {
            source,
            control: self.clone(),
            epoch,
            target: target.clone(),
            witness,
        };
        prepared.validate()?;
        Ok(prepared)
    }
    pub async fn alab_prepare_destination(
        self: &Arc<Self>,
        prepared: PreparedDestination,
        destination: Arc<dyn Destination>,
        seal: &Seal,
        mut destination_config: AuthConfig,
    ) -> std::io::Result<DestinationEpoch> {
        prepared.validate()?;
        let source = &prepared.source;
        if !Arc::ptr_eq(&self.alab_capture_current_manager()?, source) {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        let target = &prepared.target;
        target.validate(&target.operation)?;
        let context = destination.context();
        if context.assignment() != target.operation
            || context.target_ref() != target.target_ref
            || context.assignment_generation() <= target.expected_assignment
        {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        let policy = destination_config
            .auth_route_config
            .application_network_policy();
        if !policy.is_managed()
            || (source
                .auth_route_config
                .application_network_policy()
                .is_managed()
                && policy
                    .clone()
                    .with_account_binding(source.auth_route_config.application_network_policy())
                    .is_some())
        {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        // Only the resource owner's fresh account policy/local bootstrap routing
        // varies. Every actual source-manager constraint remains in force.
        destination_config.codex_home = source.codex_home.clone();
        destination_config.auth_credentials_store_mode = source.auth_credentials_store_mode;
        destination_config.keyring_backend_kind = source.keyring_backend_kind;
        destination_config.forced_login_method = source.forced_login_method;
        destination_config.forced_chatgpt_workspace_id = source.forced_chatgpt_workspace_id();
        destination_config.managed_auth_policy = source.managed_auth_policy.clone();
        destination_config.chatgpt_base_url = source.chatgpt_base_url.clone();
        let modes = source
            .allowed_auth_modes
            .read()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?
            .clone();
        let factory = seal
            .destination_factory(&prepared.epoch.domain(), destination.clone())
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        if !factory.protects_old_domain(&prepared.epoch.domain()) {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        let operation = format!("{:032x}", rand::random::<u128>());
        factory
            .bind_constructor(&operation)
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        let (reply, result) = tokio::sync::oneshot::channel();
        // The actor owns constructor/census before the first native OPEN. Losing
        // its waiter cannot cancel a pending PRELOAD/READY or forget cleanup.
        tokio::spawn(async move {
            let attempted = factory
                .scope(codex_alab_core::native_target::construct(
                    destination,
                    &operation,
                    async {
                        let manager =
                            AuthManager::new_from_auth_config(destination_config, false, modes)
                                .await;
                        alab_manager_catalog::attach(Arc::new(manager))
                    },
                ))
                .await;
            let constructed = match attempted {
                Err(_) => {
                    let _ = factory.retire_unstarted();
                    let _ = reply.send(Err(codex_alab_core::managed_auth::unavailable()));
                    return;
                }
                Ok(Err(failure)) => {
                    let census = Arc::new(factory.failed_owned());
                    let error = failure.error;
                    let receipt = failure.receipt;
                    tokio::spawn(async move {
                        census.abandon(receipt).await;
                    });
                    let _ = reply.send(Err(error));
                    return;
                }
                Ok(Ok(constructed)) => constructed,
            };
            let (census, completed) = factory.complete_owned();
            let pending = DestinationEpoch {
                manager: constructed.value,
                prepared: Arc::new(prepared),
                census: Arc::new(census),
                construction: Some(constructed.receipt),
                runtime: tokio::runtime::Handle::current(),
            };
            if completed.is_err() || pending.validate().is_err() {
                drop(pending);
                let _ = reply.send(Err(codex_alab_core::managed_auth::unavailable()));
                return;
            }
            // A cancelled receiver drops this pending epoch, whose cleanup actor
            // retains the real B domain and old census until actual Closed.
            let _ = reply.send(Ok(pending));
        });
        result
            .await
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?
    }
}
